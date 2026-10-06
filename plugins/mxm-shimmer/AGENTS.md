# AGENTS.md — plugins/mxm-shimmer/

Parent: [`../AGENTS.md`](../AGENTS.md) · The detail, history and reasoning behind each rule:
[`NOTES.md`](NOTES.md)

# Purpose

The `mxm-shimmer` CLAP effect: plugin shell, permanent parameters and identity, factory presets,
control map, telemetry and four-card MXM editor around `crates/mxm-shimmer-dsp`.

Product plan: `plans/plan-mxm-shimmer.md` in the private archive. UI brief:
[`../../docs/briefs/mxm-shimmer.md`](../../docs/briefs/mxm-shimmer.md).

# Ownership

- `src/lib.rs` — CLAP export, layouts, process/activity contract and DSP adaptation.
- `src/params.rs` — sixteen permanent parameter ids (the two tempo syncs the newest) and defaults.
- `src/preset.rs`, `presets/` — twelve categorized factory sounds through `mxm-preset`.
- `examples/shimmer_preset_audit.rs` — repeatable long-render descriptors for every shipped factory sound.
- `src/editor.rs`, `src/editor/` — app bar, paging, bindings and Space/Motion/Ascent/Loop cards.
- `src/telemetry.rs` — lock-free peak and wet-difference telemetry.
- `control-map.json` — this effect's assignments into the normative collection map.
- `README.md` — product documentation; the licence is the repository's root `LICENSE` (`../../LICENSE`).

# Local Contracts

## Identity is permanent

- Product name: `mxm-shimmer`.
- CLAP id: `dk.mxm.mxm-shimmer`.
- `plugin_name!` in `src/lib.rs` is the one crate-local name literal; the display name and CLAP id
  derive from it. `bundler.toml` is the one external duplicate and a test pins agreement.
- This is an original effect named for the pitch-shifted feedback-reverb practice, not a copy of a
  product and not a promotion from an instrument.

## Audio layouts and Off

The declared layouts are mono input to stereo output and stereo input to stereo output. Mono input
feeds both wet channels; stereo input preserves L/R through the dry path. There is no MIDI input and
no developer channel.

Mix is a dry/wet crossfade. **Mix at exactly zero is Off**: output is dry to the bit in both declared
layouts, the DSP empties once, and silence thereafter is parked. Off wins over Freeze. Re-engaging
starts from empty rather than spilling an old held tail.

## Permanent parameter ids

| Id | Display | Contract |
|---|---|---|
| `mix` | Mix | `0..1` crossfade and Off at zero |
| `regen` | Regen | **the loop gain, and the only control that adds gain to the system.** Reaches one; the top few percent self-oscillate |
| `shimmer` | Shimmer | shifted level, separate from Regen |
| `shift` | Shift | stepped upward semitones from 0 through 24 on both wet channels |
| `placement` | Placement | stable enum order `Input`, `Regen`, `Both` |
| `reverse` | Grain | Forward/Reverse swept-delay travel |
| `freeze` | Freeze | stable accumulating held topology, distinct from Regen |
| `size` | Size | normalized reverb-network scale |
| `diffusion` | Diffusion | normalized density/scattering, independent of decay length |
| `modrate` | Mod rate | 0.01–8 Hz base rate of the late field's delay drift |
| `modsync` | Mod rate sync | Mod rate's tempo sync: on `params::MOD_SYNC`, every LFO's ladder, the top the fastest |
| `moddepth` | Mod depth | 0–8 ms of that drift; more of it thickens the tail and shortens it |
| `lowcut` | Low cut | high-pass inside the outer loop |
| `highcut` | High cut | low-pass inside the outer loop |
| `predelay` | Pre-delay | 0–250 ms before the wet summing node; zero gives about 19 ms onset through the DSP early cluster |
| `predelaysync` | Pre-delay sync | Pre-delay's tempo sync: on `params::PRE_DELAY_SYNC`, 1/64 to an eighth, the top the longest |

Do not rename or recycle an id. Parameter smoothing advances once per sample.

- **The two tempo syncs glide through the controls' own smoothers**, resolved once a block by
  `follow_tempo`; a free control is untouched.
  `a_synced_pre_delay_glides_to_its_division_and_back_to_the_knob` holds it
  ([NOTES.md § The two tempo syncs](NOTES.md#the-two-tempo-syncs)).
- Placement, Reverse and Freeze are discrete DSP transitions; their live-tail behavior belongs to the
  DSP crate. Activation and reset first settle the engine to the current parameter topology,
  including state restored while the effect was not processing.
- **Freeze holds the reverb's experience, not a literal sample buffer**: an energy-preserving held
  network outside the shifter and outer filters, input still open (each new signal shifted once),
  silent and parked when armed on silence, and **no output-derived level normaliser**
  ([NOTES.md § Freeze](NOTES.md#freeze-holds-the-reverbs-experience)).
- Init is the post-listening-rejection audition setting; owner acceptance of this third listening
  build remains manual ([NOTES.md § Init](NOTES.md#init)).

**Regen is loop gain, not merely a widened range.** Its top reaches self-oscillation, and the
threshold falls as Size rises. Every high-Regen preset must therefore be rendered;
`shimmer_preset_audit` fails if a factory sound intended to decay does not.

## Activity and tail

- Nonzero input, a live Freeze texture, or an empty parked engine reports `ProcessStatus::Normal`.
- A decaying tail reports `Tail(n)`, recomputed from current controls every block.
- Freeze reports `Normal` only when signal is actually sustaining; Freeze on an empty engine creates
  no sound and remains parked until later input wakes and joins the held field.
- Reset clears all DSP state. Input scanning flushes non-finite and subnormal values before they
  reach recursion.
- **The decision is rate-independent.** The two level followers behind `is_quiet` are times rather
  than per-sample coefficients, so the same signal is called the same thing at every accepted rate
  and `Normal`/`Tail(n)` does not change with the host's. The DSP crate owns the contract.

A large finite tail must not stand in for a sustaining texture. Empty means no callback work beyond
the input scan and mono-output copy required by the negotiated layout.

## Presets

- The shared preset system supplies Init; twelve complete factory presets ship in Pad or Fx
  categories. The design table in `src/preset.rs` is the source for regenerating and
  equality-testing their JSON, in the parameters' own units.
- Every factory pair, and every factory sound against Init, must differ by at least `0.06` on four
  normalized parameter axes; the audit's closest descriptor pair must stay above its failing floor.
- Families come from public maker guidance, not commercial preset data. No commercial factory value
  or name is copied, and factory names contain no reference maker or model identity.
- The audit guards the bank's spread; it does not approve character, which remains an
  owner-listening gate ([NOTES.md § Presets](NOTES.md#presets)).

## Control map

Mix fills the existing `fx.reverb` role. Page 12, Effects/Shimmer, maps seven performed controls in
this stable order: Shimmer, Regen, Shift, Placement, Grain/Reverse, Size, Freeze. The eighth slot is
empty. Diffusion, Low Cut, High Cut and Pre-delay are stored shape controls and intentionally remain
unmapped while staying available to host automation.

`control-map.json` uses the data-driven instrument-map schema with this CLAP id; it must be staged
beside both debug and release bundles.

## Editor

- Four stable Effects cards — Space, Motion, Ascent, Loop, in that order — each as wide as its
  controls. Opening size `REFERENCE` and minimum `MINIMUM` are held by
  `the_opening_size_is_the_budget_hugged` and `the_minimum_holds_the_widest_card`.
- Every card is a `mxm_ui::tree`; **floors are computed**, never declared;
  `every_card_passes_the_tree_checks_in_every_state` holds it.
- The bloom display reads only lossy atomics from the audio thread (`take_shimmer` once, before the
  frame), publishes an accessibility label and states Off and Held in text.
- Every parameter is bound exactly once, has a tooltip, and brackets host gestures through
  `editor/binding.rs`. Effects carry no developer-category CC path. More:
  [NOTES.md § Editor](NOTES.md#editor).

# Work Guidance

- Do not move DSP into this crate or add plugin-framework types to `mxm-shimmer-dsp`.
- Do not claim a compile/test result approved the P3.5 listening defaults. The first sound was
  rejected as a slap delay; the cascaded-diffusor core, two upward channel voices, revised Init,
  reverse sweep and level-style Shimmer remain subject to owner listening.
- Do not add the disputed burst gate without first falsifying the research interpretation as the
  plan requires.
- Preserve mono/stereo Off vectors and activity status when changing process code.

# Verification

The DSP suite includes a thirty-second early/late Freeze energy and density comparison; the factory
audit covers the non-Freeze bank and reports the current 0.653 minimum descriptor separation.
Automated checks do not establish broader product/preset listening, Bitwig operation, native §15, or
Linux/macOS behavior.

```bash
cargo test -p mxm-shimmer-dsp
cargo test -p mxm-shimmer
cargo clippy -p mxm-shimmer-dsp --all-targets
cargo clippy -p mxm-shimmer --all-targets
# Every page, light and dark, for review -> target/layout-tree/mxm-shimmer/<MXM_PICTURES tag>/
MXM_PICTURES=after cargo test -p mxm-shimmer --lib tree_pictures -- --ignored
cargo run -p mxm-shimmer --release --example shimmer_preset_audit
cargo xtask bundle mxm-shimmer
clap-validator validate "target/bundled/mxm-shimmer.clap"
cargo xtask bundle mxm-shimmer --release
clap-validator validate "target/bundled/mxm-shimmer.clap"
```

Also run the control-map tests after changing `control-map.json` or the collection page. Since the
split this plugin's map is read in this repository's `effect_chain` host test (its `control_map`
module); the player's own suites run in mxm-player:

```bash
cargo test -p mxm-shimmer-host-tests --test effect_chain
# in mxm-player:
cargo test -p mxm-player control_map --lib
cargo test -p mxm-player --test t5_control_map
```

Manual gates still required: broader owner listening, native design-system §15 review in both themes,
MXM Player audition, and Bitwig mono/stereo automation/state/tail/reset checks. Linux and macOS are
not verified on the Windows development machine. *Since the split (2026-10-06):* Linux and macOS are checked later, together, and by CI on `v*` tags; the manual gates there are still open.

# Child DOX Index

No child `AGENTS.md` files.
