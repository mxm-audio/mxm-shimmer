//! `mxm-shimmer` — a pitch-shifted feedback reverb.
//!
//! The pitch shifter can read the input, the regenerating return, or both. Input gives a stable
//! harmony layer; Regen sends the shifted signal around again so successive passes climb. The DSP
//! follows the documented shimmer-reverb practice, not a proprietary product's constants.

macro_rules! plugin_name {
    () => {
        "mxm-shimmer"
    };
}

pub const NAME: &str = plugin_name!();
pub const CLAP_ID: &str = concat!("dk.mxm.", plugin_name!());

pub mod editor;
pub mod params;
pub mod preset;
pub mod telemetry;

use mxm_shimmer_dsp::{Controls, Engine};
use nice_plug::prelude::*;
use params::MxmShimmerParams;
use std::sync::Arc;
use telemetry::Telemetry;

pub struct MxmShimmer {
    pub params: Arc<MxmShimmerParams>,
    telemetry: Arc<Telemetry>,
    engine: Engine,
    sample_rate: f32,
    input_channels: usize,
    /// Pre-delay and mod rate as their syncs resolved them for this block, or `None` for their
    /// free values, and the free values they were resolved beside (`follow_tempo`).
    synced: [Option<f32>; 2],
    free_seen: [f32; 2],
}

impl Default for MxmShimmer {
    fn default() -> Self {
        Self {
            params: Arc::new(MxmShimmerParams::default()),
            telemetry: Telemetry::shared(),
            engine: Engine::new(48_000.0),
            sample_rate: 48_000.0,
            input_channels: 1,
            synced: [None; 2],
            free_seen: [0.0; 2],
        }
    }
}

impl MxmShimmer {
    fn prepare(&mut self, sample_rate: f32, input_channels: usize) {
        // Forget the last activation's tempo too: nice-plug resets right after activating, and a
        // division resolved from a tempo the host may since have changed would seed the engine.
        self.telemetry.tempo.publish(None);
        self.sample_rate = valid_sample_rate(sample_rate);
        self.input_channels = input_channels.clamp(1, 2);
        self.engine = Engine::new(self.sample_rate);
        // Activation resets the smoothers to the knobs, so a sync in force is re-aimed next block.
        self.synced = [None; 2];
        // State may have been restored before activation. Discrete topology and DSP controls must
        // begin at that state, not at the Rust default followed by an audible transition.
        self.engine.set_controls(self.target_controls());
        self.engine.reset();
    }

    fn controls(&self) -> Controls {
        let p = &self.params;
        Controls {
            mix: p.mix.smoothed.next(),
            regen: p.regen.smoothed.next(),
            shimmer: p.shimmer.smoothed.next(),
            shift_semitones: p.shift.smoothed.next(),
            placement: p.placement.value().dsp(),
            reverse: p.reverse.value(),
            freeze: p.freeze.value(),
            size: p.size.smoothed.next(),
            diffusion: p.diffusion.smoothed.next(),
            mod_rate_hz: p.mod_rate.smoothed.next(),
            mod_depth_s: p.mod_depth.smoothed.next(),
            low_cut_hz: p.low_cut.smoothed.next(),
            high_cut_hz: p.high_cut.smoothed.next(),
            pre_delay_s: p.pre_delay.smoothed.next(),
        }
    }

    fn target_controls(&self) -> Controls {
        let p = &self.params;
        Controls {
            mix: p.mix.value(),
            regen: p.regen.value(),
            shimmer: p.shimmer.value(),
            shift_semitones: p.shift.value(),
            placement: p.placement.value().dsp(),
            reverse: p.reverse.value(),
            freeze: p.freeze.value(),
            size: p.size.value(),
            diffusion: p.diffusion.value(),
            mod_rate_hz: self.synced[1].unwrap_or_else(|| p.mod_rate.value()),
            mod_depth_s: p.mod_depth.value(),
            low_cut_hz: p.low_cut.value(),
            high_cut_hz: p.high_cut.value(),
            pre_delay_s: self.synced[0].unwrap_or_else(|| p.pre_delay.value()),
        }
    }

    /// **The tempo syncs, once a block** (`plans/plan-tempo-sync-controls.md`), through each control's
    /// own smoother: the engine reads pre-delay as a delay-line position every sample, so a division
    /// arriving at once would click. The wrapper aims a smoother at the knob whenever the knob moves;
    /// synced, it is aimed back at the division, and when sync goes off, at the knob again — so a
    /// synced control glides exactly as a free one does, and a free one is untouched.
    fn follow_tempo(&mut self, tempo: Option<f64>) {
        let p = Arc::clone(&self.params);
        let resolved = [p.synced_pre_delay(tempo), p.synced_mod_rate(tempo)];
        for (i, param) in [&p.pre_delay, &p.mod_rate].into_iter().enumerate() {
            let free = param.value();
            let moved = free != self.free_seen[i];
            if resolved[i] != self.synced[i] || (resolved[i].is_some() && moved) {
                param
                    .smoothed
                    .set_target(self.sample_rate, resolved[i].unwrap_or(free));
            }
            self.free_seen[i] = free;
        }
        self.synced = resolved;
        self.telemetry.tempo.publish(tempo);
    }

    fn tail_samples(&self) -> u32 {
        (self.engine.remaining_tail_seconds() * self.sample_rate).clamp(0.0, u32::MAX as f32) as u32
    }

    pub fn prepare_for_test(&mut self, sample_rate: f32, input_channels: usize) {
        self.prepare(sample_rate, input_channels);
    }

    pub fn process_block_for_test(&mut self, channels: &mut [&mut [f32]]) -> ProcessStatus {
        self.process_block(channels)
    }

    fn process_block(&mut self, channels: &mut [&mut [f32]]) -> ProcessStatus {
        let Some(first) = channels.first() else {
            return ProcessStatus::Normal;
        };
        let samples = first.len();
        let mono = self.input_channels == 1 || channels.len() < 2;
        let inputs = self.input_channels.min(channels.len());
        let mut has_input = false;
        for channel in &mut channels[..inputs] {
            for sample in &mut channel[..samples] {
                if sample.abs() < f32::MIN_POSITIVE || !sample.is_finite() {
                    *sample = 0.0;
                } else {
                    has_input = true;
                }
            }
        }

        if !has_input && self.engine.is_parked() {
            if mono && channels.len() > 1 {
                let (left, rest) = channels.split_at_mut(1);
                rest[0][..samples].copy_from_slice(&left[0][..samples]);
            }
            return ProcessStatus::Normal;
        }

        let mut peak = 0.0f32;
        let mut shifted_energy = 0.0f32;
        for index in 0..samples {
            let controls = self.controls();
            self.engine.set_controls(controls);
            let dry_l = channels[0][index];
            let dry_r = if mono { dry_l } else { channels[1][index] };
            let (out_l, out_r) = self.engine.process(dry_l, dry_r);
            peak = peak.max(out_l.abs()).max(out_r.abs());
            shifted_energy = shifted_energy
                .max((out_l - dry_l).abs())
                .max((out_r - dry_r).abs());
            channels[0][index] = out_l;
            if channels.len() > 1 {
                channels[1][index] = out_r;
            }
        }
        if channels.len() > 2 {
            let (left, rest) = channels.split_at_mut(1);
            for channel in rest.iter_mut().skip(1) {
                channel[..samples].copy_from_slice(&left[0][..samples]);
            }
        }
        self.telemetry.publish(peak, shifted_energy);

        if has_input || self.engine.is_sustaining() || self.engine.is_parked() {
            ProcessStatus::Normal
        } else {
            ProcessStatus::Tail(self.tail_samples())
        }
    }
}

impl Plugin for MxmShimmer {
    const NAME: &'static str = NAME;
    const VENDOR: &'static str = "mxm";
    const URL: &'static str = "https://mxm.dk";
    const EMAIL: &'static str = "plugins@mxm.dk";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
    ];
    const MIDI_INPUT: MidiConfig = MidiConfig::None;
    const SAMPLE_ACCURATE_AUTOMATION: bool = false;

    type Editor = editor::MxmShimmerEditor;
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Self::Editor> {
        editor::create(self.params.clone(), self.telemetry.clone())
    }

    fn activate(
        &mut self,
        layout: &AudioIOLayout,
        config: &BufferConfig,
        _context: &mut impl ActivateContext<Self>,
    ) -> bool {
        self.prepare(
            config.sample_rate,
            layout
                .main_input_channels
                .map_or(1, |channels| channels.get() as usize),
        );
        true
    }

    fn reset(&mut self) {
        // Parameter flushes can arrive while a host has the effect bypassed and unprocessed.
        // Synchronise those targets before resetting so the next excitation cannot begin in the
        // previous topology.
        // A host resets without a callback between (a bypass, a transport restart), and a parameter
        // flush may have moved a sync meanwhile: re-resolve every sync from the parameters as they
        // stand and the last tempo seen, so nothing is seeded from the previous division.
        // Its two synced controls ride their own smoothers, which a reset does not touch: where a
        // sync is or was involved, each is set at its target, as the rest of the DSP state is cleared.
        let before = self.synced;
        self.follow_tempo(self.telemetry.tempo.get());
        let p = Arc::clone(&self.params);
        for (i, param) in [&p.pre_delay, &p.mod_rate].into_iter().enumerate() {
            if before[i].is_some() || self.synced[i].is_some() {
                param
                    .smoothed
                    .reset(self.synced[i].unwrap_or_else(|| param.value()));
            }
        }
        self.engine.set_controls(self.target_controls());
        self.engine.reset();
    }

    /// **A project saved before the tempo syncs** restores each Off rather than keeping this
    /// instance's, and a loaded preset's baseline gains it, so the preset stays clean
    /// (`mxm_preset::add_switches_off`).
    fn filter_state(state: &mut PluginState) {
        mxm_preset::add_switches_off(state, crate::preset::TEMPO_SYNC_IDS);
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        self.follow_tempo(context.transport().tempo);
        self.process_block(buffer.as_slice())
    }
}

impl ClapPlugin for MxmShimmer {
    const CLAP_ID: &'static str = CLAP_ID;
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("A shimmering reverb whose tail climbs in pitch");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Reverb,
        ClapFeature::Stereo,
    ];
}

nice_export_clap!(MxmShimmer);

fn valid_sample_rate(sample_rate: f32) -> f32 {
    if sample_rate.is_finite() {
        sample_rate.clamp(8_000.0, 384_000.0)
    } else {
        48_000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nice_plug::params::{InternalParamMut, Param};

    fn prepared() -> MxmShimmer {
        let mut plugin = MxmShimmer::default();
        for param in [
            &plugin.params.mix,
            &plugin.params.regen,
            &plugin.params.shimmer,
            &plugin.params.shift,
            &plugin.params.size,
            &plugin.params.diffusion,
            &plugin.params.mod_rate,
            &plugin.params.mod_depth,
            &plugin.params.low_cut,
            &plugin.params.high_cut,
            &plugin.params.pre_delay,
        ] {
            unsafe { param._internal_update_smoother(48_000.0, true) };
        }
        plugin.prepare(48_000.0, 1);
        plugin
    }

    fn set(param: &FloatParam, value: f32) {
        unsafe {
            let normalised = param.preview_normalized(value);
            let _ = param._internal_set_normalized_value(normalised);
            param._internal_update_smoother(48_000.0, true);
        }
    }

    fn set_bool(param: &BoolParam, value: bool) {
        unsafe {
            let _ = param._internal_set_normalized_value(if value { 1.0 } else { 0.0 });
        }
    }

    /// **A synced pre-delay glides to its division, and back to the knob**, through the control's
    /// own smoother: the engine reads it as a delay-line position every sample, so a jump would
    /// click. A knob moved while synced does not pull it off the division.
    #[test]
    fn a_synced_pre_delay_glides_to_its_division_and_back_to_the_knob() {
        let mut plugin = prepared();
        let settle = |plugin: &MxmShimmer| {
            for _ in 0..48_000 {
                plugin.params.pre_delay.smoothed.next();
            }
            plugin.params.pre_delay.smoothed.next()
        };
        set(&plugin.params.pre_delay, 0.030);
        plugin.follow_tempo(Some(120.0));
        assert_eq!(
            settle(&plugin),
            plugin.params.pre_delay.value(),
            "free is the knob"
        );

        set_bool(&plugin.params.pre_delay_sync, true);
        plugin.follow_tempo(Some(120.0));
        let division = plugin.params.synced_pre_delay(Some(120.0)).expect("synced");
        let first = plugin.params.pre_delay.smoothed.next();
        assert!(
            (first - division).abs() > 1e-4,
            "the division arrived at once: {first}"
        );
        assert!((settle(&plugin) - division).abs() < 1e-6);

        // The host moves the knob to a value on the same division: the wrapper aims the smoother at
        // the knob, and the next block aims it back.
        set(&plugin.params.pre_delay, 0.031);
        plugin.follow_tempo(Some(120.0));
        let division = plugin.params.synced_pre_delay(Some(120.0)).expect("synced");
        assert!((settle(&plugin) - division).abs() < 1e-6);

        set_bool(&plugin.params.pre_delay_sync, false);
        plugin.follow_tempo(Some(120.0));
        assert!((settle(&plugin) - plugin.params.pre_delay.value()).abs() < 1e-6);
    }

    #[test]
    fn mix_zero_is_the_dry_to_the_bit_in_both_layouts() {
        let left_input: Vec<f32> = (0..4096).map(|n| (n as f32 * 0.13).sin() * 0.4).collect();
        let right_input: Vec<f32> = (0..4096).map(|n| (n as f32 * 0.07).cos() * 0.3).collect();
        for input_channels in [1, 2] {
            let mut plugin = prepared();
            set(&plugin.params.mix, 0.0);
            plugin.prepare(48_000.0, input_channels);
            let mut left = left_input.clone();
            let mut right = right_input.clone();
            let mut channels: Vec<&mut [f32]> = vec![&mut left, &mut right];
            plugin.process_block(&mut channels);
            assert_eq!(left, left_input);
            let expected_right = if input_channels == 1 {
                left_input.as_slice()
            } else {
                right_input.as_slice()
            };
            assert_eq!(
                right.as_slice(),
                expected_right,
                "layout with {input_channels} input channel(s)"
            );
        }
    }

    #[test]
    fn a_tail_is_reported_and_a_live_freeze_is_sustaining() {
        let mut plugin = prepared();
        let mut left = vec![0.0; 4096];
        left[0] = 0.8;
        let mut right = vec![0.0; 4096];
        {
            let mut channels: Vec<&mut [f32]> = vec![&mut left, &mut right];
            plugin.process_block(&mut channels);
        }
        left.fill(0.0);
        right.fill(0.0);
        {
            let mut channels: Vec<&mut [f32]> = vec![&mut left, &mut right];
            assert!(matches!(
                plugin.process_block(&mut channels),
                ProcessStatus::Tail(_)
            ));
        }
        // `process_block` is in place: erase the previous block's output before claiming this is a
        // silent-input freeze check, or that output becomes fresh excitation on the next call.
        left.fill(0.0);
        right.fill(0.0);
        set_bool(&plugin.params.freeze, true);
        let mut channels: Vec<&mut [f32]> = vec![&mut left, &mut right];
        assert_eq!(plugin.process_block(&mut channels), ProcessStatus::Normal);
    }

    #[test]
    fn the_name_and_id_have_one_source() {
        assert_eq!(NAME, "mxm-shimmer");
        assert_eq!(CLAP_ID, format!("dk.mxm.{NAME}"));
    }

    #[test]
    fn the_bundle_is_named_after_this_plugin() {
        mxm_plugin_test::bundle::is_named(env!("CARGO_MANIFEST_DIR"), env!("CARGO_PKG_NAME"), NAME);
    }
}

/// **A reset re-resolves the tempo syncs**: a sync turned off while the host held the effect
/// unprocessed does not seed the reset from the previous division, and one still on stays on it.
#[cfg(test)]
mod reset_resolves_the_syncs {
    use super::*;
    use nice_plug::params::InternalParamMut;

    #[test]
    fn a_reset_re_resolves_the_tempo_syncs() {
        let mut plugin = MxmShimmer::default();
        plugin.telemetry.tempo.publish(Some(120.0));
        unsafe {
            let _ = plugin
                .params
                .pre_delay_sync
                ._internal_set_normalized_value(1.0);
        }
        plugin.reset();
        let division = plugin.params.synced_pre_delay(Some(120.0)).expect("synced");
        assert_eq!(
            plugin.params.pre_delay.smoothed.next(),
            division,
            "set at the division"
        );
        // Turned off while unprocessed: the reset lands on the knob, not the old division.
        unsafe {
            let _ = plugin
                .params
                .pre_delay_sync
                ._internal_set_normalized_value(0.0);
        }
        plugin.reset();
        assert_eq!(
            plugin.params.pre_delay.smoothed.next(),
            plugin.params.pre_delay.value()
        );
    }

    /// **Reactivation forgets the old tempo**: nice-plug resets right after activating, and that
    /// reset must not resolve from the tempo the host reported before it was deactivated — the first
    /// callback's tempo is the first one used.
    #[test]
    fn reactivation_forgets_the_previous_tempo() {
        let mut plugin = MxmShimmer::default();
        plugin.telemetry.tempo.publish(Some(120.0));
        unsafe {
            let _ = plugin
                .params
                .pre_delay_sync
                ._internal_set_normalized_value(1.0);
        }
        plugin.prepare(48_000.0, 2);
        plugin.reset();
        assert_eq!(plugin.synced, [None; 2]);
    }
}

/// What a player reads — on hover in the editor, and in a host's plugin browser — speaks to the
/// player about the sound, never about the machine or the code (`mxm_plugin_test::hover_text`).
#[cfg(test)]
mod speaks_to_the_player {
    #[test]
    fn hover_text() {
        mxm_plugin_test::hover_text::speaks_to_the_player(env!("CARGO_MANIFEST_DIR"));
    }

    #[test]
    fn host_description() {
        mxm_plugin_test::hover_text::host_description_speaks_to_the_player(env!(
            "CARGO_MANIFEST_DIR"
        ));
    }
}
