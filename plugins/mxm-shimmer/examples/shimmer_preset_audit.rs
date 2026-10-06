//! Measures what every shipped factory preset does through the plugin's real parameter and DSP
//! adaptation path.
//!
//! ```bash
//! cargo run -p mxm-shimmer --release --example shimmer_preset_audit
//! ```
//!
//! A 30-second wet impulse reports onset, envelope peak, -60 dB span, density, stereo width and
//! high-frequency share. A six-second 220 Hz burst reports whether successive shifted returns have
//! overtaken the source by two seconds. These descriptors do not approve character; they prevent a
//! stronger Init or a DSP-law change from silently collapsing sparse preset overrides into one sound.

use mxm_shimmer::params::MxmShimmerParams;
use mxm_shimmer::{MxmShimmer, preset};
use nice_plug::params::InternalParamMut;
use nice_plug::prelude::*;

const FS: f32 = 48_000.0;
const IMPULSE_SECONDS: usize = 30;
const TONE_SECONDS: usize = 6;
const WINDOW: usize = 4_800;
const DENSITY_WINDOW: usize = 960;
const MIN_DESCRIPTOR_DISTANCE: f64 = 0.60;

struct ApplyingHost;

impl nice_plug::context::gui::GuiContextInner for ApplyingHost {
    // A test double has no host to ask for a restart (nice-plug 0.4).
    fn request_restart(&self) {}
    fn plugin_api(&self) -> PluginApi {
        PluginApi::Clap
    }

    unsafe fn raw_begin_set_parameter(&self, _param: nice_plug::params::internals::ParamPtr) {}

    unsafe fn raw_set_parameter_normalized(
        &self,
        param: nice_plug::params::internals::ParamPtr,
        value: f32,
    ) {
        unsafe { param._internal_set_normalized_value(value) };
    }

    unsafe fn raw_end_set_parameter(&self, _param: nice_plug::params::internals::ParamPtr) {}

    fn get_state(&self) -> PluginState {
        PluginState {
            version: String::new(),
            params: Default::default(),
            fields: Default::default(),
        }
    }

    fn set_state(&self, _: PluginState) {}
}

fn settle_smoothers(params: &MxmShimmerParams) {
    unsafe {
        for parameter in [
            &params.mix,
            &params.regen,
            &params.shimmer,
            &params.shift,
            &params.size,
            &params.diffusion,
            &params.mod_rate,
            &params.mod_depth,
            &params.low_cut,
            &params.high_cut,
            &params.pre_delay,
        ] {
            parameter._internal_update_smoother(FS, true);
        }
    }
}

fn load(text: &str) -> MxmShimmer {
    let mut plugin = MxmShimmer::default();
    let parsed = preset::Preset::parse(text, mxm_shimmer::CLAP_ID).expect("factory preset parses");
    let host = ApplyingHost;
    let setter = ParamSetter::new(&host);
    let (writes, problems) = parsed.resolve(plugin.params.as_ref());
    assert!(problems.is_empty(), "{problems:?}");
    for (_, parameter, value) in writes {
        parameter.set(&setter, value);
    }
    // The audit compares wet structures rather than small factory Mix differences.
    unsafe { plugin.params.mix._internal_set_normalized_value(1.0) };
    settle_smoothers(&plugin.params);
    plugin.prepare_for_test(FS, 2);
    plugin
}

fn render(text: &str, seconds: usize, tone: bool) -> Vec<(f32, f32)> {
    let mut plugin = load(text);
    let mut output = Vec::with_capacity(FS as usize * seconds);
    for block_start in (0..FS as usize * seconds).step_by(256) {
        let frames = 256.min(FS as usize * seconds - block_start);
        let mut left = vec![0.0; frames];
        let mut right = vec![0.0; frames];
        for frame in 0..frames {
            let sample = block_start + frame;
            let input = if tone {
                if sample < FS as usize / 4 {
                    (core::f32::consts::TAU * 220.0 * sample as f32 / FS).sin() * 0.1
                } else {
                    0.0
                }
            } else if sample == 0 {
                0.5
            } else {
                0.0
            };
            left[frame] = input;
            right[frame] = input;
        }
        {
            let mut channels: Vec<&mut [f32]> = vec![&mut left, &mut right];
            plugin.process_block_for_test(&mut channels);
        }
        output.extend(left.into_iter().zip(right));
    }
    output
}

fn tone_magnitude(samples: &[(f32, f32)], start: usize, count: usize, frequency: f32) -> f64 {
    let mut energy = 0.0;
    for channel in 0..2 {
        let mut real = 0.0;
        let mut imaginary = 0.0;
        for (sample, &(left, right)) in samples[start..start + count].iter().enumerate() {
            let value = if channel == 0 { left } else { right } as f64;
            let phase = core::f64::consts::TAU * frequency as f64 * sample as f64 / FS as f64;
            real += value * phase.cos();
            imaginary -= value * phase.sin();
        }
        energy += real * real + imaginary * imaginary;
    }
    energy.sqrt() / count as f64
}

#[derive(Clone)]
struct Metrics {
    name: &'static str,
    shift_st: f64,
    onset_ms: f64,
    peak_s: f64,
    rt60_s: f64,
    rms_1s_db: f64,
    rms_5s_db: f64,
    density_pct: f64,
    width_db: f64,
    brightness: f64,
    climb_db: f64,
}

fn decibels(ratio: f64) -> f64 {
    20.0 * ratio.max(1.0e-12).log10()
}

fn measure(name: &'static str, text: &str) -> Metrics {
    let configured = load(text);
    let shift_st = f64::from(configured.params.shift.value());
    drop(configured);
    let impulse = render(text, IMPULSE_SECONDS, false);
    let peak_sample = impulse
        .iter()
        .map(|&(left, right)| left.abs().max(right.abs()))
        .fold(0.0, f32::max);
    let onset = impulse
        .iter()
        .position(|&(left, right)| left.abs().max(right.abs()) > 1.0e-7)
        .unwrap_or_default();
    let mut rms = Vec::new();
    let smoothing = 1.0 - (-core::f32::consts::TAU * 6_000.0 / FS).exp();
    let mut low = [0.0; 2];
    let (mut total_energy, mut high_energy) = (0.0, 0.0);
    let (mut mid_energy, mut side_energy) = (0.0, 0.0);
    for (window, samples) in impulse.as_chunks::<WINDOW>().0.iter().enumerate() {
        let mut energy = 0.0;
        for (frame, &(left, right)) in samples.iter().enumerate() {
            energy += f64::from(left * left + right * right);
            let sample = window * WINDOW + frame;
            if (FS as usize / 2..FS as usize * 3).contains(&sample) {
                for (channel, value) in [left, right].into_iter().enumerate() {
                    low[channel] += smoothing * (value - low[channel]);
                    let high = value - low[channel];
                    total_energy += f64::from(value * value);
                    high_energy += f64::from(high * high);
                }
                let mid = 0.5 * (left + right);
                let side = 0.5 * (left - right);
                mid_energy += f64::from(mid * mid);
                side_energy += f64::from(side * side);
            }
        }
        rms.push((energy / (2 * WINDOW) as f64).sqrt());
    }
    let (peak_window, peak_rms) = rms
        .iter()
        .copied()
        .enumerate()
        .max_by(|left, right| left.1.total_cmp(&right.1))
        .expect("measured windows");
    let rt60_window = rms
        .iter()
        .rposition(|&value| value > peak_rms * 0.001)
        .unwrap_or_default();
    let density_threshold = peak_sample * 1.0e-4;
    // Measure the first 20 ms after each sound's own onset rather than at an absolute second,
    // where every current design is already fully dense and the descriptor says nothing about
    // Diffusion.
    let density_start = onset.min(impulse.len() - DENSITY_WINDOW);
    let density = impulse[density_start..density_start + DENSITY_WINDOW]
        .iter()
        .filter(|&&(left, right)| left.abs().max(right.abs()) > density_threshold)
        .count();

    let tone = render(text, TONE_SECONDS, true);
    let start = FS as usize * 2;
    let count = FS as usize / 2;
    let source = tone_magnitude(&tone, start, count, 220.0);
    let shift_ratio = 2.0_f32.powf(shift_st as f32 / 12.0);
    let shifted_once = 220.0 * shift_ratio;
    let shifted_twice = shifted_once * shift_ratio;
    let ascent = tone_magnitude(&tone, start, count, shifted_once)
        + tone_magnitude(&tone, start, count, shifted_twice);

    Metrics {
        name,
        shift_st,
        onset_ms: onset as f64 * 1_000.0 / FS as f64,
        peak_s: peak_window as f64 * 0.1,
        rt60_s: rt60_window as f64 * 0.1,
        rms_1s_db: decibels(rms[10] / peak_rms),
        rms_5s_db: decibels(rms[50] / peak_rms),
        density_pct: density as f64 * 100.0 / DENSITY_WINDOW as f64,
        width_db: decibels((side_energy / mid_energy.max(1.0e-30)).sqrt()),
        brightness: (high_energy / total_energy.max(1.0e-30)).sqrt(),
        climb_db: decibels(ascent / source.max(1.0e-30)),
    }
}

fn descriptor_distance(left: &Metrics, right: &Metrics) -> f64 {
    let terms = [
        (left.shift_st - right.shift_st) / 12.0,
        (left.onset_ms - right.onset_ms) / 100.0,
        left.peak_s - right.peak_s,
        (left.rt60_s - right.rt60_s) / 10.0,
        (left.rms_1s_db - right.rms_1s_db) / 20.0,
        (left.rms_5s_db - right.rms_5s_db) / 40.0,
        (left.density_pct - right.density_pct) / 50.0,
        (left.width_db - right.width_db) / 3.0,
        (left.brightness - right.brightness) / 0.2,
        (left.climb_db - right.climb_db) / 40.0,
    ];
    terms
        .into_iter()
        .map(|term| term * term)
        .sum::<f64>()
        .sqrt()
}

fn main() {
    println!(
        "name,shift_st,onset_ms,peak_s,rt60_s,rms_1s_db,rms_5s_db,density_after_onset_pct,width_db,brightness,climb_2s_db"
    );
    let metrics: Vec<_> = preset::FACTORY_FILES
        .iter()
        .map(|&(name, text)| measure(name, text))
        .collect();
    for measured in &metrics {
        println!(
            "{},{:.0},{:.2},{:.2},{:.2},{:.2},{:.2},{:.1},{:.2},{:.4},{:.2}",
            measured.name,
            measured.shift_st,
            measured.onset_ms,
            measured.peak_s,
            measured.rt60_s,
            measured.rms_1s_db,
            measured.rms_5s_db,
            measured.density_pct,
            measured.width_db,
            measured.brightness,
            measured.climb_db,
        );
    }
    // No shipped sound may be a drone. Regen can take the loop past unity now, and the threshold
    // moves with Size — loss is per lap, and a large space has fewer, longer laps — so a factory
    // value can cross it by accident. `rt60_window` is the last window still above -60 dB, so a
    // preset that never decays reports the final window rather than a real time.
    let droning: Vec<_> = metrics
        .iter()
        .filter(|measured| measured.rt60_s >= IMPULSE_SECONDS as f64 - 0.15)
        .map(|measured| measured.name)
        .collect();
    assert!(
        droning.is_empty(),
        "still ringing at the end of a {IMPULSE_SECONDS} s render, i.e. self-oscillating: {droning:?}"
    );

    let closest = metrics
        .iter()
        .enumerate()
        .flat_map(|(index, left)| {
            metrics[index + 1..]
                .iter()
                .map(move |right| (descriptor_distance(left, right), left.name, right.name))
        })
        .min_by(|left, right| left.0.total_cmp(&right.0))
        .expect("more than one preset");
    eprintln!(
        "closest descriptor pair: {} / {} at {:.3}; rendered {} samples",
        closest.1,
        closest.2,
        closest.0,
        preset::FACTORY_FILES.len() * (IMPULSE_SECONDS + TONE_SECONDS) * FS as usize,
    );
    assert!(
        closest.0 >= MIN_DESCRIPTOR_DISTANCE,
        "{} and {} collapsed to descriptor distance {:.3}, below {MIN_DESCRIPTOR_DISTANCE:.2}",
        closest.1,
        closest.2,
        closest.0,
    );
}
