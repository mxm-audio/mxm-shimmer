# NOTES.md — plugins/mxm-shimmer/

The detail behind this folder's AGENTS.md: history, measurements, rationale and worked examples.
AGENTS.md is the contract; this file is the reference it links to.

## The two tempo syncs

**The two tempo syncs glide through the controls' own smoothers** (2026-09-25,
`plans/plan-tempo-sync-controls.md`). Each is the collection's quarter note beside its knob, resolved
once a block by `follow_tempo`. The engine reads pre-delay as a delay-line position every sample, so
a division arriving at once would click: synced, a control's smoother is aimed at the division
whenever it or the knob moves (the wrapper aims it at the knob on every move), and aimed back at the
knob when sync goes off. A free control is untouched.
`a_synced_pre_delay_glides_to_its_division_and_back_to_the_knob` holds it. `Telemetry::tempo` lets
the knobs read their divisions. Placement, Reverse and Freeze are discrete DSP transitions; their
live-tail behavior belongs to the DSP crate. Activation and reset first settle the engine to the
current parameter topology, including state restored while the effect was not processing.

## Freeze holds the reverb's experience

**Freeze holds the reverb's experience, not a literal sample buffer.** The wet field remains inside
an energy-preserving version of the same delay network and stops passing through the regenerative
shifter and outer filters, so its harmony neither climbs nor progressively narrows. Delay-tap
modulation parks at the held endpoint because moving fractional reads lose energy; the orthogonal
network itself continues circulating and decorrelating the field. Input remains open: each new
signal receives the selected shift once, irrespective of Placement, and accumulates into the held
field. Freeze armed on silence stays silent and parked until input arrives. There is no
output-derived level normaliser: output-derived gain modulation produces jitter/stutter and
destabilizes held energy. Input remains connected and each new signal is shifted once before joining
the held field.

## Init

Init is the post-listening-rejection audition setting: Mix 48%, Regen 68%, Shimmer 85%, Shift +12,
Regen placement, Size 78%, Diffusion 90%, Mod rate 0.16 Hz, Mod depth 1.3 ms, 180 Hz / 8 kHz loop
filters and 30 ms Pre-delay. The two modulation defaults are where the previous fixed law sat at the
default Diffusion, so making them controls did not move the sound they were set against. The
shimmer, density and tail are intentionally unmistakable on first insertion; owner acceptance of
this third listening build remains manual.

## Presets

The shared preset system supplies Init. Twelve complete factory presets ship in Pad or Fx
categories: Ascending hall, Input halo, Dual orbit, Fifth heaven, Two octaves, Reverse bloom, Dark
ascent, Air chapel, Small glass, Long bloom, Plain hall and Wide wash. The design table in
`src/preset.rs` is the source for regenerating and equality-testing their JSON. It states
frequencies, times and intervals in the parameters' own units rather than opaque normalized values.
Every factory pair, and every factory sound against Init, must differ by at least `0.06` on four
normalized parameter axes.

The bank keeps Regen, Diffusion, Size and modulation varied independently. Its families come from
public maker guidance summarized in `research:effects/shimmer-reverb.md` §§3.2–4.5, not from
commercial preset data: +12 semitones, Regen placement, about 0.9 Diffusion and low modulation depth
form the classic hall; Input makes a fixed harmony; low Diffusion makes an echo-like orbit; Reverse
makes the smoother bloom; 7, 12, 19 and 24 semitones cover the canonical fifth, octave,
octave-plus-fifth and two-octave intervals. Modulation, filters, pre-delay, placement and room scale
are now deliberately spread across the bank instead of inheriting Init.

The tracked audit renders each factory sound from a fresh engine with a 30-second wet impulse and
6-second 220 Hz burst: 20,736,000 samples. The current bank spans 13–151 ms onset, 0.3–0.8 s
envelope peaks and 1.3–20.8 s to -60 dB. Its closest descriptor pair is 0.653 against a failing
floor of 0.60. Long bloom is uniquely longest; Plain hall has no shifted branch; Input halo and Two
octaves are fixed harmonies; the three placements, both grain directions and broad motion settings
are represented. The audit prevents another parameter-law change from collapsing the bank; it does
not approve character, which remains an owner-listening gate. No commercial factory value or name
was copied. Factory names contain no reference maker or model identity.

## Editor

The editor uses the normative MXM design system and shared space-derived paging. Its stable Effects
cards are Space, Motion, Ascent and Loop, in that order, each as wide as its controls (its ceiling is
its floor). The opening size is `REFERENCE`, the quarter-4K budget hugged to the four cards
(`the_opening_size_is_the_budget_hugged`); the minimum is one widest card plus gutters (`MINIMUM`,
held by `the_minimum_holds_the_widest_card`). Motion holds modulation and Diffusion because density
and movement interact, and paints the modulation's *Rate* and *Depth* (a host reads *Mod rate*, *Mod
depth*). Freeze is the collection's on/off toggle; switch cells are the parameters' own text. Native
100%, 150% and 200% inspection at fixed physical size remains a manual §15 gate.

The bloom display reserves its geometry at rest, reads only lossy atomics from the audio thread,
uses `mxm-ui`'s shared telemetry canvas/stroke tokens, publishes an accessibility label, and adds
explicit Off and Held text. Every parameter is bound exactly once, has a tooltip, and brackets host
gestures through `editor/binding.rs`. Effects carry no developer-category CC path.

**Every card is a `mxm_ui::tree`** (`crates/ui/AGENTS.md`, *A card body as data*; in mxm-kit, where
it is now [`crates/ui/NOTES.md`](https://github.com/mxm-audio/mxm-kit/blob/main/crates/ui/NOTES.md#a-card-body-as-data--tree)).
`sections::card` describes each body once — the collection's knob rows (`mxm_ui::tree::knob_row`),
Placement and Grain sharing one cell, Freeze's toggle, the bloom `SPACE_2` further from the knobs
than the card's rhythm — and that description is measured for the card's floor and height and drawn
leaf by leaf through the bindings (`sections::paint`), through `paging::editor::show`. **Floors are
computed**, each tree's narrowest plus the card's chrome, with no usability minimum declared beside
them. The bloom states its own size: `TALL_PLOT_HEIGHT` tall and at least `BLOOM_MIN_WIDTH` wide,
filling its card. `take_shimmer` is read once, before the frame.
`every_card_passes_the_tree_checks_in_every_state` runs the shared checks
(`mxm_plugin_test::tree_checks`) at Init, Off and Held, and the widest bloom.
