# AGENTS.md — crates/mxm-shimmer-dsp/

Parent: [`../../AGENTS.md`](../../AGENTS.md) · The measurements and reasoning behind each rule:
[`NOTES.md`](NOTES.md)

# Purpose

Framework-free, zero-dependency DSP for `mxm-shimmer`: a stereo cascaded-allpass/feedback-delay-
network reverb with a gated swept-delay pitch shifter on the outer Input tap, Regen tap, or both.

Technique evidence: `research:effects/shimmer-reverb.md`. The installed-binary study at
`research:effects/valhalla-shimmer.md` may contribute mechanism and test shapes only under the root
clean-room ruling. Every constant in this crate is ours.

# Ownership

- `src/lib.rs` — outer loop, placement and freeze transitions, filters, pre-delay, Mix/parking,
  activity and tail declarations.
- `src/reverb.rs` — two sixteen-stage input diffusors and the bounded eight-line FDN, including its
  modulated ordinary mode and energy-preserving held mode.
- `src/shifter.rs` — one gated swept-delay voice and deterministic per-period decorrelation.
- `src/delay.rs` — private fractional delay, finite flush, saturator, and
  `follower_coefficient`: the one place a follower's settling time becomes a per-sample number.

No plugin-framework, parameter, preset, MIDI, editor or host type belongs here.

# Local Contracts

## The taps stay outside the core

Input reads the pre-delayed input and crosses the core once. Regen reads the filtered return and
therefore shifts on every lap. Both is a bounded crossfade of those outer taps. Nothing may reach
inside `ReverbCore`; replacing the core must not change what a placement means.

A live placement change resets the shifter newly attached to a node and linearly fades branch weights
to exact endpoints over 25 ms. No buffered history from one tap may be replayed as another tap's
history. Branch weights remain in one gain budget during the fade, and a branch at exact zero stops
running. A placement changed while parked settles immediately: the next excitation starts in the
selected topology rather than fading out the old one.

## The loop may pass unity; the saturator is what bounds it

The full reasoning and measurements: [NOTES.md § The loop](NOTES.md#the-loop-may-pass-unity-the-saturator-is-what-bounds-it).

- **Boundedness does not come from keeping the loop gain below one, and must not be restored that
  way.** Regen's top is deliberately above unity; the rational saturator on every line write makes
  growth settle into a bounded limit cycle, and a DC blocker inside the feedback path makes it ring.
- Sixteen Schroeder allpasses per channel feed the late field; the FDN's Walsh-Hadamard matrix is
  energy-preserving, and every line write and final output sum is explicitly saturated.
- **The coefficient is not the loop gain.** Only measurement decides where the threshold is; do not
  assert the coefficient against one.
- **The FDN mix is always the full orthogonal rotation**; Diffusion never attenuates its modes.
- **Neither tone control belongs inside the loop.** The in-loop filter is a plain 1 Hz DC blocker;
  Low Cut and High Cut stay on the outer return.
- The oscillation threshold falls as Size rises, so a factory preset with high Regen and high Size
  must be rendered: `shimmer_preset_audit` fails if one of them never decays.
- Direct and shifted Regen returns are a crossfade, not an additive pair.
- Every regenerative sum passes through the monotonic rational saturator.
- `worst_case_gain()` for the core and shifter means maximum instantaneous amplitude contribution,
  not energy or a typical measurement.
- Inputs, controls and recursive state must remain finite at 8–192 kHz stress rates; no NaN,
  infinity or subnormal is permitted to persist. Each public DSP seam enforces this itself rather
  than relying on the plugin wrapper.

## Freeze is a stable, accumulating held topology

- Freeze crossfades the core itself into an energy-preserving held loop **before** the outer filters
  and regenerative shifter; delay-tap modulation is parked while held. Silence cannot make the
  held field climb, narrow, decay, grow into a limit cycle or acquire gain-control sidebands.
- New wet input stays connected: each arriving signal receives the selected shift once, regardless
  of Placement, then joins the held field. Freeze on silence arms the empty engine and makes no
  sound. Mix at zero wins over Freeze, empties every line once and parks. Freeze is sustaining only
  while the signal is not quiet.
- **There is no output-level normaliser in Freeze**, and the long-hold regression fails if the held
  network needs one again. The owner accepted this topology on 2026-09-08
  ([NOTES.md § Freeze](NOTES.md#freeze-is-a-stable-accumulating-held-topology)).

## The shifter's artifacts are deliberate; its reset click is not

- The swept delay follows Dattorro, “Effect Design, Part 2”. A Hann gate falls to zero at each period
  reset; the periodic envelope and per-period reseed are deliberate texture, a discontinuity at the
  reset is a defect.
- Forward zero shift is an ordinary delay; Reverse means negative read travel, including at zero
  shift. A live direction edit fades the shifted share to exact silence and back without clearing
  same-node history; a parked edit settles immediately.
- One magnitude drives two **upward** channel voices; there is no descending pair. The sweep, base
  delay and jitter are times converted from the active sample rate; no fixed sample count may
  replace them ([NOTES.md § The shifter](NOTES.md#the-shifters-artifacts-are-deliberate-its-reset-click-is-not)).

## Reset is complete and reproducible

`reset()` clears pre-delay, core, both tap shifters, loop filters, activity, held state and transition
state. Shifter random state returns to its seed. Rendering the same input and controls after reset
must produce the same samples and no old tail may return.

## Realtime and numeric rules

- Rust MSRV is 1.87; keep the crate zero-dependency.
- Allocate only in constructors. `process()` and all methods it calls are allocation-, lock-, log-
  and I/O-free.
- Host block boundaries do not enter the model. Outer recursion is one sample, so output must be
  bit-identical under any partition of the same sample stream.
- Delay storage sizes derive from the validated sample rate. Fractional reads split time into an
  integer age and a fraction **before** wrapping. Reset invalidates delay history in constant time.
  Cutoffs are ordered below 0.45 × sample rate.
- **Every follower is a time, and no fixed per-sample coefficient may replace one**: the core's
  `level` and the engine's `activity` go through `delay::follower_coefficient`, computed in `new()`;
  there is no rate setter ([NOTES.md § Realtime and numeric detail](NOTES.md#realtime-and-numeric-detail)).
- The remaining-tail declaration may overestimate but must not intentionally underestimate.

## The early cluster, and what each control owns

- Three taps out of the diffusor cascade, at stages 6, 9 and 12, join the late field inside the
  output saturator. **Each tap reads its stage's delay line, never the stage's output**
  (`Allpass::process` returns `(output, delayed)`). Tap positions are stage indices, so the times
  ride `diffuser_scale` ([NOTES.md § The early cluster](NOTES.md#the-early-cluster-is-the-only-path-that-is-not-through-the-whole-cascade)).
- **Regen** is the loop gain and the only control that adds gain to the system. **Diffusion** sets
  the input diffusors' allpass gain and nothing in the loop gain. **Size** scales the late field;
  the diffusors follow it weakly and never collapse. **Mod Rate and Mod Depth** are controls, not
  constants: a frequency and a time
  ([NOTES.md § Regen, Diffusion, Size](NOTES.md#regen-owns-decay-diffusion-owns-density-size-owns-the-space)).

## Chosen constants and behavioural reference

- Every constant (listed in [NOTES.md § Chosen constants](NOTES.md#chosen-constants-and-behavioural-reference))
  is chosen for this implementation. They are not values read from a reference product and must not
  be relabelled as such.
- The outer Regeneration scale is calibrated against ascent, not stability. The shifted outer leg may
  exceed unity, bounded by the saturators; the unshifted leg keeps only `UNSHIFTED_OUTER_SHARE`.
  Freeze removes both ordinary outer legs; its one shifted pass belongs only to new input.
- Public-audio measurement supplies behavioural targets only. A 220 Hz burst through Regen must
  migrate toward 880 Hz and 1.76 kHz rather than accumulate at 110 Hz.
- The superseded pre-decay-rework campaign in `plans/plan-mxm-shimmer.md` revision 15 does not
  describe this build and must not be used as acceptance thresholds.

# Work Guidance

- Measure a closed-loop change; compiling a feedback effect proves no character or stability.
- Preserve the distinction between the boundedness oracle and listening. Tests can prove finite,
  bounded, silent, deterministic and partition-invariant behavior; they cannot approve the bloom.
- Do not extract a shared reverb or shifter primitive. Honest per-product implementations are the
  evidence required by the root rule.

# Verification

```bash
cargo test -p mxm-shimmer-dsp
cargo clippy -p mxm-shimmer-dsp --all-targets
cargo check -p mxm-shimmer-dsp --all-targets
```

- `the_park_decision_does_not_depend_on_the_sample_rate` guards the follower times by asserting the
  **decision** at 8, 48 and 192 kHz, with a stimulus built from a frequency in hertz.
- What the tests cover: [NOTES.md § What the tests cover](NOTES.md#what-the-tests-cover).

# Child DOX Index

None.
