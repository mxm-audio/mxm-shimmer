# AGENTS.md — plugins/mxm-shimmer/

Parent: [`../AGENTS.md`](../AGENTS.md)

# Purpose

The `mxm-shimmer` CLAP effect: plugin shell, permanent parameters and identity, factory presets,
control map, telemetry and four-card MXM editor around `crates/mxm-shimmer-dsp`.

Product plan: `../../plans/plan-mxm-shimmer.md` (`plans/plan-mxm-shimmer.md` in the private archive). UI brief:
[`../../docs/briefs/mxm-shimmer.md`](../../docs/briefs/mxm-shimmer.md).

# Ownership

- `src/lib.rs` — CLAP export, layouts, process/activity contract and DSP adaptation.
- `src/params.rs` — sixteen permanent parameter ids (the two tempo syncs the newest) and defaults.
- `src/preset.rs`, `presets/` — twelve categorized factory sounds through `mxm-preset`.
- `examples/shimmer_preset_audit.rs` — repeatable long-render descriptors for every shipped factory sound.
- `src/editor.rs`, `src/editor/` — app bar, paging, bindings and Space/Motion/Ascent/Loop cards.
- `src/telemetry.rs` — lock-free peak and wet-difference telemetry.
- `control-map.json` — this effect's assignments into the normative collection map.
- `README.md`, `LICENSE` — product documentation and MIT licence.

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

**The two tempo syncs glide through the controls' own smoothers** (2026-09-25,
`plans/plan-tempo-sync-controls.md`). Each is the collection's quarter note beside its knob, resolved
once a block by `follow_tempo`. The engine reads pre-delay as a delay-line position every sample, so
a division arriving at once would click: synced, a control's smoother is aimed at the division
whenever it or the knob moves (the wrapper aims it at the knob on every move), and aimed back at the
knob when sync goes off. A free control is untouched.
`a_synced_pre_delay_glides_to_its_division_and_back_to_the_knob` holds it. `Telemetry::tempo` lets
the knobs read their divisions. Placement, Reverse and
Freeze are discrete DSP transitions; their live-tail behavior belongs to the DSP crate. Activation
and reset first settle the engine to the current parameter topology, including state restored while
the effect was not processing.

**Freeze holds the reverb's experience, not a literal sample buffer.** The wet field remains inside
an energy-preserving version of the same delay network and stops passing through the regenerative
shifter and outer filters, so its harmony neither climbs nor progressively narrows. Delay-tap
modulation parks at the held endpoint because moving fractional reads lose energy; the orthogonal
network itself continues circulating and decorrelating the field. Input remains open: each new signal
receives the selected shift once, irrespective of Placement, and accumulates into the held field.
Freeze armed on silence stays silent and parked until input arrives. There is no output-derived level
normaliser: output-derived gain modulation produces jitter/stutter and destabilizes held energy.
Input remains connected and each new signal is shifted once before joining the held field.

Init is the post-listening-rejection audition setting: Mix 48%, Regen 68%, Shimmer 85%, Shift +12,
Regen placement, Size 78%, Diffusion 90%, Mod rate 0.16 Hz, Mod depth 1.3 ms, 180 Hz / 8 kHz loop
filters and 30 ms Pre-delay. The two modulation defaults are where the previous fixed law sat at the
default Diffusion, so making them controls did not move the sound they were set against. The
shimmer, density and tail are intentionally unmistakable on first insertion; owner acceptance of
this third listening build remains manual.

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

The shared preset system supplies Init. Twelve complete factory presets ship in Pad or Fx categories:
Ascending hall, Input halo, Dual orbit, Fifth heaven, Two octaves, Reverse bloom, Dark ascent, Air
chapel, Small glass, Long bloom, Plain hall and Wide wash. The design table in `src/preset.rs` is the source for regenerating and equality-testing their JSON. It states frequencies, times and intervals
in the parameters' own units rather than opaque normalized values. Every factory pair, and every
factory sound against Init, must differ by at least `0.06` on four normalized parameter axes.

The bank keeps Regen, Diffusion, Size and modulation varied independently. Its families come from
public maker guidance summarized in
`research:effects/shimmer-reverb.md` §§3.2–4.5, not from commercial preset data: +12 semitones,
Regen placement, about 0.9 Diffusion and low modulation depth form the classic hall; Input makes a
fixed harmony; low Diffusion makes an echo-like orbit; Reverse makes the smoother bloom; 7, 12, 19
and 24 semitones cover the canonical fifth, octave, octave-plus-fifth and two-octave intervals.
Modulation, filters, pre-delay, placement and room scale are now deliberately spread across the bank
instead of inheriting Init.

The tracked audit renders each factory sound from a fresh engine with a 30-second wet impulse and
6-second 220 Hz burst: 20,736,000 samples. The current bank spans 13–151 ms onset, 0.3–0.8 s
envelope peaks and 1.3–20.8 s to -60 dB. Its closest descriptor pair is 0.653 against a failing floor of 0.60. Long bloom is uniquely longest; Plain hall has no shifted branch;
Input halo and Two octaves are fixed
harmonies; the three placements, both grain directions and broad motion settings are represented.
The audit prevents another parameter-law change from collapsing the bank; it does not approve
character, which remains an owner-listening gate. No commercial factory value or name was copied.
Factory names contain no reference maker or model identity.

## Control map

Mix fills the existing `fx.reverb` role. Page 12, Effects/Shimmer, maps seven performed controls in
this stable order: Shimmer, Regen, Shift, Placement, Grain/Reverse, Size, Freeze. The eighth slot is
empty. Diffusion, Low Cut, High Cut and Pre-delay are stored shape controls and intentionally remain
unmapped while staying available to host automation.

`control-map.json` uses the data-driven instrument-map schema with this CLAP id; it must be staged
beside both debug and release bundles.

## Editor

The editor uses the normative MXM design system and shared space-derived paging. Its stable Effects
cards are Space, Motion, Ascent and Loop, in that order, each as wide as its controls (its ceiling
is its floor). The opening size is `REFERENCE`, the quarter-4K budget hugged to the four cards
(`the_opening_size_is_the_budget_hugged`); the minimum is one widest card plus gutters (`MINIMUM`,
held by `the_minimum_holds_the_widest_card`). Motion holds modulation and Diffusion because density
and movement interact, and paints the modulation's *Rate* and *Depth* (a host reads *Mod rate*,
*Mod depth*). Freeze is the collection's on/off toggle; switch cells are the parameters' own text. Native 100%, 150% and 200% inspection at
fixed physical size remains a manual §15 gate.

The bloom display reserves its geometry at rest, reads only lossy atomics from the audio thread, uses
`mxm-ui`'s shared telemetry canvas/stroke tokens, publishes an accessibility label, and adds explicit
Off and Held text. Every parameter is bound exactly once, has a tooltip, and brackets
host gestures through `editor/binding.rs`. Effects carry no developer-category CC path.

**Every card is a `mxm_ui::tree`** (`crates/ui/AGENTS.md`, *A card body as data*).
`sections::card` describes each body once — the collection's knob rows (`mxm_ui::tree::knob_row`),
Placement and Grain sharing one cell, Freeze's toggle, the bloom `SPACE_2` further from the knobs
than the card's rhythm — and that description is measured for the card's floor and height and
drawn leaf by leaf through the bindings (`sections::paint`), through `paging::editor::show`.
**Floors are computed**, each tree's narrowest plus the card's chrome, with no usability minimum
declared beside them. The bloom states its own size:
`TALL_PLOT_HEIGHT` tall and at least `BLOOM_MIN_WIDTH` wide, filling its card. `take_shimmer` is
read once, before the frame. `every_card_passes_the_tree_checks_in_every_state` runs the shared
checks (`mxm_plugin_test::tree_checks`) at Init, Off and Held, and the widest bloom.

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

Also run the player control-map tests after changing `control-map.json` or the collection page:

```bash
cargo test -p mxm-player control_map --lib
cargo test -p mxm-player --test t5_control_map
```

Manual gates still required: broader owner listening, native design-system §15 review in both themes,
MXM Player audition, and Bitwig mono/stereo automation/state/tail/reset checks. Linux and macOS are
not verified on the Windows development machine.

# Child DOX Index

No child `AGENTS.md` files.
