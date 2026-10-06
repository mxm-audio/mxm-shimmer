# AGENTS.md — crates/mxm-shimmer-dsp/

Parent: [`../../AGENTS.md`](../../AGENTS.md)

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

**Boundedness no longer comes from keeping the loop gain below one, and must not be restored that
way.** Regen is the FDN's loop gain and its top is deliberately above unity, because a loop that
only approaches unity cannot self-oscillate: with the rational saturator on every line write,
`bounded(g·x) < x` for every `x > 0` whenever `g <= 1`, so no sustain is reachable at any setting.
Above unity the same saturator makes `bounded(g·x) = x` a stable equilibrium at `x = (g − 1) / g`,
so growth settles into a bounded limit cycle rather than running away. A DC blocker inside the
feedback path decides that the limit cycle rings rather than drifting to zero frequency.

- Sixteen linear Schroeder allpasses per channel redistribute the input before the late field. The
  FDN's Walsh-Hadamard matrix is energy-preserving at `1/sqrt(8)`; every line write and final output
  sum is explicitly saturated. Internal feedback is `0.80..=1.05` over the **Regen** domain.
- **The coefficient is not the loop gain.** The network loses about 1.4% a lap on its own —
  fractional interpolation on every line, more of it once Mod Depth sweeps the reads — so the
  coefficient passes one well before the round trip does. Only measurement decides where the
  threshold is; do not assert the coefficient against one.
- **The FDN mix is always the full orthogonal rotation.** It used to be
  `reads + diffusion * (diffuse - reads)`, whose eigenvalues are `1` and `1 - 2·diffusion`: that
  attenuated half the network's modes by an amount Diffusion set, making Diffusion a second decay
  control and a high setting the only route to a long tail. Measured before the fix: at Diffusion
  0.99 a preset would not decay inside thirty seconds at any Regen down to 0.56.
- **Neither tone control belongs inside the loop.** Both were tried and measured out. A one-pole
  low-pass at the 8 kHz High Cut default costs more per lap than any sensible loop gain returns, so
  the network could not sustain at all; running the high-pass at the 180 Hz Low Cut default
  collapsed decays that should run for tens of seconds. The in-loop filter is a plain 1 Hz DC
  blocker; Low Cut and High Cut stay on the outer return.
- **The oscillation threshold moves with Size, and that is not a defect.** Loss is per lap, so a
  large space with fewer, longer laps reaches unity at a lower Regen — about 0.80 at Size 0.98
  against about 0.90 at Size 0.20. A bigger room ringing longer for the same absorption is what a
  room does. It does mean a factory preset cannot be given a high Regen and a high Size without
  being rendered: `shimmer_preset_audit` fails if one of them never decays.
- Direct and shifted Regen returns are a crossfade, not an additive pair.
- Every regenerative sum passes through the monotonic rational saturator.
- `worst_case_gain()` for the core and shifter means maximum instantaneous amplitude contribution,
  not energy or a typical measurement.
- Inputs, controls and recursive state must remain finite at 8–192 kHz stress rates; no NaN,
  infinity or subnormal is permitted to persist. Each public DSP seam enforces this itself rather
  than relying on the plugin wrapper.

## Freeze is a stable, accumulating held topology

Freeze crossfades the core itself from its ordinary lossy, modulated loop into an energy-preserving
held loop **before** the outer filters and regenerative shifter. At the held endpoint, each delay uses
its rounded integer base length, the Walsh-Hadamard feedback rotation stays orthogonal, the DC
blocker and ordinary gain are bypassed, and line writes are linear until actual accumulation reaches
the hard clamp. Delay-tap modulation is parked while held: moving fractional reads necessarily lose,
duplicate or skip energy in this topology. The network still circulates and decorrelates the field in
time, but silence cannot make its harmony climb, narrow, decay, grow into a limit cycle, or acquire
gain-control sidebands.

New wet excitation stays connected: each arriving signal receives the selected shift once,
regardless of ordinary Placement, and then joins the held field. Engaging Freeze on silence creates
no sound but arms the empty engine for later input. Disengaging crossfades the same delay-bank state
back to ordinary modulation and decay. Mix at zero wins over Freeze, empties every line once and
parks. Freeze is sustaining only while the signal is not quiet.

**There is no output-level normaliser in Freeze.** The first accumulating build recirculated the
core's output through its input and corrected the resulting loss from a rectified-output envelope.
At roughly 1–4 ms it ring-modulated the field; slowing it to two seconds only made the field decay
before compensation rose and made the late stutter more pronounced. Both failed by audition. The
long-hold regression now compares five- and thirty-second energy and density and fails if the held
network needs that chasing gain topology again. The owner accepted this energy-preserving topology
in Player audition as “absolutely perfect” on 2026-09-08.

## The shifter's artifacts are deliberate; its reset click is not

The rotating-head/Doppler swept delay follows Jon Dattorro, “Effect Design, Part 2”. A Hann gate
falls to zero at each period reset. The periodic envelope and deterministic per-period delay reseed
are deliberate texture established by `research:effects/shimmer-reverb.md`; a discontinuity at the
reset is a defect and remains gated out. Forward zero shift is an ordinary delay. Reverse means
negative read travel, including at zero shift. A live Forward/Reverse edit fades the shifted share
to exact silence, changes direction without clearing same-node history, and fades back in; a parked
edit settles immediately.

One magnitude drives two upward channel voices. This is deliberately a unidirectional ascent: the
first listening build's opposite-sign pair retained a conspicuous descending layer once the stereo
network cross-coupled it, rather than reading as a climbing shimmer. The sweep, base delay and jitter
are times and are converted from the active sample rate; no fixed sample count may replace them.

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
- Delay storage sizes derive from the validated sample rate. Fractional reads split the requested
  time into an integer sample age and interpolation fraction before wrapping: with a roughly
  50k-sample line, wrapping the whole position in `f32` can round a point just below the buffer
  length up to the length and panic at the seam. Reset invalidates delay history in constant time;
  it never clears a high-rate allocation on the audio thread. Cutoffs are ordered below 0.45 ×
  sample rate even when host parameter ranges exceed Nyquist at the 8 kHz stress rate.
- **Every follower is a time, and no fixed per-sample coefficient may replace one.** The two level
  followers that decide quiet — the core's `level` and the engine's `activity` — are built from
  `LEVEL_ATTACK_S`/`LEVEL_RELEASE_S` and `ACTIVITY_ATTACK_S`/`ACTIVITY_RELEASE_S` through
  `delay::follower_coefficient`, which is the one place a follower's speed becomes a number. They are
  *peak* followers, fast up and slow down, so each is a lagging upper bound on the recent envelope
  and cannot report quiet before the signal is: the failure a fixed coefficient causes is parking
  **late** at a low rate, never a truncated tail. The rate is fixed at construction and the plugin
  rebuilds the engine in `prepare`, so both are computed in `new()` and there is no rate setter.
- The remaining-tail declaration may overestimate but must not intentionally underestimate.

## The early cluster is the only path that is not through the whole cascade

Three taps are summed out of the diffusor cascade and joined to the late field inside the output
saturator, at stages 6, 9 and 12 with falling weights and an overall `EARLY_LEVEL`.

**Each tap reads its stage's delay line, never the stage's output.** A Schroeder allpass carries
`-gain * input` instantaneously, so a tap on the output is a sample-zero spike, not an early
reflection: the first attempt measured every Size peaking at 0 ms with the early cluster at full
level. `Allpass::process` returns `(output, delayed)` for exactly this reason.

The cluster exists because the cascade is long — 337 ms of allpass unscaled, 431 ms at the default
Size — and it was the *only* path to the output, while the feed-through that might have arrived
sooner is attenuated by `(0.75 * Diffusion)^16`, about -57 dB at the default Diffusion. The effect
therefore had no early energy at all and read as though it had a pre-delay it did not have: at
Size 0.78 with Pre-delay at zero, nothing reached -20 dB of peak until **118 ms**. With the cluster
that is **19 ms**, while the bloom still peaks at 561 ms and the early cluster stays about 0.20-0.29
of the overall peak. Reported by the owner as a long pre-delay at Pre-delay zero.

Tap positions are stage indices, so the times ride `diffuser_scale` and a small space keeps a
proportionally quick onset. Three rather than one: a single tap is a discrete echo, which is the
slap the diffusors exist to prevent, and the left and right trains already differ so the cluster is
decorrelated without extra work.

## Regen owns decay; Diffusion owns density; Size owns the space

These three were entangled and are now separated, because a listening comparison found the decay
short, the feedback incapable of self-oscillation, and Size audibly *undiffused* at the bottom.

- **Regen** is the loop gain and the only control that adds gain to the system. Measured at Size
  0.70: 1.9 s at the bottom, about 8 s at the 0.68 default, 10 s at 0.80, 18 s at 0.90, and
  self-oscillation in the top few percent.
- **Diffusion** sets the input diffusors' allpass gain and nothing in the loop gain.
- **Size** scales the late field only. The diffusors follow it weakly and never collapse:
  `0.85 + 0.55 × Size`, against the old `0.25 + 1.65 × Size` which took them to a quarter of their
  length at Size 0 and made a small setting arrive as countable taps rather than a small diffuse
  room.
- **Mod Rate and Mod Depth** are controls, not constants. Depth is a time and rate a frequency, so
  both survive a sample-rate change; the per-line stagger is a ratio set so the rate scales all
  eight lines together. Depth costs decay — a swept interpolating read is lossy — and that is a
  real trade the player now makes rather than one Diffusion made for them.

## Chosen constants and behavioural reference

The sixteen diffuser times per channel, `0.75 × Diffusion` allpass gain, `0.85 + 0.55 × Size`
diffuser scale, the three early-tap stages/weights and `EARLY_LEVEL`, eight FDN ratios, modulation stagger and phases, 12–90 ms FDN size law, the
`0.80 + 0.183·r^0.4 + 0.067·r^16` loop-gain law, 1 Hz DC-blocker corner, injection signs and level,
far-corner diffuser state ceiling, output level, `0.15` outer-Regeneration scale, `0.25` unshifted
outer share, interval-dependent `1..=10` shifted-level compensation, 70 ms shifter sweep, 12 ms base
delay, 6 ms jitter, 25 ms transition, 0.75 s quiet hold and the four level-follower times (0.4 ms
attack for both; 42 ms release on the core's level, 104 ms on the engine's activity) are chosen for
this implementation. They are not values read from a reference product and must not be relabelled as
such.

**The outer Regeneration scale is calibrated against ascent, not stability.** At default settings,
the ratio of energy in the two octaves above a 220 Hz burst to energy at the fundamental is:
`0.06` → 0.26, `0.09` → 0.74, `0.12` → 1.6, `0.15` → 3.1, `0.18` → 5.2. The chosen `0.15` climbs
convincingly while the default still decays inside thirty seconds; `0.18` barely does.

**The two legs of the outer branch are not on the same gain.** The shifted leg is deliberately
allowed above unity — a transposed copy has to outgrow the untransposed tail underneath it or the
tail buries it — and is bounded by the saturators rather than by being under one. The unshifted leg
keeps only `UNSHIFTED_OUTER_SHARE` of the ordinary gain, because the core already recirculates
unshifted energy and an outer path carrying it again is surplus loop gain; at Shimmer zero that
surplus was what turned a moderate Regen into a growing loop. Freeze removes both ordinary outer
legs and preserves energy inside the core; its one shifted pass belongs only to new input.

Public-audio black-box measurement at 48 kHz supplies behavioral targets only: the chosen
long-room reference becomes dense, peaks near 0.5 s and remains continuously active above `1e-6`
for roughly fifteen seconds. At `Size = 0.70`, `Diffusion = 0.90`, this core becomes dense, peaks
near 0.5 s and remains above the same threshold for roughly nine seconds; the Long bloom factory
topology reaches about −60 dB after 17.8 seconds. A 220 Hz burst through Regen must migrate toward
880 Hz and 1.76 kHz rather than accumulate at 110 Hz.

The current `shimmer_preset_audit` envelope is 1.3–20.8 s to −60 dB, 13–151 ms onset and 0.3–0.8 s
peaks,
using interval-aware tone and first-20-ms density descriptors. The superseded pre-decay-rework
campaign and its correlation figures remain preserved in `plans/plan-mxm-shimmer.md` revision 15;
they do not describe this build and must not be used as acceptance thresholds.

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

The tests cover silence/reset, constant-time delay invalidation, the high-rate fractional-delay wrap
seam, zero-shift delay, octave translation, gated reset steps, direct public-seam recovery after
non-finite input, the core's control-domain bound, a dense delayed peak and five-second bloom,
integrated upward spectral migration, uncapped long-tail declaration, late sub-unity-Regeneration
decay, that Regen owns decay and its top sustains while a low setting does not, that a small Size
stays as diffuse as a large one,
full-engine adversarial bounds, exact placement/freeze endpoints, silent-state settling, click-safe
direction changes, cutoff ordering at stress rates,
Freeze sustain, thirty-second energy/density preservation without output gain control, silent
arming, later-input accumulation, one-pass frozen-input shifting without continued ascent, Mix-zero
parking and host-block partition invariance.

`the_park_decision_does_not_depend_on_the_sample_rate` is the one that guards the follower times, and
it asserts the **decision** rather than a coefficient: at 8 kHz, 48 kHz and 192 kHz a decayed field
must park inside a stated number of seconds and a frozen one must never park. Its stimulus is built
from a frequency in hertz, not a per-sample increment — the older tests here use
`(n as f32 * 0.03).sin()`, which at three rates is three different signals and would compare nothing.
It was verified red against the fixed coefficients first: 11.24 s to park at 8 kHz against 6.59 s at
48 kHz and 6.54 s at 192 kHz; it is 6.586 s at all three now.

# Child DOX Index
