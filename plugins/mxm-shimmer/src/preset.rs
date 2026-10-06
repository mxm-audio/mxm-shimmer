//! Factory sounds and the shared preset-system seam.

use std::sync::RwLock;

pub use mxm_preset::{
    Category, Entry, INIT_NAME, Library, Loaded, Origin, Preset, PresetIdentity, Refused, Value,
    factory, loaded, mark_loaded, mark_none, read_favourites, snapshot, write_favourites,
};

use crate::params::MxmShimmerParams;

/// **The tempo syncs this plugin gained on 2026-09-25** (`plans/plan-tempo-sync-controls.md`). A
/// preset file written before them was written unsynced, so each loads off rather than keeping the
/// instance's sync, and without reporting a missing control.
pub(crate) const TEMPO_SYNC_IDS: &[&str] = &["predelaysync", "modsync"];

impl mxm_preset::Instrument for MxmShimmerParams {
    fn clap_id(&self) -> &'static str {
        crate::CLAP_ID
    }

    fn parameters(&self) -> Vec<(&'static str, &dyn mxm_preset::ErasedParam)> {
        // Declaration order is the preset/init write order. It is deliberately independent of the
        // editor's signal-flow order, which may change without changing host-facing state writes.
        vec![
            ("mix", &self.mix),
            ("regen", &self.regen),
            ("shimmer", &self.shimmer),
            ("shift", &self.shift),
            ("placement", &self.placement),
            ("reverse", &self.reverse),
            ("freeze", &self.freeze),
            ("size", &self.size),
            ("diffusion", &self.diffusion),
            ("modrate", &self.mod_rate),
            ("modsync", &self.mod_sync),
            ("moddepth", &self.mod_depth),
            ("lowcut", &self.low_cut),
            ("highcut", &self.high_cut),
            ("predelay", &self.pre_delay),
            ("predelaysync", &self.pre_delay_sync),
        ]
    }

    fn identity(&self) -> &RwLock<PresetIdentity> {
        &self.preset
    }

    fn factory_files(&self) -> &'static [(&'static str, &'static str)] {
        FACTORY_FILES
    }

    fn default_missing_legacy_parameter(&self, id: &str) -> bool {
        TEMPO_SYNC_IDS.contains(&id)
    }
}

pub const FACTORY_FILES: &[(&str, &str)] = &[
    (
        "Ascending hall",
        include_str!("../presets/ascending-hall.json"),
    ),
    ("Input halo", include_str!("../presets/input-halo.json")),
    ("Dual orbit", include_str!("../presets/dual-orbit.json")),
    ("Fifth heaven", include_str!("../presets/fifth-heaven.json")),
    ("Two octaves", include_str!("../presets/two-octaves.json")),
    (
        "Reverse bloom",
        include_str!("../presets/reverse-bloom.json"),
    ),
    ("Dark ascent", include_str!("../presets/dark-ascent.json")),
    ("Air chapel", include_str!("../presets/air-chapel.json")),
    ("Small glass", include_str!("../presets/small-glass.json")),
    ("Long bloom", include_str!("../presets/long-bloom.json")),
    ("Plain hall", include_str!("../presets/plain-hall.json")),
    ("Wide wash", include_str!("../presets/wide-wash.json")),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// **A project saved before the tempo syncs restores them Off** (`mxm_preset::add_switches_off`),
    /// whatever this instance had.
    #[test]
    fn an_older_state_restores_the_tempo_syncs_off() {
        use nice_plug::prelude::Plugin as _;
        let mut state = nice_plug::prelude::PluginState {
            version: String::new(),
            params: Default::default(),
            fields: Default::default(),
        };
        crate::MxmShimmer::filter_state(&mut state);
        for id in TEMPO_SYNC_IDS {
            assert!(
                matches!(
                    state.params.get(*id),
                    Some(nice_plug::plugin::ParamValue::Bool(false))
                ),
                "{{id}} was not restored off"
            );
        }
    }

    /// **A preset saved before the tempo syncs loads them off, and cleanly** ([`TEMPO_SYNC_IDS`]).
    #[test]
    fn a_preset_from_before_the_tempo_syncs_loads_them_off() {
        let params = crate::params::MxmShimmerParams::default();
        let mut old = mxm_preset::Preset::init(&params);
        for id in TEMPO_SYNC_IDS {
            old.params.remove(*id);
        }
        let (writes, problems) = old.resolve(&params);
        assert!(problems.is_empty(), "{{problems:?}}");
        for id in TEMPO_SYNC_IDS {
            assert!(
                writes.iter().any(|(w, _, v)| w == id && *v == 0.0),
                "{{id}} was not written off"
            );
        }
    }

    use nice_plug::prelude::Param;

    /// Factory designs in the parameters' own units. Placement uses 0/1/2 for Input/Regen/Both;
    /// the two switches use 0/1. Keeping Hz, seconds and semitones here makes the bank reviewable
    /// without reverse-engineering each parameter's skewed normalised range.
    pub type Design = (&'static str, Category, &'static [(&'static str, f32)]);

    pub fn designs() -> &'static [Design] {
        DESIGNS
    }

    const INPUT: f32 = 0.0;
    const REGEN: f32 = 1.0;
    const BOTH: f32 = 2.0;
    const FORWARD: f32 = 0.0;
    const REVERSE: f32 = 1.0;

    const DESIGNS: &[Design] = &[
        (
            "Ascending hall",
            Category::Pad,
            &[
                ("mix", 0.44),
                ("regen", 0.58),
                ("shimmer", 0.72),
                ("shift", 12.0),
                ("placement", REGEN),
                ("reverse", FORWARD),
                ("size", 0.74),
                ("diffusion", 0.90),
                ("modrate", 0.10),
                ("moddepth", 0.0005),
                ("lowcut", 240.0),
                ("highcut", 6_500.0),
                ("predelay", 0.025),
            ],
        ),
        (
            "Input halo",
            Category::Pad,
            &[
                ("mix", 0.38),
                ("regen", 0.30),
                ("shimmer", 0.90),
                ("shift", 12.0),
                ("placement", INPUT),
                ("reverse", FORWARD),
                ("size", 0.52),
                ("diffusion", 0.82),
                ("modrate", 0.24),
                ("moddepth", 0.0018),
                ("lowcut", 700.0),
                ("highcut", 12_000.0),
                ("predelay", 0.0),
            ],
        ),
        (
            "Dual orbit",
            Category::Fx,
            &[
                ("mix", 0.55),
                ("regen", 0.46),
                ("shimmer", 0.62),
                ("shift", 19.0),
                ("placement", BOTH),
                ("reverse", FORWARD),
                ("size", 0.66),
                ("diffusion", 0.48),
                ("modrate", 0.55),
                ("moddepth", 0.0045),
                ("lowcut", 300.0),
                ("highcut", 9_000.0),
                ("predelay", 0.065),
            ],
        ),
        (
            "Fifth heaven",
            Category::Pad,
            &[
                ("mix", 0.50),
                ("regen", 0.58),
                ("shimmer", 0.88),
                ("shift", 7.0),
                ("placement", REGEN),
                ("reverse", REVERSE),
                ("size", 0.88),
                ("diffusion", 0.94),
                ("modrate", 0.05),
                ("moddepth", 0.00035),
                ("lowcut", 200.0),
                ("highcut", 6_000.0),
                ("predelay", 0.035),
            ],
        ),
        (
            "Two octaves",
            Category::Fx,
            &[
                ("mix", 0.42),
                ("regen", 0.30),
                ("shimmer", 1.0),
                ("shift", 24.0),
                ("placement", INPUT),
                ("reverse", FORWARD),
                ("size", 0.38),
                ("diffusion", 0.58),
                ("modrate", 0.90),
                ("moddepth", 0.0006),
                ("lowcut", 900.0),
                ("highcut", 16_000.0),
                ("predelay", 0.0),
            ],
        ),
        (
            "Reverse bloom",
            Category::Fx,
            &[
                ("mix", 0.52),
                ("regen", 0.52),
                ("shimmer", 0.74),
                ("shift", 12.0),
                ("placement", BOTH),
                ("reverse", REVERSE),
                ("size", 0.84),
                ("diffusion", 0.90),
                ("modrate", 0.07),
                ("moddepth", 0.00025),
                ("lowcut", 280.0),
                ("highcut", 7_000.0),
                ("predelay", 0.110),
            ],
        ),
        (
            "Dark ascent",
            Category::Pad,
            &[
                ("mix", 0.48),
                ("regen", 0.60),
                ("shimmer", 0.66),
                ("shift", 19.0),
                ("placement", REGEN),
                ("reverse", FORWARD),
                ("size", 0.94),
                ("diffusion", 0.93),
                ("modrate", 0.09),
                ("moddepth", 0.0008),
                ("lowcut", 550.0),
                ("highcut", 2_800.0),
                ("predelay", 0.055),
            ],
        ),
        (
            "Air chapel",
            Category::Pad,
            &[
                ("mix", 0.40),
                ("regen", 0.34),
                ("shimmer", 0.96),
                ("shift", 19.0),
                ("placement", INPUT),
                ("reverse", REVERSE),
                ("size", 0.70),
                ("diffusion", 0.97),
                ("modrate", 0.18),
                ("moddepth", 0.0020),
                ("lowcut", 1_200.0),
                ("highcut", 18_000.0),
                ("predelay", 0.015),
            ],
        ),
        (
            "Small glass",
            Category::Fx,
            &[
                ("mix", 0.32),
                ("regen", 0.18),
                ("shimmer", 0.75),
                ("shift", 12.0),
                ("placement", BOTH),
                ("reverse", FORWARD),
                ("size", 0.08),
                ("diffusion", 0.30),
                ("modrate", 2.40),
                ("moddepth", 0.0060),
                ("lowcut", 1_000.0),
                ("highcut", 19_000.0),
                ("predelay", 0.0),
            ],
        ),
        (
            "Long bloom",
            Category::Pad,
            &[
                ("mix", 0.56),
                ("regen", 0.67),
                ("shimmer", 0.68),
                ("shift", 12.0),
                ("placement", REGEN),
                ("reverse", FORWARD),
                ("size", 0.99),
                ("diffusion", 0.99),
                // Modulated interpolation costs decay, so the long archetype deliberately moves
                // least. Regen stays below the measured large-Size self-oscillation threshold.
                ("modrate", 0.03),
                ("moddepth", 0.0002),
                ("lowcut", 140.0),
                ("highcut", 5_200.0),
                ("predelay", 0.130),
            ],
        ),
        (
            "Plain hall",
            Category::Fx,
            &[
                ("mix", 0.34),
                ("regen", 0.46),
                ("shimmer", 0.0),
                ("shift", 12.0),
                ("placement", INPUT),
                ("reverse", FORWARD),
                ("size", 0.76),
                ("diffusion", 0.92),
                ("modrate", 0.32),
                ("moddepth", 0.0032),
                ("lowcut", 100.0),
                ("highcut", 6_500.0),
                ("predelay", 0.018),
            ],
        ),
        (
            "Wide wash",
            Category::Pad,
            &[
                ("mix", 0.60),
                ("regen", 0.38),
                ("shimmer", 0.52),
                ("shift", 7.0),
                ("placement", BOTH),
                ("reverse", FORWARD),
                ("size", 0.92),
                ("diffusion", 0.98),
                ("modrate", 0.28),
                ("moddepth", 0.0075),
                ("lowcut", 350.0),
                ("highcut", 10_500.0),
                ("predelay", 0.045),
            ],
        ),
    ];

    fn normalised_for(params: &MxmShimmerParams, id: &str, plain: f32) -> f32 {
        match id {
            "mix" => params.mix.preview_normalized(plain),
            "regen" => params.regen.preview_normalized(plain),
            "shimmer" => params.shimmer.preview_normalized(plain),
            "shift" => params.shift.preview_normalized(plain),
            "placement" => {
                if plain == INPUT {
                    0.0
                } else if plain == REGEN {
                    0.5
                } else if plain == BOTH {
                    1.0
                } else {
                    panic!("placement selector {plain} is not Input/Regen/Both")
                }
            }
            "reverse" | "freeze" => {
                if plain == FORWARD {
                    0.0
                } else if plain == REVERSE {
                    1.0
                } else {
                    panic!("switch value {plain} is not 0/1")
                }
            }
            "size" => params.size.preview_normalized(plain),
            "diffusion" => params.diffusion.preview_normalized(plain),
            "modrate" => params.mod_rate.preview_normalized(plain),
            "moddepth" => params.mod_depth.preview_normalized(plain),
            "lowcut" => params.low_cut.preview_normalized(plain),
            "highcut" => params.high_cut.preview_normalized(plain),
            "predelay" => params.pre_delay.preview_normalized(plain),
            other => panic!("`{other}` is not a parameter"),
        }
    }

    pub fn generated(
        params: &MxmShimmerParams,
        name: &str,
        category: Category,
        overrides: &[(&str, f32)],
    ) -> Preset {
        let bindings = crate::editor::sections::all_parameters(params);
        let mut preset = Preset::init(params);
        preset.name = name.to_owned();
        preset.category = category;
        for (id, plain) in overrides {
            let binding = bindings
                .iter()
                .find(|binding| binding.id == *id)
                .expect("real parameter");
            let value = normalised_for(params, id, *plain).clamp(0.0, 1.0);
            preset.params.insert(
                (*id).to_owned(),
                Value {
                    v: value,
                    text: binding.param.format(value),
                },
            );
        }
        preset
    }

    #[test]
    #[ignore = "writes the factory preset files"]
    fn write_the_factory_presets() {
        let params = MxmShimmerParams::default();
        for (name, category, overrides) in DESIGNS {
            let preset = generated(&params, name, *category, overrides);
            let filename = name.to_ascii_lowercase().replace(' ', "-") + ".json";
            std::fs::write(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("presets")
                    .join(filename),
                preset.to_json(),
            )
            .expect("write preset");
        }
    }

    #[test]
    fn preset_writes_follow_parameter_declaration_order() {
        let params = MxmShimmerParams::default();
        let ids: Vec<_> = mxm_preset::Instrument::parameters(&params)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        assert_eq!(
            ids,
            [
                "mix",
                "regen",
                "shimmer",
                "shift",
                "placement",
                "reverse",
                "freeze",
                "size",
                "diffusion",
                "modrate",
                "modsync",
                "moddepth",
                "lowcut",
                "highcut",
                "predelay",
                "predelaysync",
            ]
        );
    }

    #[test]
    fn the_factory_files_match_the_design() {
        let params = MxmShimmerParams::default();
        assert_eq!(FACTORY_FILES.len(), DESIGNS.len());
        for (name, category, overrides) in DESIGNS {
            let shipped = Preset::parse(
                FACTORY_FILES
                    .iter()
                    .find(|(found, _)| found == name)
                    .expect("listed")
                    .1,
                crate::CLAP_ID,
            )
            .expect("factory preset parses");
            assert_eq!(
                shipped,
                generated(&params, name, *category, overrides),
                "{name}"
            );
        }
    }

    fn meaningful_axes(left: &Preset, right: &Preset) -> usize {
        left.params
            .iter()
            .filter(|(id, value)| {
                *id != "freeze"
                    && (value.v - right.params.get(*id).expect("complete preset").v).abs() >= 0.06
            })
            .count()
    }

    #[test]
    fn every_factory_pair_moves_at_least_four_meaningful_parameter_axes() {
        let params = MxmShimmerParams::default();
        let presets: Vec<_> = DESIGNS
            .iter()
            .map(|(name, category, overrides)| generated(&params, name, *category, overrides))
            .collect();
        for (index, left) in presets.iter().enumerate() {
            for right in &presets[index + 1..] {
                let changed = meaningful_axes(left, right);
                assert!(
                    changed >= 4,
                    "{} and {} differ meaningfully on only {changed} axes",
                    left.name,
                    right.name
                );
            }
        }
    }

    #[test]
    fn no_factory_sound_is_init_under_another_name() {
        let params = MxmShimmerParams::default();
        let init = Preset::init(&params);
        for (name, category, overrides) in DESIGNS {
            let preset = generated(&params, name, *category, overrides);
            let changed = meaningful_axes(&init, &preset);
            assert!(
                changed >= 4,
                "{name} differs meaningfully from Init on only {changed} axes"
            );
        }
    }

    #[test]
    fn every_factory_preset_is_complete_and_categorised() {
        let params = MxmShimmerParams::default();
        for (name, text) in FACTORY_FILES {
            let preset = Preset::parse(text, crate::CLAP_ID).expect(name);
            assert_ne!(preset.category, Category::Uncategorised, "{name}");
            assert!(preset.resolve(&params).1.is_empty(), "{name} is incomplete");
        }
        assert_eq!(factory(&params).first().expect("Init").name, INIT_NAME);
    }
}

#[cfg(test)]
mod regenerate {
    use super::tests::*;

    #[test]
    #[ignore]
    fn rewrite_factory_files() {
        let params = crate::params::MxmShimmerParams::default();
        for (name, category, overrides) in designs() {
            let slug = name.to_lowercase().replace(' ', "-");
            let path = format!("presets/{slug}.json");
            let json = generated(&params, name, *category, overrides).to_json();
            std::fs::write(
                &path,
                json + "
",
            )
            .expect("write preset");
            println!("wrote {path}");
        }
    }
}
