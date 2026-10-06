//! Allocation-free-at-audio-rate fractional delay lines.

#[derive(Clone)]
pub(crate) struct Delay {
    data: Vec<f32>,
    write: usize,
    written: usize,
}

impl Delay {
    pub(crate) fn new(max_samples: usize) -> Self {
        Self {
            data: vec![0.0; max_samples.max(4) + 2],
            write: 0,
            written: 0,
        }
    }

    #[inline]
    pub(crate) fn read(&self, delay_samples: f32) -> f32 {
        let len = self.data.len();
        let delay_samples = if delay_samples.is_finite() {
            delay_samples.clamp(1.0, (len - 2) as f32)
        } else {
            1.0
        };
        // Split time into an integer age and a fractional interpolation before wrapping. At high
        // sample rates these buffers are tens of thousands of samples long; wrapping the whole
        // position in f32 can round a point just below `len` up to `len` and index past the buffer.
        let newer_age = delay_samples.floor() as usize;
        let fraction = delay_samples - newer_age as f32;
        let older_age = newer_age + 1;
        let newer = if newer_age <= self.written {
            self.data[(self.write + len - newer_age) % len]
        } else {
            0.0
        };
        let older = if older_age <= self.written {
            self.data[(self.write + len - older_age) % len]
        } else {
            0.0
        };
        newer + fraction * (older - newer)
    }

    #[inline]
    pub(crate) fn write(&mut self, sample: f32) {
        self.data[self.write] = flush(sample);
        self.write += 1;
        if self.write == self.data.len() {
            self.write = 0;
        }
        if self.written < self.data.len() {
            self.written += 1;
        }
    }

    pub(crate) fn clear(&mut self) {
        // Invalidating history is constant-time. Discrete automation can reset shifters from the
        // audio thread, so clearing a high-rate delay allocation here would create a latency spike.
        self.write = 0;
        self.written = 0;
    }
}

#[inline]
pub(crate) fn flush(value: f32) -> f32 {
    if value.abs() < f32::MIN_POSITIVE || !value.is_finite() {
        0.0
    } else {
        value
    }
}

#[inline]
pub(crate) fn bounded(value: f32) -> f32 {
    // A monotonic rational saturator, exactly bounded in f32.
    let value = flush(value);
    value / (1.0 + value.abs())
}

/// The per-sample coefficient of a one-pole follower that settles in `seconds` at `sample_rate`.
///
/// The one place a follower's speed is turned into a number, so that a follower is written as a
/// *time* and stays that time at every accepted rate. Same form as the smoothers in
/// `mxm-grain-fx-dsp`.
#[inline]
pub(crate) fn follower_coefficient(seconds: f32, sample_rate: f32) -> f32 {
    let samples = (seconds * sample_rate).max(1.0);
    1.0 - (-1.0 / samples).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fractional_read_cannot_round_the_wrap_position_to_the_buffer_length() {
        let mut delay = Delay::new(49_920);
        delay.write(0.5);
        // In f32, `1 - 1.0001` wrapped by a roughly 50k-sample buffer rounds to exactly
        // the buffer length. The old interpolation then indexed one element past the allocation.
        let old_position = (delay.write as f32 - 1.0001).rem_euclid(delay.data.len() as f32);
        assert_eq!(old_position, delay.data.len() as f32);
        assert!(delay.read(1.0001).is_finite());
    }

    #[test]
    fn clearing_invalidates_history_without_visiting_the_allocation() {
        let mut delay = Delay::new(32);
        for n in 0..64 {
            delay.write(n as f32 + 1.0);
        }
        delay.clear();
        assert_eq!(delay.read(1.0), 0.0);
        assert_eq!(delay.read(12.5), 0.0);

        delay.write(0.5);
        assert_eq!(delay.read(1.0), 0.5);
        assert!((delay.read(1.25) - 0.375).abs() < 1.0e-6);
    }
}
