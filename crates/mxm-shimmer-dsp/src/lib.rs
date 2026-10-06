//! `mxm-shimmer` DSP: a swept-delay pitch shifter around a bounded stereo reverb core.
//!
//! This crate contains no plugin-framework types and has no dependencies. The topology follows the
//! pitch-shifted feedback practice documented at `research:effects/shimmer-reverb.md`; all delay
//! times and gains are this implementation's own chosen or measured constants.

mod delay;
pub mod reverb;
pub mod shifter;

use delay::{Delay, bounded, flush, follower_coefficient};
use reverb::ReverbCore;
use shifter::SweptShifter;

const TRANSITION_S: f32 = 0.025;
const MAX_PREDELAY_S: f32 = 0.250;
const QUIET_LEVEL: f32 = 1.0e-7;
const QUIET_HOLD_S: f32 = 0.75;
// How long the activity follower takes to rise and to fall. Times, for the reason
// `reverb::LEVEL_RELEASE_S` gives at greater length: these were fixed per-sample coefficients of
// 0.05 and 0.0002 — twenty and five thousand samples, 0.4 ms and 104 ms at 48 kHz but 2.5 ms and
// 625 ms at 8 kHz — while `QUIET_HOLD_S` three lines of `process` below them was already converted
// from the rate. This follower is the slower of the two and is what decides `is_quiet`, so it set
// the low-rate park delay. The values are what the coefficients were at 48 kHz.
const ACTIVITY_ATTACK_S: f32 = 0.000_4;
const ACTIVITY_RELEASE_S: f32 = 0.104;
// How much of the filtered return re-enters the core. This is the shimmer's own path: the ascent
// exists because the shifted return is fed back and shifted again, so this gain decides how many
// octaves the tail actually climbs before the unshifted energy under it wins.
//
// Raised from 0.06 when the core stopped losing most of its energy each lap. The core used to
// decay fast enough that a small shifted return dominated what remained; now that the same Regen
// holds the unshifted tone for tens of seconds, 0.06 left the ascent measurably *below* the
// fundamental — a 220 Hz burst put only about a quarter of its energy into the octaves above.
// Measured at the default settings, as the ratio of energy in the two octaves above a 220 Hz burst
// to the energy left at the fundamental: 0.06 -> 0.26, 0.09 -> 0.74, 0.12 -> 1.6, 0.15 -> 3.1,
// 0.18 -> 5.2. Chosen at 0.15, which climbs convincingly while the default patch still decays to
// nothing well inside thirty seconds; 0.18 left it barely ringing at the end.
const OUTER_REGEN_SCALE: f32 = 0.15;
const MAX_SHIFT_LEVEL_COMPENSATION: f32 = 10.0;
// The share of the ordinary outer gain the *unshifted* leg keeps.
//
// The core already recirculates unshifted energy — that is exactly what Regen sets — so an outer
// path carrying it a second time is surplus loop gain with nothing to show for it. It was
// harmless while the core threw away most of its energy each lap; once the core held on to it,
// the same path turned a moderate Regen into a growing loop with Shimmer at zero. The shifted leg
// has no such duplicate: nothing else in the topology feeds a transposed copy back, which is why
// only this leg is reduced. Freeze bypasses both ordinary legs through its unshifted held return.
const UNSHIFTED_OUTER_SHARE: f32 = 0.25;

#[derive(Debug, Clone, Copy)]
struct Ramp {
    value: f32,
    target: f32,
    step: f32,
    remaining: u32,
}

impl Ramp {
    const fn new(value: f32) -> Self {
        Self {
            value,
            target: value,
            step: 0.0,
            remaining: 0,
        }
    }

    fn set_target(&mut self, target: f32, samples: u32) {
        self.target = target;
        self.remaining = samples.max(1);
        self.step = (target - self.value) / self.remaining as f32;
    }

    fn settle(&mut self, value: f32) {
        self.value = value;
        self.target = value;
        self.step = 0.0;
        self.remaining = 0;
    }

    #[inline]
    fn next(&mut self) -> f32 {
        if self.remaining > 0 {
            self.value += self.step;
            self.remaining -= 1;
            if self.remaining == 0 {
                self.value = self.target;
                self.step = 0.0;
            }
        }
        self.value
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReverseStage {
    Stable,
    FadeOut,
    FadeIn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    Input,
    Regen,
    Both,
}

#[derive(Debug, Clone, Copy)]
pub struct Controls {
    pub mix: f32,
    pub regen: f32,
    pub shimmer: f32,
    pub shift_semitones: f32,
    pub placement: Placement,
    pub reverse: bool,
    pub freeze: bool,
    pub size: f32,
    pub diffusion: f32,
    /// Base rate of the late field's delay modulation, in hertz, before the per-line stagger.
    pub mod_rate_hz: f32,
    /// Depth of that modulation, in seconds of delay excursion.
    pub mod_depth_s: f32,
    pub low_cut_hz: f32,
    pub high_cut_hz: f32,
    pub pre_delay_s: f32,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            mix: 0.48,
            regen: 0.68,
            shimmer: 0.85,
            shift_semitones: 12.0,
            placement: Placement::Regen,
            reverse: false,
            freeze: false,
            size: 0.78,
            diffusion: 0.90,
            // Chosen to land where the previous fixed law sat at the default Diffusion, so the
            // controls become adjustable without moving the sound they were adjusted around.
            mod_rate_hz: 0.16,
            mod_depth_s: 0.0013,
            low_cut_hz: 180.0,
            high_cut_hz: 8_000.0,
            pre_delay_s: 0.030,
        }
    }
}

pub struct Engine {
    sample_rate: f32,
    pre: [Delay; 2],
    core: ReverbCore,
    input_shift: [SweptShifter; 2],
    regen_shift: [SweptShifter; 2],
    filters: [LoopFilter; 2],
    controls: Controls,
    placement: Placement,
    input_gain: Ramp,
    regen_gain: Ramp,
    freeze_mix: Ramp,
    reverse_active: bool,
    reverse_target: bool,
    reverse_stage: ReverseStage,
    reverse_gain: Ramp,
    last_wet: [f32; 2],
    activity: f32,
    activity_attack: f32,
    activity_release: f32,
    quiet_samples: usize,
    parked: bool,
}

impl Engine {
    pub fn new(sample_rate: f32) -> Self {
        let sample_rate = valid_rate(sample_rate);
        let max_pre = (MAX_PREDELAY_S * sample_rate).ceil() as usize + 4;
        Self {
            sample_rate,
            pre: [Delay::new(max_pre), Delay::new(max_pre)],
            core: ReverbCore::new(sample_rate),
            input_shift: [
                SweptShifter::new(sample_rate, 0x91e1_0da5),
                SweptShifter::new(sample_rate, 0x7f4a_7c15),
            ],
            regen_shift: [
                SweptShifter::new(sample_rate, 0xa341_316c),
                SweptShifter::new(sample_rate, 0xc801_3ea4),
            ],
            filters: [LoopFilter::default(), LoopFilter::default()],
            controls: Controls::default(),
            placement: Placement::Regen,
            input_gain: Ramp::new(0.0),
            regen_gain: Ramp::new(1.0),
            freeze_mix: Ramp::new(0.0),
            reverse_active: false,
            reverse_target: false,
            reverse_stage: ReverseStage::Stable,
            reverse_gain: Ramp::new(1.0),
            last_wet: [0.0; 2],
            activity: 0.0,
            activity_attack: follower_coefficient(ACTIVITY_ATTACK_S, sample_rate),
            activity_release: follower_coefficient(ACTIVITY_RELEASE_S, sample_rate),
            quiet_samples: 0,
            parked: true,
        }
    }

    pub fn set_controls(&mut self, mut controls: Controls) {
        controls.mix = finite(controls.mix, 0.0, 1.0);
        // Regen reaches one. It is the loop gain, and the top of it is self-oscillation rather
        // than a merely long decay; the old 0.96 ceiling existed to keep the loop under unity.
        controls.regen = finite(controls.regen, 0.0, 1.0);
        controls.mod_rate_hz = finite(controls.mod_rate_hz, 0.0, 20.0);
        controls.mod_depth_s = finite(controls.mod_depth_s, 0.0, 0.02);
        controls.shimmer = finite(controls.shimmer, 0.0, 1.0);
        controls.shift_semitones = finite(controls.shift_semitones, 0.0, 24.0);
        controls.size = finite(controls.size, 0.0, 1.0);
        controls.diffusion = finite(controls.diffusion, 0.0, 1.0);
        // Keep the two cutoffs ordered even at the validator's 8 kHz stress rate. The published
        // parameter ranges extend above that rate's Nyquist limit, so clamping each independently
        // would eventually hand `f32::clamp()` an inverted range and panic on the audio thread.
        let high_ceiling = (0.45 * self.sample_rate).max(100.0);
        controls.low_cut_hz = finite(
            controls.low_cut_hz,
            20.0,
            4_000.0f32.min(high_ceiling - 20.0),
        );
        controls.high_cut_hz = finite(
            controls.high_cut_hz,
            (controls.low_cut_hz + 20.0).min(high_ceiling),
            high_ceiling,
        );
        controls.pre_delay_s = finite(controls.pre_delay_s, 0.0, MAX_PREDELAY_S);

        let transition_samples = (TRANSITION_S * self.sample_rate).round().max(1.0) as u32;
        if controls.placement != self.placement {
            let (new_in, new_regen) = placement_targets(controls.placement);
            if self.input_gain.value == 0.0 && new_in > 0.0 && !self.controls.freeze {
                self.input_shift[0].reset();
                self.input_shift[1].reset();
            }
            if self.regen_gain.value == 0.0 && new_regen > 0.0 {
                self.regen_shift[0].reset();
                self.regen_shift[1].reset();
            }
            if self.parked {
                // There is no live route to preserve. Starting the next note in the previous
                // placement would turn a silent edit or restored state into audible automation.
                self.input_gain.settle(new_in);
                self.regen_gain.settle(new_regen);
            } else {
                self.input_gain.set_target(new_in, transition_samples);
                self.regen_gain.set_target(new_regen, transition_samples);
            }
            self.placement = controls.placement;
        }
        if controls.reverse != self.reverse_target {
            self.reverse_target = controls.reverse;
            if self.parked {
                self.reverse_active = controls.reverse;
                self.reverse_stage = ReverseStage::Stable;
                self.reverse_gain.settle(1.0);
            } else if self.reverse_target == self.reverse_active {
                self.reverse_stage = ReverseStage::FadeIn;
                self.reverse_gain
                    .set_target(1.0, reverse_half_transition_samples(self.sample_rate));
            } else {
                self.reverse_stage = ReverseStage::FadeOut;
                self.reverse_gain
                    .set_target(0.0, reverse_half_transition_samples(self.sample_rate));
            }
        }
        if controls.freeze && !self.controls.freeze {
            // Freeze turns the input shifter into a one-pass injection path regardless of ordinary
            // placement. Clear history if that node was inactive so engaging Freeze cannot replay
            // an earlier Input placement into the held field.
            if self.input_gain.value == 0.0 {
                self.input_shift[0].reset();
                self.input_shift[1].reset();
            }
            // The core itself changes to an energy-preserving delay-bank loop. Engaging on silence
            // therefore creates no sound, while later input can still wake and join the held field.
            if self.parked {
                self.freeze_mix.settle(1.0);
            } else {
                self.freeze_mix.set_target(1.0, transition_samples);
            }
        } else if !controls.freeze && self.controls.freeze {
            if self.parked {
                self.freeze_mix.settle(0.0);
            } else {
                self.freeze_mix.set_target(0.0, transition_samples);
            }
        }
        self.controls = controls;
    }

    pub fn reset(&mut self) {
        for line in &mut self.pre {
            line.clear();
        }
        self.core.reset();
        self.reset_shifters();
        self.filters = [LoopFilter::default(), LoopFilter::default()];
        self.last_wet = [0.0; 2];
        self.activity = 0.0;
        let (input_gain, regen_gain) = placement_targets(self.placement);
        self.input_gain.settle(input_gain);
        self.regen_gain.settle(regen_gain);
        self.freeze_mix
            .settle(if self.controls.freeze { 1.0 } else { 0.0 });
        self.reverse_active = self.controls.reverse;
        self.reverse_target = self.controls.reverse;
        self.reverse_stage = ReverseStage::Stable;
        self.reverse_gain.settle(1.0);
        self.quiet_samples = 0;
        self.parked = true;
    }

    fn reset_shifters(&mut self) {
        for shifter in self
            .input_shift
            .iter_mut()
            .chain(self.regen_shift.iter_mut())
        {
            shifter.reset();
        }
    }

    #[inline]
    fn advance_reverse_transition(&mut self) -> f32 {
        let gain = self.reverse_gain.next();
        match self.reverse_stage {
            ReverseStage::FadeOut if self.reverse_gain.remaining == 0 => {
                if self.reverse_target != self.reverse_active {
                    // The tap has not changed, only its read direction. Keep the same recent input
                    // history and switch while the shifted branch is exactly silent; clearing four
                    // delay buffers here would add a needless callback-time spike.
                    self.reverse_active = self.reverse_target;
                }
                self.reverse_stage = ReverseStage::FadeIn;
                self.reverse_gain
                    .set_target(1.0, reverse_half_transition_samples(self.sample_rate));
            }
            ReverseStage::FadeIn if self.reverse_gain.remaining == 0 => {
                if self.reverse_target == self.reverse_active {
                    self.reverse_stage = ReverseStage::Stable;
                } else {
                    self.reverse_stage = ReverseStage::FadeOut;
                    self.reverse_gain
                        .set_target(0.0, reverse_half_transition_samples(self.sample_rate));
                }
            }
            _ => {}
        }
        gain
    }

    pub fn is_parked(&self) -> bool {
        self.parked
    }

    pub fn is_quiet(&self) -> bool {
        self.parked || (self.activity < QUIET_LEVEL && self.core.is_quiet())
    }

    pub fn is_sustaining(&self) -> bool {
        self.controls.freeze && !self.is_quiet()
    }

    /// Conservative in the useful direction: overestimation costs CPU; underestimation truncates.
    fn core_controls(&self, freeze: f32) -> reverb::CoreControls {
        reverb::CoreControls {
            size: self.controls.size,
            diffusion: self.controls.diffusion,
            regen: self.controls.regen,
            mod_rate_hz: self.controls.mod_rate_hz,
            mod_depth_s: self.controls.mod_depth_s,
            freeze,
        }
    }

    pub fn remaining_tail_seconds(&self) -> f32 {
        if self.is_sustaining() {
            return f32::INFINITY;
        }
        let core = ReverbCore::remaining_tail_seconds(
            self.controls.size,
            self.controls.diffusion,
            self.controls.regen,
        );
        let pass = core + self.controls.pre_delay_s + 0.11;
        let gain = self.controls.regen.clamp(0.001, 0.96);
        let outer_laps = (1.0e-6f32.ln() / gain.ln()).max(1.0);
        core + pass * outer_laps + QUIET_HOLD_S
    }

    #[inline]
    pub fn process(&mut self, dry_l: f32, dry_r: f32) -> (f32, f32) {
        let dry_l = flush(dry_l);
        let dry_r = flush(dry_r);
        let mix = self.controls.mix;
        if mix <= 0.0 {
            if !self.parked {
                self.reset();
            }
            return (dry_l, dry_r);
        }

        let input_peak = dry_l.abs().max(dry_r.abs());
        if self.parked {
            if input_peak <= QUIET_LEVEL {
                return (dry_l * (1.0 - mix), dry_r * (1.0 - mix));
            }
            self.parked = false;
        }

        let pre_samples = self.controls.pre_delay_s * self.sample_rate;
        // The lines keep following the input while the control is at zero. Otherwise automating
        // Pre-delay up from zero would expose unrelated stale samples left from its previous use.
        let delayed_l = self.pre[0].read(pre_samples.max(1.0));
        let delayed_r = self.pre[1].read(pre_samples.max(1.0));
        self.pre[0].write(dry_l);
        self.pre[1].write(dry_r);
        let pre_l = if pre_samples < 1.0 { dry_l } else { delayed_l };
        let pre_r = if pre_samples < 1.0 { dry_r } else { delayed_r };

        let input_gain = self.input_gain.next();
        let regen_gain = self.regen_gain.next();
        let freeze_mix = self.freeze_mix.next();
        let reverse_gain = self.advance_reverse_transition();

        let shift_level = 1.0
            + (MAX_SHIFT_LEVEL_COMPENSATION - 1.0)
                * (self.controls.shift_semitones.abs() / 12.0).min(1.0);
        // While frozen, each new input receives the selected shift once before it joins the held
        // field. The held return itself never passes through a shifter, so its harmony stays put.
        let one_pass_shift_gain = input_gain + freeze_mix * (1.0 - input_gain);
        let input_shifted = if one_pass_shift_gain > 0.0 {
            [
                bounded(
                    shift_level
                        * self.input_shift[0].process(
                            pre_l,
                            self.controls.shift_semitones,
                            self.reverse_active,
                        ),
                ),
                bounded(
                    shift_level
                        * self.input_shift[1].process(
                            pre_r,
                            self.controls.shift_semitones,
                            self.reverse_active,
                        ),
                ),
            ]
        } else {
            [0.0; 2]
        };

        let return_l = self.filters[0].process(
            self.last_wet[0],
            self.controls.low_cut_hz,
            self.controls.high_cut_hz,
            self.sample_rate,
        );
        let return_r = self.filters[1].process(
            self.last_wet[1],
            self.controls.low_cut_hz,
            self.controls.high_cut_hz,
            self.sample_rate,
        );
        // The ordinary Regen tap is after its gain and both filters, as the topology names it.
        // Freeze has a separate unshifted path below; keeping this branch ordinary during the
        // transition prevents the held state from continuing to climb behind the crossfade.
        let ordinary_tap_gain = self.controls.regen * OUTER_REGEN_SCALE;
        let tap_l = ordinary_tap_gain * return_l;
        let tap_r = ordinary_tap_gain * return_r;
        let shifted_return = if regen_gain > 0.0 {
            [
                bounded(
                    shift_level
                        * self.regen_shift[0].process(
                            tap_l,
                            self.controls.shift_semitones,
                            self.reverse_active,
                        ),
                ),
                bounded(
                    shift_level
                        * self.regen_shift[1].process(
                            tap_r,
                            self.controls.shift_semitones,
                            self.reverse_active,
                        ),
                ),
            ]
        } else {
            [0.0; 2]
        };

        // Direct and shifted feedback are a crossfade, and the two legs are not on the same gain.
        //
        // The shifted leg is deliberately allowed above unity — at full Shift the compensated
        // return reaches `1.0 * 0.15 * 10`. That is the ascent: a transposed copy has to outgrow
        // the untransposed tail underneath it or the tail simply buries it, and measurement said
        // a product below about 1.5 leaves the octaves *quieter* than the fundamental. It is
        // bounded by the shifted-return and summing-node saturators, not by being under one.
        //
        // The direct leg is not, because it duplicates the core (see `UNSHIFTED_OUTER_SHARE`).
        let ordinary_direct_gain = UNSHIFTED_OUTER_SHARE * ordinary_tap_gain;
        let direct_l = ordinary_direct_gain * return_l;
        let direct_r = ordinary_direct_gain * return_r;
        let shifted_fraction = (self.controls.shimmer * regen_gain * reverse_gain).clamp(0.0, 1.0);
        let branch_l = direct_l + shifted_fraction * (shifted_return[0] - direct_l);
        let branch_r = direct_r + shifted_fraction * (shifted_return[1] - direct_r);

        let branch_peak = branch_l.abs().max(branch_r.abs());
        // Freeze removes both ordinary outer-return legs. Sustain now belongs inside the core's
        // energy-preserving delay-bank topology; no output envelope is measured or multiplied back
        // into the audio, which avoids both progressive pitch/filter motion and AGC sidebands.
        let feedback_l = (1.0 - freeze_mix) * branch_l;
        let feedback_r = (1.0 - freeze_mix) * branch_r;
        let sum_l = bounded(
            pre_l
                + self.controls.shimmer * one_pass_shift_gain * reverse_gain * input_shifted[0]
                + feedback_l,
        );
        let sum_r = bounded(
            pre_r
                + self.controls.shimmer * one_pass_shift_gain * reverse_gain * input_shifted[1]
                + feedback_r,
        );
        let (wet_l, wet_r) = self
            .core
            .process(sum_l, sum_r, self.core_controls(freeze_mix));
        self.last_wet = [wet_l, wet_r];

        let wet_peak = self.last_wet[0].abs().max(self.last_wet[1].abs());
        let observed = input_peak.max(wet_peak).max(branch_peak);
        let follow = if observed > self.activity {
            self.activity_attack
        } else {
            self.activity_release
        };
        self.activity += (observed - self.activity) * follow;

        if input_peak <= QUIET_LEVEL && self.activity < QUIET_LEVEL && self.core.is_quiet() {
            self.quiet_samples += 1;
            if self.quiet_samples >= (QUIET_HOLD_S * self.sample_rate) as usize {
                self.reset();
            }
        } else {
            self.quiet_samples = 0;
        }

        (
            (1.0 - mix) * dry_l + mix * self.last_wet[0],
            (1.0 - mix) * dry_r + mix * self.last_wet[1],
        )
    }
}

fn placement_targets(placement: Placement) -> (f32, f32) {
    match placement {
        Placement::Input => (1.0, 0.0),
        Placement::Regen => (0.0, 1.0),
        Placement::Both => (0.5, 0.5),
    }
}

fn reverse_half_transition_samples(sample_rate: f32) -> u32 {
    (0.5 * TRANSITION_S * sample_rate).round().max(1.0) as u32
}

fn finite(value: f32, min: f32, max: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        min
    }
}

fn valid_rate(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(8_000.0, 384_000.0)
    } else {
        48_000.0
    }
}

#[derive(Clone, Copy, Default)]
struct LoopFilter {
    lp: f32,
    hp: f32,
    previous: f32,
}

impl LoopFilter {
    #[inline]
    fn process(&mut self, input: f32, low_cut: f32, high_cut: f32, sample_rate: f32) -> f32 {
        let lp_a = 1.0 - (-core::f32::consts::TAU * high_cut / sample_rate).exp();
        self.lp += lp_a * (input - self.lp);
        let hp_a = (-core::f32::consts::TAU * low_cut / sample_rate).exp();
        self.hp = hp_a * (self.hp + self.lp - self.previous);
        self.previous = self.lp;
        flush(self.hp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn impulse(engine: &mut Engine, controls: Controls, samples: usize) -> Vec<(f32, f32)> {
        engine.set_controls(controls);
        (0..samples)
            .map(|n| engine.process(if n == 0 { 0.8 } else { 0.0 }, 0.0))
            .collect()
    }

    pub fn tone_magnitude(
        samples: &[(f32, f32)],
        start: usize,
        count: usize,
        sample_rate: f32,
        frequency: f32,
    ) -> f64 {
        let mut energy = 0.0;
        for channel in 0..2 {
            let mut real = 0.0;
            let mut imaginary = 0.0;
            for (n, &(left, right)) in samples[start..start + count].iter().enumerate() {
                let sample = if channel == 0 { left } else { right } as f64;
                let phase =
                    core::f64::consts::TAU * frequency as f64 * n as f64 / sample_rate as f64;
                real += sample * phase.cos();
                imaginary -= sample * phase.sin();
            }
            energy += real * real + imaginary * imaginary;
        }
        energy.sqrt() / count as f64
    }

    #[test]
    fn silent_route_edits_settle_before_the_next_excitation() {
        let mut engine = Engine::new(48_000.0);
        let controls = Controls {
            placement: Placement::Input,
            reverse: true,
            freeze: true,
            ..Controls::default()
        };
        engine.set_controls(controls);
        assert!(engine.is_parked());
        assert_eq!(engine.input_gain.value, 1.0);
        assert_eq!(engine.regen_gain.value, 0.0);
        assert_eq!(engine.freeze_mix.value, 1.0);
        assert!(engine.reverse_active);
        assert_eq!(engine.reverse_stage, ReverseStage::Stable);
    }

    #[test]
    fn live_placement_and_freeze_transitions_reach_exact_endpoints() {
        let sample_rate = 48_000.0;
        let transition = (TRANSITION_S * sample_rate) as usize;
        let mut engine = Engine::new(sample_rate);
        let mut controls = Controls {
            mix: 1.0,
            ..Controls::default()
        };
        engine.set_controls(controls);
        engine.process(0.8, 0.8);

        controls.placement = Placement::Input;
        controls.freeze = true;
        engine.set_controls(controls);
        for _ in 0..transition {
            engine.process(0.0, 0.0);
        }
        assert_eq!(engine.input_gain.value, 1.0);
        assert_eq!(engine.regen_gain.value, 0.0);
        assert_eq!(engine.freeze_mix.value, 1.0);
        assert_eq!(engine.input_gain.remaining, 0);
        assert_eq!(engine.freeze_mix.remaining, 0);

        controls.freeze = false;
        engine.set_controls(controls);
        for _ in 0..transition {
            engine.process(0.0, 0.0);
        }
        assert_eq!(engine.freeze_mix.value, 0.0);
    }

    #[test]
    fn reverse_switches_direction_only_while_the_shifted_branch_is_silent() {
        let sample_rate = 48_000.0;
        let half = reverse_half_transition_samples(sample_rate) as usize;
        let mut engine = Engine::new(sample_rate);
        let mut controls = Controls {
            mix: 1.0,
            placement: Placement::Both,
            ..Controls::default()
        };
        engine.set_controls(controls);
        for n in 0..10_000 {
            engine.process((n as f32 * 0.03).sin() * 0.4, 0.0);
        }

        controls.reverse = true;
        engine.set_controls(controls);
        for _ in 0..half - 1 {
            engine.process(0.0, 0.0);
            assert!(!engine.reverse_active);
        }
        engine.process(0.0, 0.0);
        assert!(engine.reverse_active);
        assert_eq!(engine.reverse_gain.value, 0.0);
        assert_eq!(engine.reverse_stage, ReverseStage::FadeIn);

        for _ in 0..half {
            engine.process(0.0, 0.0);
        }
        assert_eq!(engine.reverse_gain.value, 1.0);
        assert_eq!(engine.reverse_stage, ReverseStage::Stable);
    }

    #[test]
    fn mix_zero_is_bit_exact_dry_and_parks() {
        let mut engine = Engine::new(48_000.0);
        let controls = Controls {
            mix: 0.0,
            ..Controls::default()
        };
        engine.set_controls(controls);
        for n in 0..1000 {
            let sample = (n as f32 * 0.17).sin();
            assert_eq!(engine.process(sample, -sample), (sample, -sample));
        }
        assert!(engine.is_parked());
    }

    #[test]
    fn silence_creates_nothing_and_reset_removes_every_tail() {
        let mut engine = Engine::new(48_000.0);
        engine.set_controls(Controls::default());
        for _ in 0..100_000 {
            assert_eq!(engine.process(0.0, 0.0), (0.0, 0.0));
        }
        impulse(&mut engine, Controls::default(), 10_000);
        engine.reset();
        assert_eq!(engine.process(0.0, 0.0), (0.0, 0.0));
        assert!(engine.is_parked());
    }

    #[test]
    fn an_impulse_blooms_after_the_pre_delay() {
        let mut engine = Engine::new(48_000.0);
        let controls = Controls {
            mix: 1.0,
            regen: 0.0,
            pre_delay_s: 0.02,
            ..Controls::default()
        };
        let out = impulse(&mut engine, controls, 30_000);
        let before = out[..800]
            .iter()
            .fold(0.0f32, |m, x| m.max(x.0.abs()).max(x.1.abs()));
        let after = out[1500..15_000]
            .iter()
            .fold(0.0f32, |m, x| m.max(x.0.abs()).max(x.1.abs()));
        assert_eq!(before, 0.0);
        assert!(after > 1.0e-5, "the reverb never bloomed");
    }

    #[test]
    fn freeze_holds_a_live_tail_and_accepts_later_input_without_creating_sound_from_silence() {
        let mut engine = Engine::new(48_000.0);
        let mut controls = Controls {
            mix: 1.0,
            regen: 0.7,
            ..Controls::default()
        };
        engine.set_controls(controls);
        for n in 0..12_000 {
            engine.process(
                if n < 2000 {
                    (n as f32 * 0.07).sin() * 0.5
                } else {
                    0.0
                },
                0.0,
            );
        }
        controls.freeze = true;
        engine.set_controls(controls);
        let mut early = 0.0f32;
        let mut late = 0.0f32;
        for n in 0..240_000 {
            let (l, r) = engine.process(0.0, 0.0);
            if (24_000..48_000).contains(&n) {
                early = early.max(l.abs()).max(r.abs());
            }
            if n > 216_000 {
                late = late.max(l.abs()).max(r.abs());
            }
        }
        assert!(engine.is_sustaining());
        assert!(
            late > early * 0.05,
            "freeze decayed away: {early} -> {late}"
        );

        let mut empty = Engine::new(48_000.0);
        empty.set_controls(Controls {
            freeze: true,
            ..controls
        });
        for _ in 0..100_000 {
            assert_eq!(empty.process(0.0, 0.0), (0.0, 0.0));
        }
        assert!(empty.is_parked());

        let mut accumulated_peak = 0.0f32;
        for n in 0..96_000 {
            let input = if n < 4_000 {
                (n as f32 * 0.09).sin() * 0.4
            } else {
                0.0
            };
            let (left, right) = empty.process(input, -input);
            if n > 72_000 {
                accumulated_peak = accumulated_peak.max(left.abs()).max(right.abs());
            }
        }
        assert!(
            accumulated_peak > 1.0e-5,
            "input armed after Freeze did not join the held field"
        );
        assert!(empty.is_sustaining());
    }

    #[test]
    fn freeze_preserves_a_dense_field_for_thirty_seconds_without_output_gain_control() {
        let sample_rate = 48_000.0;
        let mut engine = Engine::new(sample_rate);
        let mut controls = Controls {
            mix: 1.0,
            regen: 0.68,
            shimmer: 0.85,
            shift_semitones: 12.0,
            placement: Placement::Regen,
            size: 0.78,
            diffusion: 0.90,
            mod_rate_hz: 0.16,
            mod_depth_s: 0.0013,
            low_cut_hz: 180.0,
            high_cut_hz: 8_000.0,
            pre_delay_s: 0.030,
            ..Controls::default()
        };
        engine.set_controls(controls);
        for n in 0..sample_rate as usize * 2 {
            let input = if n < sample_rate as usize {
                (core::f32::consts::TAU * 220.0 * n as f32 / sample_rate).sin() * 0.12
            } else {
                0.0
            };
            engine.process(input, input);
        }
        controls.freeze = true;
        engine.set_controls(controls);

        let window = sample_rate as usize;
        let mut early_energy = 0.0f64;
        let mut late_energy = 0.0f64;
        let mut late_active = 0usize;
        for n in 0..sample_rate as usize * 30 {
            let (left, right) = engine.process(0.0, 0.0);
            let energy = f64::from(left * left + right * right);
            if (window * 5..window * 6).contains(&n) {
                early_energy += energy;
            }
            if (window * 29..window * 30).contains(&n) {
                late_energy += energy;
                late_active += usize::from(left.abs().max(right.abs()) > 1.0e-5);
            }
        }

        assert!(early_energy > 1.0e-9, "the measured frozen field was empty");
        assert!(
            (early_energy * 0.70..early_energy * 1.30).contains(&late_energy),
            "the frozen field changed level over the long hold: {early_energy} -> {late_energy}"
        );
        assert!(
            late_active > window * 99 / 100,
            "the long frozen field became sparse: {late_active}/{window} active"
        );
    }

    #[test]
    fn later_input_accumulates_into_an_existing_frozen_field() {
        let sample_rate = 48_000.0;
        let render = |add_second: bool| {
            let mut engine = Engine::new(sample_rate);
            engine.set_controls(Controls {
                mix: 1.0,
                regen: 0.50,
                shimmer: 0.0,
                freeze: true,
                size: 0.60,
                diffusion: 0.90,
                mod_rate_hz: 0.01,
                mod_depth_s: 0.0,
                low_cut_hz: 20.0,
                high_cut_hz: 20_000.0,
                pre_delay_s: 0.0,
                ..Controls::default()
            });
            (0..sample_rate as usize * 6)
                .map(|n| {
                    let first = if n < sample_rate as usize / 4 {
                        (core::f32::consts::TAU * 220.0 * n as f32 / sample_rate).sin() * 0.08
                    } else {
                        0.0
                    };
                    let second_start = sample_rate as usize * 2;
                    let second = if add_second
                        && (second_start..second_start + sample_rate as usize / 4).contains(&n)
                    {
                        (core::f32::consts::TAU * 997.0 * n as f32 / sample_rate).sin() * 0.08
                    } else {
                        0.0
                    };
                    engine.process(first + second, first + second)
                })
                .collect::<Vec<_>>()
        };
        let baseline = render(false);
        let accumulated = render(true);
        let count = sample_rate as usize / 2;
        let late_start = sample_rate as usize * 4;
        let retained_first = tone_magnitude(&accumulated, late_start, count, sample_rate, 220.0);
        let baseline_second = tone_magnitude(&baseline, late_start, count, sample_rate, 997.0);
        let accumulated_second =
            tone_magnitude(&accumulated, late_start, count, sample_rate, 997.0);

        assert!(
            retained_first > 1.0e-6,
            "the first frozen tone was displaced by later input: {retained_first}"
        );
        assert!(
            accumulated_second > baseline_second * 2.0 && accumulated_second > 1.0e-6,
            "later input did not accumulate: {baseline_second} -> {accumulated_second}"
        );
    }

    #[test]
    fn freeze_adds_one_shift_to_new_input_without_repeatedly_climbing_the_held_field() {
        let sample_rate = 48_000.0;
        let mut engine = Engine::new(sample_rate);
        engine.set_controls(Controls {
            mix: 1.0,
            regen: 0.50,
            shimmer: 1.0,
            shift_semitones: 12.0,
            placement: Placement::Regen,
            freeze: true,
            size: 0.60,
            diffusion: 0.90,
            mod_rate_hz: 0.01,
            mod_depth_s: 0.0,
            low_cut_hz: 20.0,
            high_cut_hz: 20_000.0,
            pre_delay_s: 0.0,
            ..Controls::default()
        });
        let output: Vec<_> = (0..sample_rate as usize * 6)
            .map(|n| {
                let input = if n < sample_rate as usize / 4 {
                    (core::f32::consts::TAU * 220.0 * n as f32 / sample_rate).sin() * 0.08
                } else {
                    0.0
                };
                engine.process(input, input)
            })
            .collect();
        let start = sample_rate as usize * 4;
        let count = sample_rate as usize / 2;
        let fundamental = tone_magnitude(&output, start, count, sample_rate, 220.0);
        let shifted_once = tone_magnitude(&output, start, count, sample_rate, 440.0);
        let shifted_again = tone_magnitude(&output, start, count, sample_rate, 880.0)
            + tone_magnitude(&output, start, count, sample_rate, 1_760.0);

        assert!(
            shifted_once > fundamental * 0.05,
            "new frozen input did not receive its one-pass shift: {fundamental}/{shifted_once}"
        );
        assert!(
            shifted_again < (fundamental + shifted_once) * 0.25,
            "the held field kept climbing: base={fundamental}, once={shifted_once}, upper={shifted_again}"
        );
    }

    #[test]
    fn a_tone_burst_climbs_through_successive_octaves_in_the_integrated_loop() {
        let sample_rate = 48_000.0;
        let mut engine = Engine::new(sample_rate);
        engine.set_controls(Controls {
            mix: 1.0,
            regen: 0.68,
            shimmer: 0.85,
            shift_semitones: 12.0,
            placement: Placement::Regen,
            size: 0.78,
            diffusion: 0.90,
            low_cut_hz: 180.0,
            high_cut_hz: 12_000.0,
            pre_delay_s: 0.0,
            ..Controls::default()
        });
        let output: Vec<_> = (0..sample_rate as usize * 4)
            .map(|n| {
                let input = if n < sample_rate as usize / 4 {
                    (core::f32::consts::TAU * 220.0 * n as f32 / sample_rate).sin() * 0.1
                } else {
                    0.0
                };
                engine.process(input, input)
            })
            .collect();
        let start = sample_rate as usize * 2;
        let count = sample_rate as usize / 2;
        let low = tone_magnitude(&output, start, count, sample_rate, 110.0)
            + tone_magnitude(&output, start, count, sample_rate, 220.0);
        let climbed = tone_magnitude(&output, start, count, sample_rate, 880.0)
            + tone_magnitude(&output, start, count, sample_rate, 1_760.0);
        assert!(
            climbed > low * 1.5,
            "the tail did not climb: low={low}, upper octaves={climbed}"
        );
    }

    #[test]
    fn the_longest_core_tail_is_not_truncated_to_the_old_one_minute_ceiling() {
        let mut engine = Engine::new(48_000.0);
        engine.set_controls(Controls {
            regen: 0.0,
            size: 1.0,
            diffusion: 1.0,
            ..Controls::default()
        });
        engine.process(0.8, 0.8);
        assert!(engine.remaining_tail_seconds() > 60.0);
    }

    #[test]
    fn adversarial_controls_are_finite_and_bounded_at_validator_rates() {
        for &sample_rate in &[8_000.0, 44_100.0, 48_000.0, 96_000.0, 192_000.0] {
            for &placement in &[Placement::Input, Placement::Regen, Placement::Both] {
                for &reverse in &[false, true] {
                    let mut engine = Engine::new(sample_rate);
                    let controls = Controls {
                        mix: 1.0,
                        regen: 1.0,
                        shimmer: 1.0,
                        shift_semitones: 24.0,
                        placement,
                        reverse,
                        size: 1.0,
                        diffusion: 1.0,
                        low_cut_hz: 20.0,
                        high_cut_hz: sample_rate,
                        ..Controls::default()
                    };
                    engine.set_controls(controls);
                    for n in 0..(sample_rate as usize * 3) {
                        let input = if n < 512 {
                            (n as f32 * 0.31).sin() * 20.0
                        } else {
                            0.0
                        };
                        let (l, r) = engine.process(input, -input);
                        assert!(l.is_finite() && r.is_finite());
                        assert!(
                            l.abs() <= 1.0 && r.abs() <= 1.0,
                            "{sample_rate}/{placement:?}/{reverse}: {l}/{r}"
                        );
                    }
                }
            }
        }
    }

    /// Below the unity crossing the ordinary loop must still resolve to a decay.
    ///
    /// This used to be asserted at `regen = 0.96`, on the old law where every setting decayed by
    /// construction. Regen is the loop gain now and its top sustains deliberately, so the decay
    /// claim moves well below the sustain threshold; that the sustaining end stays *bounded* is
    /// proved separately, in the core's own control-domain sweep.
    ///
    /// It has to stay clear of that threshold at *every* Size, not just this one. Loss is per lap
    /// and a large Size makes laps longer, so the same Regen rings longer in a big space than a
    /// small one — see the core's `internal_gain` note.
    #[test]
    fn high_ordinary_regen_below_unity_decays_instead_of_forming_a_late_saturated_attractor() {
        let sample_rate = 48_000.0;
        let window = sample_rate as usize / 10;
        let mut engine = Engine::new(sample_rate);
        engine.set_controls(Controls {
            mix: 1.0,
            regen: 0.50,
            shimmer: 0.0,
            placement: Placement::Regen,
            size: 1.0,
            diffusion: 1.0,
            low_cut_hz: 20.0,
            high_cut_hz: 20_000.0,
            pre_delay_s: 0.0,
            ..Controls::default()
        });

        let mut middle_energy = 0.0f64;
        let mut late_energy = 0.0f64;
        for n in 0..(sample_rate as usize * 20 + window) {
            let input = if n == 0 { 0.5 } else { 0.0 };
            let (left, right) = engine.process(input, input);
            let energy = f64::from(left * left + right * right);
            if (sample_rate as usize * 10..sample_rate as usize * 10 + window).contains(&n) {
                middle_energy += energy;
            }
            if (sample_rate as usize * 20..sample_rate as usize * 20 + window).contains(&n) {
                late_energy += energy;
            }
        }

        assert!(
            middle_energy > 1.0e-12,
            "the measured tail window was empty"
        );
        // The claim is that it decays rather than growing or plateauing, and the margin has to
        // suit the tails this now produces: a setting whose decay legitimately runs for tens of
        // seconds cannot be asked to fall fourfold inside a ten-second window, and the old 0.25
        // threshold was calibrated against the much shorter tails of the previous gain law.
        assert!(
            late_energy < middle_energy * 0.6,
            "the ordinary loop grew or plateaued after ten seconds: {middle_energy} -> {late_energy}"
        );
    }

    #[test]
    fn reset_restarts_every_random_and_transition_state() {
        let controls = Controls {
            mix: 1.0,
            placement: Placement::Both,
            reverse: true,
            pre_delay_s: 0.031,
            ..Controls::default()
        };
        let mut engine = Engine::new(48_000.0);
        let render = |engine: &mut Engine| {
            (0..80_000)
                .map(|n| {
                    let input = if n < 4000 {
                        (n as f32 * 0.071).sin() * 0.4
                    } else {
                        0.0
                    };
                    engine.process(input, -input)
                })
                .collect::<Vec<_>>()
        };
        engine.set_controls(controls);
        engine.reset();
        let first = render(&mut engine);
        engine.reset();
        let second = render(&mut engine);
        assert_eq!(first, second);
    }

    #[test]
    fn transitions_and_invalid_cutoff_order_remain_bounded_at_every_stress_rate() {
        for &sample_rate in &[
            8_000.0, 44_100.0, 48_000.0, 88_200.0, 96_000.0, 176_400.0, 192_000.0,
        ] {
            let mut engine = Engine::new(sample_rate);
            let mut controls = Controls {
                mix: 1.0,
                regen: 1.0,
                shimmer: 1.0,
                low_cut_hz: 4_000.0,
                high_cut_hz: 1_000.0,
                size: 1.0,
                diffusion: 1.0,
                ..Controls::default()
            };
            let count = sample_rate as usize * 2;
            for n in 0..count {
                if n % (sample_rate as usize / 5).max(1) == 0 {
                    let stage = (n / (sample_rate as usize / 5).max(1)) % 6;
                    controls.placement =
                        [Placement::Input, Placement::Regen, Placement::Both][stage % 3];
                    controls.reverse = stage & 1 != 0;
                    controls.freeze = stage == 3 || stage == 4;
                    controls.shift_semitones = [0.0, 12.0, 24.0][stage % 3];
                    engine.set_controls(controls);
                }
                let input = if n < (sample_rate as usize / 2) {
                    (n as f32 * 0.37).sin() * 20.0
                } else {
                    0.0
                };
                let (left, right) = engine.process(input, -input);
                assert!(
                    left.is_finite() && right.is_finite(),
                    "{sample_rate}: {left}/{right}"
                );
                assert!(
                    left.abs() <= 1.0 && right.abs() <= 1.0,
                    "{sample_rate}: {left}/{right}"
                );
            }
        }
    }

    #[test]
    fn host_block_partition_cannot_change_sample_recursion() {
        fn render(block: usize) -> Vec<f32> {
            let mut engine = Engine::new(48_000.0);
            engine.set_controls(Controls::default());
            let input: Vec<f32> = (0..50_000)
                .map(|n| {
                    if n < 2000 {
                        (n as f32 * 0.11).sin() * 0.4
                    } else {
                        0.0
                    }
                })
                .collect();
            let mut output = Vec::with_capacity(input.len());
            for chunk in input.chunks(block) {
                for &sample in chunk {
                    output.push(engine.process(sample, sample).0);
                }
            }
            output
        }
        assert_eq!(render(1), render(257));
    }

    /// A stimulus whose content is the same *sound* at any sample rate, so a rate sweep compares
    /// like with like. Built from a frequency in hertz rather than a per-sample increment: the
    /// older tests in this file use `(n as f32 * 0.03).sin()`, which at three rates is three
    /// different signals and would compare nothing.
    fn stimulus_at(rate: f32, seconds: f32) -> Vec<(f32, f32)> {
        let n = (rate * seconds) as usize;
        let mut out = Vec::with_capacity(n);
        let mut state = 12_345u32;
        for i in 0..n {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (state >> 9) as f32 / 8_388_608.0 - 1.0;
            let t = i as f32 / rate;
            out.push((
                0.5 * (t * 220.0 * core::f32::consts::TAU).sin() + 0.1 * noise,
                0.5 * (t * 233.0 * core::f32::consts::TAU).sin() + 0.1 * noise,
            ));
        }
        out
    }

    /// Excite the engine for a second at `rate`, then feed silence and report how many *seconds*
    /// it took to park, or `None` if it never did inside `limit_s`.
    fn seconds_until_parked(rate: f32, controls: Controls, limit_s: f32) -> Option<f32> {
        let mut engine = Engine::new(rate);
        engine.set_controls(controls);
        for &(l, r) in &stimulus_at(rate, 1.0) {
            let _ = engine.process(l, r);
        }
        for n in 0..(rate * limit_s) as usize {
            let _ = engine.process(0.0, 0.0);
            if engine.is_parked() {
                return Some(n as f32 / rate);
            }
        }
        None
    }

    #[test]
    fn the_park_decision_does_not_depend_on_the_sample_rate() {
        // Both level followers are times now, and this is the test that has teeth. Asserting a
        // coefficient proves nothing; what has to hold at every accepted rate is the *decision* —
        // so both directions are asserted at each, because a follower that simply never falls
        // would satisfy the first half alone.
        //
        // Verified red against the old fixed coefficients: a decayed field parked after 11.24 s at
        // 8 kHz against 6.59 s at 48 kHz and 6.54 s at 192 kHz, the difference being the
        // follower's own release. It is 6.586 s at all three now, within a millisecond.
        //
        // The budget is deliberately well above that, because what it must not do is drift into
        // asserting the tail's own length: parking must stay conservative, and a follower that
        // reported quiet *sooner* than the signal would be the regression this margin leaves room
        // to catch elsewhere.
        let decaying = Controls {
            mix: 1.0,
            regen: 0.2,
            shimmer: 0.0,
            size: 0.3,
            pre_delay_s: 0.0,
            ..Controls::default()
        };
        // Freeze holds the field inside the core, so this one must never park at any rate.
        let live = Controls {
            freeze: true,
            ..decaying
        };
        for &rate in &[8_000.0f32, 48_000.0, 192_000.0] {
            let parked = seconds_until_parked(rate, decaying, 20.0);
            assert!(
                parked.is_some_and(|seconds| seconds < 9.0),
                "a decayed field took {parked:?} s to park at {rate} Hz, over a 9 s budget"
            );
            assert!(
                seconds_until_parked(rate, live, 12.0).is_none(),
                "a frozen field was parked while it was still sounding at {rate} Hz"
            );
        }
    }
}
