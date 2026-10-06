//! A bounded modulated-delay-bank reverb core.
//!
//! The late field is an energy-preserving Walsh-Hadamard feedback delay network, fed by cascaded
//! Schroeder allpass diffusors. Every stored line and public output is explicitly bounded.
//!
//! The loop gain is **allowed to pass one**, at the top of Regen, which is what makes the network
//! self-oscillate rather than merely decay slowly. Boundedness no longer comes from keeping the
//! gain below unity; it comes from the rational saturator on every line write, which turns growth
//! into a limit cycle at a finite amplitude, and from the DC blocker inside the feedback path,
//! which stops that limit cycle from settling at zero frequency.

use crate::delay::{Delay, bounded, flush, follower_coefficient};

const LINES: usize = 8;
const DIFFUSER_STAGES: usize = 16;
// Chosen, pairwise awkward ratios. These are our constants, not values read from another product.
const RATIOS: [f32; LINES] = [0.337, 0.421, 0.503, 0.593, 0.677, 0.761, 0.887, 1.0];
// Unitless per-line stagger for the modulation rate, normalised so the mean is one. Mod Rate is a
// frequency in hertz and multiplies these; the spread is what stops eight lines from breathing
// together, and it is a ratio set rather than a frequency set so the control can scale all eight.
const MOD_STAGGER: [f32; LINES] = [0.448, 0.561, 0.675, 0.826, 1.028, 1.243, 1.470, 1.747];
const PHASES: [f32; LINES] = [0.03, 0.19, 0.37, 0.53, 0.68, 0.79, 0.88, 0.96];
const DIFFUSER_L_S: [f32; DIFFUSER_STAGES] = [
    0.0041, 0.0053, 0.0067, 0.0083, 0.0101, 0.0127, 0.0149, 0.0173, 0.0197, 0.0229, 0.0263, 0.0301,
    0.0337, 0.0371, 0.0413, 0.0461,
];
const DIFFUSER_R_S: [f32; DIFFUSER_STAGES] = [
    0.0047, 0.0059, 0.0073, 0.0089, 0.0109, 0.0131, 0.0157, 0.0181, 0.0209, 0.0239, 0.0271, 0.0311,
    0.0349, 0.0383, 0.0421, 0.0451,
];
const INJECTION_SIGNS: [f32; LINES] = [1.0, -1.0, -1.0, 1.0, -1.0, 1.0, 1.0, -1.0];
/// Where the early cluster is taken from the diffusor cascade, and at what weight.
///
/// The cascade is the only path to the output, and it is long — 337 ms of allpass unscaled, 431 ms
/// at the default Size — so nothing audible used to arrive for the first hundred milliseconds and
/// the effect read as though it had a pre-delay it did not have. The allpass feed-through that
/// could have arrived sooner is attenuated by `(0.75 * Diffusion)^16`, about -57 dB at the default
/// Diffusion, so there was no early energy by construction.
///
/// Each tap takes its stage's *delayed* read, never the stage output — see [`Allpass::process`].
/// These are stage indices, so the tap times ride the same `diffuser_scale` as everything else and
/// a small space keeps a proportionally quick onset. At the default Size the three land near 19,
/// 29 and 43 ms. Three rather than one: a single tap is one discrete echo, which is the slap the
/// diffusors exist to avoid, and the left and right trains differ so the cluster is already
/// decorrelated. Weights fall so the cluster reads as arrivals rather than as a repeat.
const EARLY_TAPS: [(usize, f32); 3] = [(6, 0.50), (9, 0.35), (12, 0.22)];
/// How loud that cluster is against the late field. Chosen by measurement: enough to give the
/// effect an attack, not enough to make the early cluster the loudest thing in the impulse.
const EARLY_LEVEL: f32 = 0.30;
const MAX_DELAY_S: f32 = 0.110;
const MAX_DIFFUSER_DELAY_S: f32 = 0.092;
const DIFFUSER_STATE_BOUND: f32 = 4.0;
const QUIET: f32 = 1.0e-7;
// How long the level follower takes to rise to a louder field and to fall away from a quieter one.
//
// **Times rather than sample counts, which is the correction.** These were fixed per-sample
// coefficients of 0.05 and 0.0005 — twenty and two thousand samples, which is 0.4 ms and 42 ms at
// 48 kHz but 2.5 ms and 250 ms at the 8 kHz this crate accepts, and every other time in this file
// is converted from `self.sample_rate`. The follower is a peak follower, fast up and slow down, so
// it cannot report quiet before the signal is and the failure was never a truncated tail: at a low
// rate its own release was most of the time the engine took to notice it had gone quiet, which
// costs CPU and parks late. Measured on the whole engine before this change, a decayed field parked
// after 11.24 s at 8 kHz against 6.59 s at 48 kHz and 6.54 s at 192 kHz, where the difference is
// entirely this lag. The values are what the coefficients were at the rate they were written for,
// so the behaviour there is unchanged.
const LEVEL_ATTACK_S: f32 = 0.000_4;
const LEVEL_RELEASE_S: f32 = 0.042;
/// Corner of the DC blocker inside the FDN feedback path.
///
/// Chosen very low on purpose. A first-order high-pass rejects DC completely whatever its corner,
/// so the corner buys nothing for stability and costs the tail directly: it is applied on every
/// lap, and a tail that has been through a swept interpolating read many times has most of its
/// remaining energy down low, exactly where this filter bites. At 12 Hz it took 7% per lap at
/// 30 Hz and collapsed a thirty-second decay into about five; at 3 Hz the same measurement is
/// under 1%.
const DC_BLOCK_HZ: f32 = 1.0;
/// What the tail declaration reports once the loop sustains. An hour is not a decay estimate; it
/// is the honest statement that there is no decay to estimate, kept finite so a host still gets a
/// number. The contract permits overestimating and forbids underestimating.
pub const SUSTAINING_TAIL_S: f32 = 3_600.0;
const MATRIX_SCALE: f32 = 0.353_553_38; // 1 / sqrt(8), for an energy-preserving transform.

#[derive(Clone)]
struct Allpass {
    delay: Delay,
}

impl Allpass {
    fn new(sample_rate: f32) -> Self {
        Self {
            delay: Delay::new((MAX_DIFFUSER_DELAY_S * sample_rate).ceil() as usize + 4),
        }
    }

    fn reset(&mut self) {
        self.delay.clear();
    }

    /// Returns `(output, delayed)`.
    ///
    /// `delayed` is the delay line's own read, before the feed-through term is subtracted. It is
    /// the only part of an allpass that is actually late, which is what an early tap has to use:
    /// `output` carries `-gain * input` instantaneously, so tapping it puts a sample-zero spike on
    /// the output rather than an early reflection.
    #[inline]
    fn process(&mut self, input: f32, delay_samples: f32, gain: f32) -> (f32, f32) {
        // Schroeder allpass section. It stays linear: repeatedly saturating a long cascade erases
        // the very energy redistribution and Gaussian build the topology exists to create. The
        // core clamps its public input, caps diffuser state far outside the ordinary signal range,
        // and bounds every FDN write and public output instead.
        let delayed = self.delay.read(delay_samples.max(2.0));
        let output = flush(delayed - gain * input);
        self.delay
            .write(flush(input + gain * output).clamp(-DIFFUSER_STATE_BOUND, DIFFUSER_STATE_BOUND));
        (output, delayed)
    }
}

/// The one-pole high-pass in the FDN's own feedback path.
///
/// This is the loop's stability mechanism, and it is why the loop gain is allowed to reach and
/// pass unity at all: an undamped unity loop runs away at zero frequency first, because DC is the
/// one component every lap adds in phase. The saturator on each line write bounds the amplitude;
/// this decides that what survives is a ring rather than a drift into the rails.
///
/// **Neither tone control belongs here, and the corner is deliberately far below either of them.**
/// Both were tried in the loop and measured out. A one-pole low-pass at the 8 kHz High Cut default
/// costs more per lap than any musically sensible loop gain returns, so the network could not
/// sustain at any Regen. Running this high-pass at the 180 Hz Low Cut default was subtler and worse
/// for ordinary use: interpolating a swept tap already eats the top of the band every lap, so
/// cutting the bottom too squeezed the tail from both ends and collapsed decays that used to run
/// for tens of seconds. Low Cut and High Cut stay on the outer return, where they shape the wet
/// signal; this stays a plain DC blocker, which is all stability actually asks for.
#[derive(Clone, Default)]
struct LineFilter {
    hp: f32,
    previous: f32,
}

impl LineFilter {
    fn reset(&mut self) {
        self.hp = 0.0;
        self.previous = 0.0;
    }

    #[inline]
    fn process(&mut self, input: f32, hp_coefficient: f32) -> f32 {
        self.hp = hp_coefficient * (self.hp + input - self.previous);
        self.previous = input;
        flush(self.hp)
    }
}

/// Everything the core reads per sample.
///
/// This became a struct when Regen turned into the loop gain and modulation turned into two
/// controls: the argument list had grown past the point where a caller could get the order right.
#[derive(Debug, Clone, Copy)]
pub struct CoreControls {
    pub size: f32,
    pub diffusion: f32,
    pub regen: f32,
    pub mod_rate_hz: f32,
    pub mod_depth_s: f32,
    /// Crossfade from the ordinary lossy/modulated loop to the energy-preserving held loop.
    pub freeze: f32,
}

#[derive(Clone)]
pub struct ReverbCore {
    lines: [Delay; LINES],
    diffusers: [[Allpass; DIFFUSER_STAGES]; 2],
    filters: [LineFilter; LINES],
    phases: [f32; LINES],
    sample_rate: f32,
    level: f32,
    level_attack: f32,
    level_release: f32,
}

impl ReverbCore {
    pub fn new(sample_rate: f32) -> Self {
        let sample_rate = valid_rate(sample_rate);
        let max = (MAX_DELAY_S * sample_rate).ceil() as usize + 4;
        Self {
            lines: std::array::from_fn(|_| Delay::new(max)),
            diffusers: std::array::from_fn(|_| std::array::from_fn(|_| Allpass::new(sample_rate))),
            filters: Default::default(),
            phases: PHASES,
            sample_rate,
            level: 0.0,
            level_attack: follower_coefficient(LEVEL_ATTACK_S, sample_rate),
            level_release: follower_coefficient(LEVEL_RELEASE_S, sample_rate),
        }
    }

    /// The maximum amplitude this core can contribute for a bounded input.
    ///
    /// Delay writes and the final four-line output sums are saturated to `[-1, 1]`. The declared
    /// instantaneous amplitude gain is therefore one.
    pub const fn worst_case_gain() -> f32 {
        1.0
    }

    pub fn reset(&mut self) {
        for line in &mut self.lines {
            line.clear();
        }
        for diffuser in self.diffusers.iter_mut().flatten() {
            diffuser.reset();
        }
        for filter in &mut self.filters {
            filter.reset();
        }
        self.phases = PHASES;
        self.level = 0.0;
    }

    pub fn is_quiet(&self) -> bool {
        self.level < QUIET
    }

    pub fn level(&self) -> f32 {
        self.level
    }

    /// A conservative tail estimate: it may overestimate and never intentionally underestimates.
    ///
    /// At or above unity loop gain there is no decay to estimate — the network sustains — so the
    /// declaration saturates at [`SUSTAINING_TAIL_S`] rather than returning a time that would be
    /// an underestimate by construction.
    pub fn remaining_tail_seconds(size: f32, diffusion: f32, regen: f32) -> f32 {
        let size = finite_unit(size);
        let diffusion = finite_unit(diffusion);
        let longest = delay_seconds(size, 1.0) + 0.004;
        let gain = internal_gain(finite_unit(regen));
        if gain >= 1.0 {
            return SUSTAINING_TAIL_S;
        }
        // Time to -120 dB, plus two full FDN laps and a deliberately conservative sum of every
        // recursive diffuser stage's own -120 dB span.
        let laps = (1.0e-6f32.ln() / gain.max(0.001).ln()).max(1.0);
        let diffuser_gain = (0.75 * diffusion).max(0.001);
        let diffuser_laps = (1.0e-6f32.ln() / diffuser_gain.ln()).max(1.0);
        let diffuser_tail =
            MAX_DIFFUSER_DELAY_S * diffuser_scale(size) * DIFFUSER_STAGES as f32 * diffuser_laps;
        (longest * (laps + 2.0) + diffuser_tail).min(SUSTAINING_TAIL_S)
    }

    #[inline]
    pub fn process(&mut self, left: f32, right: f32, controls: CoreControls) -> (f32, f32) {
        let left = flush(left).clamp(-1.0, 1.0);
        let right = flush(right).clamp(-1.0, 1.0);
        let size = finite_unit(controls.size);
        let diffusion = finite_unit(controls.diffusion);
        let regen = finite_unit(controls.regen);
        let freeze = finite_unit(controls.freeze);
        let diffuser_scale = diffuser_scale(size);
        let diffuser_gain = 0.75 * diffusion;
        let (left_diffused, left_early) = diffuse_input(
            left,
            &mut self.diffusers[0],
            &DIFFUSER_L_S,
            diffuser_scale,
            diffuser_gain,
            self.sample_rate,
        );
        let (right_diffused, right_early) = diffuse_input(
            right,
            &mut self.diffusers[1],
            &DIFFUSER_R_S,
            diffuser_scale,
            diffuser_gain,
            self.sample_rate,
        );
        // Mod Rate and Mod Depth are the player's now, rather than a fixed rate and a depth taken
        // from Diffusion. The depth is a time and the rate a frequency, so both survive a sample
        // rate change; the per-line stagger keeps its shape as the rate scales.
        let mod_rate_hz = controls.mod_rate_hz.clamp(0.0, 20.0);
        let mod_depth_samples = controls.mod_depth_s.clamp(0.0, 0.02) * self.sample_rate;
        let mut reads = [0.0f32; LINES];
        for i in 0..LINES {
            self.phases[i] =
                (self.phases[i] + mod_rate_hz * MOD_STAGGER[i] / self.sample_rate).fract();
            let modulation = (core::f32::consts::TAU * self.phases[i]).sin() * mod_depth_samples;
            let base_delay = delay_seconds(size, RATIOS[i]) * self.sample_rate;
            let ordinary = self.lines[i].read((base_delay + modulation).max(2.0));
            // Freeze uses an integer delay so reading and rewriting a line loses no energy to
            // interpolation. The delay modulation is deliberately parked at the held endpoint:
            // preserving the field is the contract, while a moving fractional tap necessarily
            // duplicates/skips energy unless it has a substantially different interpolation
            // topology. The 25 ms outer transition moves between the two read positions.
            let held = self.lines[i].read(base_delay.round().max(2.0));
            reads[i] = ordinary + freeze * (held - ordinary);
        }

        let mut diffuse = reads;
        hadamard(&mut diffuse);
        for value in &mut diffuse {
            *value *= MATRIX_SCALE;
        }

        let feedback = internal_gain(regen);
        let hp_coefficient = (-core::f32::consts::TAU * DC_BLOCK_HZ / self.sample_rate).exp();
        let mut peak = left
            .abs()
            .max(right.abs())
            .max(left_diffused.abs())
            .max(right_diffused.abs())
            .max(left_early.abs())
            .max(right_early.abs());
        for i in 0..LINES {
            // Always the full Walsh-Hadamard rotation, never a blend back toward the unmixed
            // line. The blend used to be `reads + diffusion * (diffuse - reads)`, and because that
            // matrix has eigenvalues `1` and `1 - 2 * diffusion`, half the network's modes were
            // attenuated by an amount Diffusion set — so Diffusion was a second decay control, and
            // a high setting was the only way to reach a long tail. Measured: at Diffusion 0.99 a
            // preset would not decay inside thirty seconds at any Regen down to 0.56. The rotation
            // is orthogonal on its own, so the loop gain is now `feedback` for every mode, and
            // Diffusion is free to mean density.
            let mixed = diffuse[i];
            // Decorrelated left/right diffuser trains and an orthogonal sign pattern excite all
            // matrix modes instead of collapsing a mono impulse into one Hadamard component.
            let source = if i & 1 == 0 {
                left_diffused
            } else {
                right_diffused
            };
            // Input remains present at high diffusion. Scaling it by `1 - feedback` made the dry
            // excitation and, crucially, every pitch-shifted outer return vanish as the desired
            // decay grew, leaving only sparse unshifted line recurrences.
            let input = 0.90 * INJECTION_SIGNS[i] * source;
            // The filters sit inside the loop, on the recirculating term only, so the input is not
            // filtered on the way in and every lap is. That is what lets `feedback` pass unity.
            let recirculated = self.filters[i].process(mixed, hp_coefficient);
            let ordinary_written = bounded(input + feedback * recirculated);
            // At full Freeze, integer reads plus the orthogonal matrix preserve the delay-bank
            // energy without observing or multiplying the output waveform. Clamp only on actual
            // accumulation; unlike the rational saturator it is exactly linear below the bound,
            // so a quiet held field neither decays nor grows into a limit cycle.
            let held_written = flush(input + mixed).clamp(-1.0, 1.0);
            let written = ordinary_written + freeze * (held_written - ordinary_written);
            self.lines[i].write(written);
            peak = peak.max(written.abs()).max(reads[i].abs());
        }
        let follow = if peak > self.level {
            self.level_attack
        } else {
            self.level_release
        };
        self.level += (peak - self.level) * follow;
        if self.level < f32::MIN_POSITIVE {
            self.level = 0.0;
        }

        // The early cluster joins the late field inside the same saturator, so the declared
        // output bound is unchanged and `worst_case_gain()` still holds.
        let out_l =
            bounded((reads[0] + reads[2] + reads[5] - reads[7]) * 0.625 + EARLY_LEVEL * left_early);
        let out_r = bounded(
            (reads[1] + reads[3] - reads[4] + reads[6]) * 0.625 + EARLY_LEVEL * right_early,
        );
        (out_l, out_r)
    }
}

fn valid_rate(rate: f32) -> f32 {
    if rate.is_finite() {
        rate.clamp(8_000.0, 384_000.0)
    } else {
        48_000.0
    }
}

fn finite_unit(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Runs the cascade, returning `(late, early)`: what reaches the FDN, and the early cluster.
fn diffuse_input(
    mut input: f32,
    diffusers: &mut [Allpass; DIFFUSER_STAGES],
    times: &[f32; DIFFUSER_STAGES],
    scale: f32,
    gain: f32,
    sample_rate: f32,
) -> (f32, f32) {
    let mut early = 0.0;
    for (index, (stage, &seconds)) in diffusers.iter_mut().zip(times).enumerate() {
        let (output, delayed) = stage.process(input, seconds * scale * sample_rate, gain);
        input = output;
        if let Some(&(_, weight)) = EARLY_TAPS.iter().find(|(stage, _)| *stage == index) {
            early += weight * delayed;
        }
    }
    (input, flush(early))
}

fn delay_seconds(size: f32, ratio: f32) -> f32 {
    // A compact late field: short, incommensurate recurrences become dense before any one return
    // can read as a slap. Size still spans discrete small-space returns to a broad wash.
    (0.012 + 0.078 * size.clamp(0.0, 1.0)) * ratio
}

/// The scale on the diffusor times.
///
/// Chosen, and deliberately never near zero. The previous `0.25 + 1.65 * size` took the diffusors
/// to a quarter of their length at Size 0, which is what made a small setting read as *undiffused*
/// rather than as a small diffuse room: the late field shrank and the thing that was supposed to
/// smear it shrank with it, so early energy arrived as countable taps. Size now moves the diffusors
/// only enough to keep a small space from sounding like a large one's front end.
fn diffuser_scale(size: f32) -> f32 {
    0.85 + 0.55 * size.clamp(0.0, 1.0)
}

/// The FDN's loop gain — the only control that adds gain to the system.
///
/// Regen owns decay length. Diffusion used to set this, which is why decay could not be set
/// independently of density and why the longest tail available was the one Diffusion happened to
/// reach. The curve is deliberately generous — the bottom is already a real room and the middle is
/// where the many-second tails live. Measured at Size 0.70: 2.9 s at the bottom, 15.7 s at the
/// default 0.68, 34.8 s at 0.80, and self-oscillation from about 0.90.
///
/// **The oscillation threshold moves with Size, and that is not a defect.** Loss is per lap, so a
/// large space with fewer, longer laps reaches unity at a lower Regen — about 0.80 at Size 0.98
/// against about 0.90 at Size 0.20. A bigger room ringing longer for the same absorption is what
/// a room does. It does mean a factory preset cannot be given a high Regen and a high Size
/// without being rendered first; `shimmer_preset_audit` fails the build if one of them drones.
///
/// Above unity the network grows. It cannot run away: the rational saturator on every line write
/// makes `bounded(g·x) = x` a stable equilibrium at `x = (g − 1) / g`, so the growth settles into a
/// bounded limit cycle. Below unity that same saturator is why no sustain is possible at all —
/// `bounded(g·x) < x` for every `x > 0` when `g ≤ 1` — which is the whole reason this law has to
/// pass unity rather than approach it.
fn internal_gain(regen: f32) -> f32 {
    let regen = regen.clamp(0.0, 1.0);
    // The last term carries real headroom rather than just crossing one, because a sweeping
    // fractional read is itself lossy: interpolating a moving tap eats high frequencies every lap,
    // and at the default Mod Depth a gain of 1.12 measured as barely-sustaining. Self-oscillation
    // at the top of the control should not be a close-run thing that Mod Depth can switch off.
    0.80 + 0.183 * regen.powf(0.4) + 0.067 * regen.powi(16)
}

fn hadamard(values: &mut [f32; LINES]) {
    let mut width = 1;
    while width < LINES {
        let mut start = 0;
        while start < LINES {
            for i in 0..width {
                let a = values[start + i];
                let b = values[start + i + width];
                values[start + i] = a + b;
                values[start + i + width] = a - b;
            }
            start += width * 2;
        }
        width *= 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A long-decaying but still sub-unity setting, which is what most of these tests want.
    fn controls(size: f32, diffusion: f32) -> CoreControls {
        CoreControls {
            size,
            diffusion,
            regen: 0.8,
            mod_rate_hz: 0.16,
            mod_depth_s: 0.0013,
            freeze: 0.0,
        }
    }

    fn with_regen(size: f32, diffusion: f32, regen: f32) -> CoreControls {
        CoreControls {
            regen,
            ..controls(size, diffusion)
        }
    }

    #[test]
    fn reset_and_silence_are_exact() {
        let mut core = ReverbCore::new(48_000.0);
        for _ in 0..20_000 {
            assert_eq!(core.process(0.0, 0.0, controls(0.5, 0.8)), (0.0, 0.0));
        }
        core.process(1.0, -1.0, controls(1.0, 1.0));
        core.reset();
        assert_eq!(core.process(0.0, 0.0, controls(1.0, 1.0)), (0.0, 0.0));
        assert!(core.is_quiet());
    }

    #[test]
    fn one_bad_sample_or_control_cannot_poison_recursive_state() {
        let mut core = ReverbCore::new(48_000.0);
        let poison = CoreControls {
            size: f32::NAN,
            diffusion: f32::INFINITY,
            regen: f32::NAN,
            mod_rate_hz: f32::INFINITY,
            mod_depth_s: f32::NAN,
            freeze: f32::INFINITY,
        };
        for &(left, right, control) in &[
            (f32::INFINITY, f32::NEG_INFINITY, controls(0.5, 0.8)),
            (f32::NAN, f32::NAN, poison),
        ] {
            let (out_l, out_r) = core.process(left, right, control);
            assert!(out_l.is_finite() && out_r.is_finite());
        }
        for n in 0..100_000 {
            let input = if n == 0 { 0.8 } else { 0.0 };
            let (out_l, out_r) = core.process(input, -input, controls(0.7, 0.9));
            assert!(out_l.is_finite() && out_r.is_finite());
        }
        assert!(core.level().is_finite());
        assert!(ReverbCore::remaining_tail_seconds(f32::NAN, f32::INFINITY, f32::NAN).is_finite());
    }

    #[test]
    fn regen_owns_decay_and_the_top_of_it_sustains() {
        // Below unity the loop must decay; at the top of the control it must not. This is the
        // property the saturator makes exact: `bounded(g·x) < x` for every `x > 0` when `g <= 1`.
        // Deliberately *not* asserted against one: the coefficient is not the loop gain. The
        // network loses several percent a lap on its own, so the coefficient passes one well
        // before the round trip does, and the only honest claim is the measured one below.
        // Monotonic, so the control never doubles back on itself.
        let mut previous = 0.0;
        for step in 0..=100 {
            let gain = internal_gain(step as f32 / 100.0);
            assert!(gain > previous, "internal_gain is not monotonic at {step}");
            previous = gain;
        }

        let sample_rate = 48_000.0;
        let measure = |regen: f32| {
            let mut core = ReverbCore::new(sample_rate);
            let mut late = 0.0f32;
            for n in 0..(20.0 * sample_rate) as usize {
                let input = if n < 64 { 0.5 } else { 0.0 };
                let (l, r) = core.process(input, -input, with_regen(0.7, 0.9, regen));
                if n > (15.0 * sample_rate) as usize {
                    late = late.max(l.abs().max(r.abs()));
                }
            }
            late
        };
        let short = measure(0.2);
        let sustained = measure(1.0);
        assert!(
            short < 1.0e-4,
            "a low Regen still rang at {short} fifteen seconds in"
        );
        assert!(
            sustained > 5.0e-3,
            "the top of Regen decayed to {sustained} instead of sustaining"
        );
        assert!(
            sustained > short * 100.0,
            "the top of Regen ({sustained}) is not meaningfully longer than a low one ({short})"
        );
    }

    #[test]
    fn a_small_size_stays_diffused() {
        // The complaint this law was changed for: Size at the bottom used to shrink the diffusors
        // with the late field, so a small room arrived as countable taps. The diffusors keep most
        // of their length across the whole of Size.
        assert!(diffuser_scale(0.0) > 0.8, "small Size collapses diffusion");
        assert!(diffuser_scale(1.0) / diffuser_scale(0.0) < 2.0);

        // The claim is comparative, so measure it comparatively: a small Size must not arrive
        // markedly less dense than a large one. An absolute density threshold would only be
        // measuring how long the late field takes to fill, which is what Size legitimately sets.
        let sample_rate = 48_000.0;
        let window = (0.05 * sample_rate) as usize;
        let density = |size: f32| {
            let mut core = ReverbCore::new(sample_rate);
            let mut active = 0usize;
            for n in 0..window {
                let input = if n == 0 { 0.5 } else { 0.0 };
                let (l, r) = core.process(input, input, with_regen(size, 0.9, 0.8));
                active += usize::from(l.abs().max(r.abs()) > 1.0e-6);
            }
            active as f32 / window as f32
        };
        let small = density(0.0);
        let large = density(1.0);
        assert!(
            small > 0.75,
            "Size 0 arrived only {small:.2} dense, i.e. as countable taps"
        );
        assert!(
            small > large * 0.85,
            "Size 0 ({small:.2}) is markedly less diffuse than Size 1 ({large:.2})"
        );
    }

    #[test]
    fn high_diffusion_builds_a_dense_many_second_bloom_instead_of_a_slap() {
        let sample_rate = 48_000.0;
        let window = (0.1 * sample_rate) as usize;
        let mut core = ReverbCore::new(sample_rate);
        let mut windows = Vec::new();
        for w in 0..60 {
            let mut energy = 0.0f64;
            let mut active = 0usize;
            for i in 0..window {
                let n = w * window + i;
                let input = if n == 0 { 0.33 } else { 0.0 };
                // Regen sets decay now, so a test about a many-second tail has to ask for one.
                let (left, right) = core.process(input, input, with_regen(0.70, 0.90, 0.85));
                let peak = left.abs().max(right.abs());
                active += usize::from(peak > 1.0e-6);
                energy += f64::from(left * left + right * right);
            }
            windows.push(((energy / (2 * window) as f64).sqrt(), active));
        }
        let (peak_window, &(peak_rms, peak_active)) = windows[..10]
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.0.total_cmp(&b.0))
            .expect("ten measured windows");
        assert!(
            // A slap peaks in the first window or two; the point is that this does not. The upper
            // bound moved out with the decay: a tail that now runs for tens of seconds keeps
            // accumulating for longer before it turns over.
            (2..=9).contains(&peak_window),
            "the envelope peaked at {} ms rather than blooming",
            peak_window * 100
        );
        assert!(
            peak_active > window * 95 / 100,
            "the peak window was only {peak_active}/{window} dense"
        );
        assert!(
            windows[50].0 > peak_rms * 0.005,
            "the dense tail collapsed before five seconds: {} -> {}",
            peak_rms,
            windows[50].0
        );
    }

    #[test]
    fn the_whole_control_domain_is_bounded() {
        // Including the self-oscillating corner: the saturator, not a sub-unity gain, is what
        // makes this hold, so Regen 1.0 has to be in the sweep for the claim to mean anything.
        for &size in &[0.0, 0.5, 1.0] {
            for &diffusion in &[0.0, 0.5, 1.0] {
                for &regen in &[0.0, 0.5, 1.0] {
                    for &freeze in &[0.0, 1.0] {
                        let mut core = ReverbCore::new(48_000.0);
                        let mut peak = 0.0f32;
                        let mut controls = with_regen(size, diffusion, regen);
                        controls.freeze = freeze;
                        for n in 0..200_000 {
                            let input = if n < 1024 {
                                (n as f32 * 0.37).sin() * 8.0
                            } else {
                                0.0
                            };
                            let (l, r) = core.process(input, -input, controls);
                            peak = peak.max(l.abs()).max(r.abs());
                            assert!(l.is_finite() && r.is_finite());
                        }
                        assert!(
                            peak <= 1.0,
                            "core reached {peak} at {size}/{diffusion}/{regen}/{freeze}"
                        );
                    }
                }
            }
        }
    }
}
