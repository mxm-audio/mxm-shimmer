//! Swept-delay pitch shifting on the rotating-head/Doppler principle.
//!
//! The read offset walks against the write head so its playback rate differs from one. A Hann gate
//! falls to zero at the reset; unlike an overlap-add shifter there is no second head and no
//! crossfade. See Jon Dattorro, “Effect Design, Part 2”, for delay-line modulation and
//! interpolation. The period envelope and deterministic per-period jitter are deliberate character;
//! a reset click is not.

use crate::delay::Delay;

const SWEEP_S: f32 = 0.070;
const BASE_S: f32 = 0.012;
const JITTER_S: f32 = 0.006;

#[derive(Clone)]
pub struct SweptShifter {
    delay: Delay,
    sample_rate: f32,
    phase: f32,
    jitter: f32,
    seed: u32,
    rng: u32,
}

impl SweptShifter {
    pub fn new(sample_rate: f32, seed: u32) -> Self {
        let sample_rate = if sample_rate.is_finite() {
            sample_rate.clamp(8_000.0, 384_000.0)
        } else {
            48_000.0
        };
        let seed = seed.max(1);
        Self {
            delay: Delay::new(((BASE_S + SWEEP_S + JITTER_S) * sample_rate).ceil() as usize + 4),
            sample_rate,
            // At zero shift the phase does not move; halfway gives a unity window.
            phase: 0.5,
            jitter: 0.0,
            seed,
            rng: seed,
        }
    }

    pub fn reset(&mut self) {
        self.delay.clear();
        self.phase = 0.5;
        self.jitter = 0.0;
        self.rng = self.seed;
    }

    /// The shifter's worst instantaneous gain. The Hann gate peaks at exactly one.
    pub const fn worst_case_gain() -> f32 {
        1.0
    }

    #[inline]
    pub fn process(&mut self, input: f32, semitones: f32, reverse: bool) -> f32 {
        let semitones = if semitones.is_finite() {
            semitones.clamp(-24.0, 24.0)
        } else {
            0.0
        };
        let ratio = 2.0f32.powf(semitones / 12.0);
        let sweep_samples = SWEEP_S * self.sample_rate;

        let (increment, delay) = if reverse {
            // read position = write - delay; d(delay)/dn = 1 + ratio gives rate -ratio.
            let increment = (1.0 + ratio) / sweep_samples;
            (
                increment,
                BASE_S * self.sample_rate + self.jitter + self.phase * sweep_samples,
            )
        } else if (ratio - 1.0).abs() < 1.0e-6 {
            (0.0, (BASE_S + 0.5 * SWEEP_S) * self.sample_rate)
        } else if ratio > 1.0 {
            let increment = (ratio - 1.0) / sweep_samples;
            (
                increment,
                BASE_S * self.sample_rate + self.jitter + (1.0 - self.phase) * sweep_samples,
            )
        } else {
            let increment = (1.0 - ratio) / sweep_samples;
            (
                increment,
                BASE_S * self.sample_rate + self.jitter + self.phase * sweep_samples,
            )
        };

        let read = self.delay.read(delay);
        self.delay.write(input);
        let window = (core::f32::consts::PI * self.phase).sin().powi(2);

        self.phase += increment;
        if self.phase >= 1.0 {
            self.phase -= self.phase.floor();
            self.rng ^= self.rng << 13;
            self.rng ^= self.rng >> 17;
            self.rng ^= self.rng << 5;
            let unit = self.rng as f32 / u32::MAX as f32;
            self.jitter = unit * JITTER_S * self.sample_rate;
        }
        read * window
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_rate_and_input_recover_to_finite_output() {
        let mut shifter = SweptShifter::new(f32::NAN, 1);
        assert!(shifter.process(f32::INFINITY, 12.0, false).is_finite());
        for n in 0..100_000 {
            let input = (n as f32 * 0.031).sin();
            assert!(shifter.process(input, 12.0, false).is_finite());
        }
    }

    #[test]
    fn zero_shift_is_an_ordinary_delay() {
        let mut shifter = SweptShifter::new(48_000.0, 1);
        let delay = ((BASE_S + 0.5 * SWEEP_S) * 48_000.0) as usize;
        let mut output = Vec::new();
        for n in 0..delay + 8 {
            output.push(shifter.process(if n == 0 { 1.0 } else { 0.0 }, 0.0, false));
        }
        let at = output
            .iter()
            .position(|x| x.abs() > 0.5)
            .expect("impulse returns");
        assert!(
            (at as isize - delay as isize).abs() <= 1,
            "returned at {at}, wanted {delay}"
        );
    }

    #[test]
    fn an_octave_moves_a_tone_to_an_octave() {
        let fs = 48_000.0;
        let mut shifter = SweptShifter::new(fs, 7);
        let samples: Vec<f32> = (0..96_000)
            .map(|n| {
                let input = (core::f32::consts::TAU * 220.0 * n as f32 / fs).sin();
                shifter.process(input, 12.0, false)
            })
            .collect();
        // Zero crossings ignore the intended envelope and expose playback rate directly.
        let start = 24_000;
        let crossings = samples[start..]
            .windows(2)
            .filter(|pair| pair[0] <= 0.0 && pair[1] > 0.0)
            .count();
        let seconds = (samples.len() - start) as f32 / fs;
        let hz = crossings as f32 / seconds;
        assert!((hz - 440.0).abs() < 12.0, "shifted frequency was {hz} Hz");
    }

    #[test]
    fn the_reset_is_gated_and_the_output_is_bounded() {
        let mut shifter = SweptShifter::new(48_000.0, 99);
        let mut peak = 0.0f32;
        let mut largest_step = 0.0f32;
        let mut previous = 0.0;
        for n in 0..200_000 {
            let input = (n as f32 * 0.071).sin();
            let out = shifter.process(input, 12.0, false);
            peak = peak.max(out.abs());
            largest_step = largest_step.max((out - previous).abs());
            previous = out;
        }
        assert!(peak <= 1.0);
        assert!(largest_step < 0.25, "a reset clicked by {largest_step}");
    }
}
