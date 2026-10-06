//! Permanent parameter definitions for `mxm-shimmer`.

use mxm_preset::PresetIdentity;
use mxm_shimmer_dsp::Placement;
use nice_plug::prelude::*;
use std::sync::{Arc, RwLock};

#[derive(Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlacementChoice {
    #[id = "input"]
    #[name = "Input"]
    Input,
    #[id = "regen"]
    #[name = "Regen"]
    Regen,
    #[id = "both"]
    #[name = "Both"]
    Both,
}

impl PlacementChoice {
    pub const fn dsp(self) -> Placement {
        match self {
            Self::Input => Placement::Input,
            Self::Regen => Placement::Regen,
            Self::Both => Placement::Both,
        }
    }
}

type ValueToString = Arc<dyn Fn(f32) -> String + Send + Sync>;
type StringToValue = Arc<dyn Fn(&str) -> Option<f32> + Send + Sync>;

fn percent_to_string() -> ValueToString {
    Arc::new(|value| format!("{:.0} %", value * 100.0))
}

fn string_to_percent() -> StringToValue {
    Arc::new(|text| {
        text.trim()
            .trim_end_matches('%')
            .trim()
            .parse::<f32>()
            .ok()
            .map(|value| value / 100.0)
    })
}

fn frequency_to_string() -> ValueToString {
    Arc::new(|value| {
        // Choose the unit from the rounded display value. Otherwise 999.9 Hz formats as
        // `1000 Hz`, parses to exactly 1000, and then formats as `1.00 kHz`; a normalized host
        // round-trip must be text-idempotent.
        if value.round() >= 1_000.0 {
            format!("{:.2} kHz", value / 1_000.0)
        } else {
            format!("{value:.0} Hz")
        }
    })
}

fn string_to_frequency() -> StringToValue {
    Arc::new(|text| {
        let lower = text.trim().to_ascii_lowercase();
        if let Some(number) = lower.strip_suffix("khz") {
            number
                .trim()
                .parse::<f32>()
                .ok()
                .map(|value| value * 1_000.0)
        } else {
            lower.trim_end_matches("hz").trim().parse::<f32>().ok()
        }
    })
}

fn time_to_string() -> ValueToString {
    Arc::new(|seconds| format!("{:.0} ms", seconds * 1_000.0))
}

fn string_to_time() -> StringToValue {
    Arc::new(|text| {
        let lower = text.trim().to_ascii_lowercase();
        lower
            .trim_end_matches("ms")
            .trim()
            .parse::<f32>()
            .ok()
            .map(|value| value / 1_000.0)
    })
}

/// **Pre-delay's tempo sync** (`plans/plan-tempo-sync-controls.md`): 1/64 to an eighth, the slice of
/// the ladder the pre-delay's 0 – 250 ms holds at 120 bpm, the top the longest.
pub const PRE_DELAY_SYNC: mxm_tempo::Ladder = mxm_tempo::Ladder::new(
    mxm_tempo::Span::new(
        mxm_tempo::Division::SixtyFourth,
        mxm_tempo::Division::Eighth,
    ),
    mxm_tempo::Direction::Time,
);

/// **Mod rate's tempo sync**: every LFO's ladder, 1/32 to four bars, the top the fastest.
pub const MOD_SYNC: mxm_tempo::Ladder =
    mxm_tempo::Ladder::new(mxm_tempo::Span::LFO, mxm_tempo::Direction::Rate);

#[derive(Params)]
pub struct MxmShimmerParams {
    #[id = "mix"]
    pub mix: FloatParam,
    #[id = "regen"]
    pub regen: FloatParam,
    #[id = "shimmer"]
    pub shimmer: FloatParam,
    #[id = "shift"]
    pub shift: FloatParam,
    #[id = "placement"]
    pub placement: EnumParam<PlacementChoice>,
    #[id = "reverse"]
    pub reverse: BoolParam,
    #[id = "freeze"]
    pub freeze: BoolParam,
    #[id = "size"]
    pub size: FloatParam,
    #[id = "diffusion"]
    pub diffusion: FloatParam,
    #[id = "modrate"]
    pub mod_rate: FloatParam,
    /// Mod rate's tempo sync: its position picks a division of the host's tempo.
    #[id = "modsync"]
    pub mod_sync: BoolParam,
    #[id = "moddepth"]
    pub mod_depth: FloatParam,
    #[id = "lowcut"]
    pub low_cut: FloatParam,
    #[id = "highcut"]
    pub high_cut: FloatParam,
    #[id = "predelay"]
    pub pre_delay: FloatParam,
    /// Pre-delay's tempo sync.
    #[id = "predelaysync"]
    pub pre_delay_sync: BoolParam,

    #[persist = "preset"]
    pub preset: RwLock<PresetIdentity>,
}

impl Default for MxmShimmerParams {
    fn default() -> Self {
        let percent = || (percent_to_string(), string_to_percent());
        let (v2s, s2v) = percent();
        let mix = FloatParam::new("Mix", 0.48, FloatRange::Linear { min: 0.0, max: 1.0 })
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_value_to_string(v2s)
            .with_string_to_value(s2v);
        let (v2s, s2v) = percent();
        let regen = FloatParam::new("Regen", 0.68, FloatRange::Linear { min: 0.0, max: 1.0 })
            .with_smoother(SmoothingStyle::Linear(30.0))
            .with_value_to_string(v2s)
            .with_string_to_value(s2v);
        let (v2s, s2v) = percent();
        let shimmer = FloatParam::new("Shimmer", 0.85, FloatRange::Linear { min: 0.0, max: 1.0 })
            .with_smoother(SmoothingStyle::Linear(30.0))
            .with_value_to_string(v2s)
            .with_string_to_value(s2v);
        let (v2s, s2v) = percent();
        let size = FloatParam::new("Size", 0.78, FloatRange::Linear { min: 0.0, max: 1.0 })
            .with_smoother(SmoothingStyle::Linear(80.0))
            .with_value_to_string(v2s)
            .with_string_to_value(s2v);
        let (v2s, s2v) = percent();
        let diffusion =
            FloatParam::new("Diffusion", 0.90, FloatRange::Linear { min: 0.0, max: 1.0 })
                .with_smoother(SmoothingStyle::Linear(40.0))
                .with_value_to_string(v2s)
                .with_string_to_value(s2v);

        Self {
            mix,
            regen,
            shimmer,
            mod_rate: FloatParam::new(
                "Mod rate",
                0.16,
                FloatRange::Skewed {
                    min: 0.01,
                    max: 8.0,
                    factor: FloatRange::skew_factor(-1.6),
                },
            )
            .with_smoother(SmoothingStyle::Linear(40.0))
            .with_unit(" Hz")
            .with_value_to_string(formatters::v2s_f32_rounded(2)),
            mod_sync: BoolParam::new("Mod rate sync", false),
            mod_depth: FloatParam::new(
                "Mod depth",
                0.0013,
                FloatRange::Skewed {
                    min: 0.0,
                    max: 0.008,
                    factor: FloatRange::skew_factor(-0.9),
                },
            )
            .with_smoother(SmoothingStyle::Linear(40.0))
            .with_value_to_string(time_to_string())
            .with_string_to_value(string_to_time()),
            shift: FloatParam::new(
                "Shift",
                12.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 24.0,
                },
            )
            .with_step_size(1.0)
            .with_smoother(SmoothingStyle::Linear(50.0))
            .with_unit(" st")
            .with_value_to_string(formatters::v2s_f32_rounded(0)),
            placement: EnumParam::new("Placement", PlacementChoice::Regen),
            reverse: BoolParam::new("Grain", false)
                .with_value_to_string(Arc::new(|value| {
                    if value { "Reverse" } else { "Forward" }.to_owned()
                }))
                .with_string_to_value(Arc::new(|text| {
                    match text.trim().to_ascii_lowercase().as_str() {
                        "reverse" | "true" => Some(true),
                        "forward" | "false" => Some(false),
                        _ => None,
                    }
                })),
            freeze: BoolParam::new("Freeze", false),
            size,
            diffusion,
            low_cut: FloatParam::new(
                "Low cut",
                180.0,
                FloatRange::Skewed {
                    min: 20.0,
                    max: 4_000.0,
                    factor: FloatRange::skew_factor(-1.4),
                },
            )
            .with_smoother(SmoothingStyle::Linear(40.0))
            .with_value_to_string(frequency_to_string())
            .with_string_to_value(string_to_frequency()),
            high_cut: FloatParam::new(
                "High cut",
                8_000.0,
                FloatRange::Skewed {
                    min: 1_000.0,
                    max: 20_000.0,
                    factor: FloatRange::skew_factor(-0.8),
                },
            )
            .with_smoother(SmoothingStyle::Linear(40.0))
            .with_value_to_string(frequency_to_string())
            .with_string_to_value(string_to_frequency()),
            pre_delay: FloatParam::new(
                "Pre-delay",
                0.030,
                FloatRange::Skewed {
                    min: 0.0,
                    max: 0.250,
                    factor: FloatRange::skew_factor(-1.2),
                },
            )
            .with_smoother(SmoothingStyle::Linear(30.0))
            .with_value_to_string(time_to_string())
            .with_string_to_value(string_to_time()),
            pre_delay_sync: BoolParam::new("Pre-delay sync", false),
            preset: RwLock::new(PresetIdentity::none()),
        }
    }
}

impl MxmShimmerParams {
    /// Pre-delay while its sync follows the host, or `None` for its free value: the modulated
    /// position picks a division on [`PRE_DELAY_SYNC`]. Resolved once a buffer by the plugin.
    pub fn synced_pre_delay(&self, tempo: Option<f64>) -> Option<f32> {
        let param = &self.pre_delay;
        PRE_DELAY_SYNC
            .resolve(
                self.pre_delay_sync.value(),
                tempo,
                param.modulated_normalized_value(),
                f64::from(param.preview_plain(0.0)),
                f64::from(param.preview_plain(1.0)),
            )
            .map(|seconds| seconds as f32)
    }

    /// Mod rate while its sync follows the host, or `None` for its free value: the modulated
    /// position picks a division on [`MOD_SYNC`]. Resolved once a buffer by the plugin.
    pub fn synced_mod_rate(&self, tempo: Option<f64>) -> Option<f32> {
        let param = &self.mod_rate;
        MOD_SYNC
            .resolve(
                self.mod_sync.value(),
                tempo,
                param.modulated_normalized_value(),
                f64::from(param.preview_plain(0.0)),
                f64::from(param.preview_plain(1.0)),
            )
            .map(|hz| hz as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Pre-delay's sync picks a division and is inert without a tempo**
    /// (`plans/plan-tempo-sync-controls.md`): off, or with no tempo, the knob's own time stands; on
    /// at 120 bpm the ends are the ladder's ends that the range can hold, the top the longest.
    #[test]
    fn pre_delay_sync_picks_a_division_and_is_inert_without_a_tempo() {
        use nice_plug::params::InternalParamMut;
        fn set<P: InternalParamMut>(param: &P, normalized: f32) {
            unsafe {
                let _ = param._internal_set_normalized_value(normalized);
            }
        }
        let p = MxmShimmerParams::default();
        set(&p.pre_delay, 1.0);
        assert_eq!(
            p.synced_pre_delay(Some(120.0)),
            None,
            "off is the free time"
        );
        set(&p.pre_delay_sync, 1.0);
        assert_eq!(p.synced_pre_delay(None), None, "no tempo is the free time");

        let top = p.synced_pre_delay(Some(120.0)).expect("synced at a tempo");
        set(&p.pre_delay, 0.0);
        let bottom = p.synced_pre_delay(Some(120.0)).expect("synced at a tempo");
        let (lo, hi) = (
            f64::from(p.pre_delay.preview_plain(0.0)),
            f64::from(p.pre_delay.preview_plain(1.0)),
        );
        assert!(
            top > bottom,
            "the top of a time is the longest: {bottom} to {top}"
        );
        let reach = PRE_DELAY_SYNC.reachable(120.0, lo, hi).divisions();
        let shortest = reach[0].seconds(120.0) as f32;
        let longest = reach[reach.len() - 1].seconds(120.0) as f32;
        assert!(
            (bottom - shortest).abs() < 1e-5,
            "{bottom} against {shortest}"
        );
        assert!((top - longest).abs() < 1e-5, "{top} against {longest}");
    }

    /// **Mod rate's sync picks a division and is inert without a tempo**
    /// (`plans/plan-tempo-sync-controls.md`): off, or with no tempo, the knob's own hertz stand; on
    /// at 120 bpm the ends are the ladder's ends that the range can hold, the top the fastest.
    #[test]
    fn mod_sync_picks_a_division_and_is_inert_without_a_tempo() {
        use nice_plug::params::InternalParamMut;
        fn set<P: InternalParamMut>(param: &P, normalized: f32) {
            unsafe {
                let _ = param._internal_set_normalized_value(normalized);
            }
        }
        let p = MxmShimmerParams::default();
        set(&p.mod_rate, 1.0);
        assert_eq!(p.synced_mod_rate(Some(120.0)), None, "off is the free rate");
        set(&p.mod_sync, 1.0);
        assert_eq!(p.synced_mod_rate(None), None, "no tempo is the free rate");

        let top = p.synced_mod_rate(Some(120.0)).expect("synced at a tempo");
        set(&p.mod_rate, 0.0);
        let bottom = p.synced_mod_rate(Some(120.0)).expect("synced at a tempo");
        let (lo, hi) = (
            f64::from(p.mod_rate.preview_plain(0.0)),
            f64::from(p.mod_rate.preview_plain(1.0)),
        );
        assert!(
            top > bottom,
            "the top of a rate is the fastest: {bottom} to {top}"
        );
        let reach = MOD_SYNC.reachable(120.0, lo, hi).divisions();
        let fastest = reach[0].hz(120.0) as f32;
        let slowest = reach[reach.len() - 1].hz(120.0) as f32;
        assert!((top - fastest).abs() < 1e-4, "{top} against {fastest}");
        assert!(
            (bottom - slowest).abs() < 1e-4,
            "{bottom} against {slowest}"
        );
    }

    #[test]
    fn the_effect_opens_engaged_and_freeze_does_not() {
        let params = MxmShimmerParams::default();
        assert!(params.mix.value() > 0.0);
        assert!(params.shimmer.value() > 0.0);
        assert!(!params.freeze.value());
        assert_eq!(params.placement.value(), PlacementChoice::Regen);
    }

    #[test]
    fn formatted_floats_are_idempotent_through_the_normalized_host_round_trip() {
        let params = MxmShimmerParams::default();
        let validator_grid = (0..=19).map(|step| step as f32 / 19.0);
        let frequency_boundaries = [999.4, 999.9, 1_000.0, 1_000.1, 1_004.9, 1_005.1]
            .into_iter()
            .flat_map(|plain| {
                [
                    params.low_cut.preview_normalized(plain),
                    params.high_cut.preview_normalized(plain),
                ]
            });
        for (name, param) in [
            ("mix", &params.mix),
            ("regen", &params.regen),
            ("shimmer", &params.shimmer),
            ("shift", &params.shift),
            ("size", &params.size),
            ("diffusion", &params.diffusion),
            ("lowcut", &params.low_cut),
            ("highcut", &params.high_cut),
            ("predelay", &params.pre_delay),
        ] {
            for normalized in validator_grid.clone().chain(frequency_boundaries.clone()) {
                let first = param.normalized_value_to_string(normalized, true);
                let reparsed = param
                    .string_to_normalized_value(&first)
                    .unwrap_or_else(|| panic!("{name} rejected {first:?}"));
                let second = param.normalized_value_to_string(reparsed, true);
                assert_eq!(second, first, "{name} text changed at {normalized}");
            }
        }
    }

    #[test]
    fn every_formatted_float_parses_back() {
        let params = MxmShimmerParams::default();
        for (name, param) in [
            ("mix", &params.mix),
            ("regen", &params.regen),
            ("shimmer", &params.shimmer),
            ("size", &params.size),
            ("diffusion", &params.diffusion),
            ("lowcut", &params.low_cut),
            ("highcut", &params.high_cut),
            ("predelay", &params.pre_delay),
        ] {
            for n in 0..=20 {
                let value = n as f32 / 20.0;
                let text = param.normalized_value_to_string(value, false);
                assert!(
                    param.string_to_normalized_value(&text).is_some(),
                    "{name} rejected {text}"
                );
            }
        }
    }
}
