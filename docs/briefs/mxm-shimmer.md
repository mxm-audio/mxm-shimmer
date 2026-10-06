# mxm-shimmer — UI design brief

Required by `MXM_DESIGN_SYSTEM.md` §14. This effect is an original pitch-shifted feedback reverb,
not the interface or constants of a reference product. Technique evidence is
`research:effects/shimmer-reverb.md`; the installed-emulation study is
`research:effects/valhalla-shimmer.md`, under the root clean-room ruling.

**Plugin:** `mxm-shimmer`; CLAP id `dk.mxm.mxm-shimmer`. Stereo wet topology, with mono-to-stereo
and stereo-to-stereo layouts.

## 1. Primary sound-design task

**Choose whether the shifted layer is a stable harmony, a pitch that climbs through successive
returns, or both, then shape the space carrying it.** The distinction must be visible without
requiring a user to infer feedback topology from one generic Shimmer knob.

## 2. Controls reached for most

1. **Shimmer** — shifted level, deliberately independent of tail length.
2. **Regen** — ordinary outer-loop recirculation and decay.
3. **Shift** — 0–24 semitones; both wet channels rise through independently decorrelated voices.
4. **Placement** — Input, Regen, or Both.
5. **Mix** — dry/wet crossfade and the Off control at zero.

Freeze and Grain are performed switches beside those controls rather than hidden setup.

## 3. Signal flow that must be visible

```text
input -> pre-delay --+-------------------------> sum -> stereo reverb -> wet -> Mix
                     +-> shifted Input tap -----^          |
                                                          v
                     sum <--- shifted/direct Regen tap <- HPF -> LPF
input ---------------------------------------------------------------> dry -> Mix
```

The interface must communicate three facts: Input placement crosses the room once; Regen placement
returns to the outer summing node and therefore shifts on successive laps; both filters are inside
that loop. No control or drawing implies access to an internal reverb tap.

## 4. Play view

There is no Play view and no view bar. This is an effect with three Effects cards, following the
standalone effects already in the collection. Space-derived paging may split the cards when they no
longer fit, without changing controller pages.

## 5. Advanced controls and disclosure

No control is hidden. The cards are the disclosure:

| Card | Controls | Job |
|---|---|---|
| **Space** | Pre-delay, Size, Diffusion | Establish onset, scale, and density before choosing the shifted path |
| **Ascent** | Shift, Shimmer, Placement, Grain | Define the interval, shifted level, tap, and sweep direction |
| **Loop** | Regen, Mix, Low Cut, High Cut, Freeze | Control recirculation, output balance, loop colour, and held topology |

Diffusion, Low Cut, High Cut and Pre-delay remain off the performance control-map page: they are the
stored shape of the space. Mix reuses `fx.reverb`; seven performed roles occupy the Shimmer page,
with its eighth slot intentionally empty.

## 6. Categories, cards, and grouping

All three stable cards are in **Effects**, ordered by signal flow: Space, Ascent, Loop. Ascent stays
whole while space permits because separating Placement from Shimmer would hide the product's primary
decision. Loop keeps Freeze beside Regen because freeze is a distinct held topology, not maximum
feedback. The shared paging renderer derives one to three visual pages from available space.

## 7. Identity accent

Use the collection accent unchanged in both themes. Effects are distinguished in a chain by their
names and signal roles, not by adding an untested hue family. Shared semantic tokens provide the
measured light/dark contrast inherited by the app bar, controls, focus states and card text; no
meaning depends on accent hue alone.

## 8. Live visualization

One reserved `TALL_PLOT_HEIGHT` display in Space shows widening reverberant arcs and five interval
marks, using the shared telemetry canvas and stroke hierarchy. Arc weight comes from lock-free audio
telemetry, while Size and Shift determine geometry. It teaches bloom and
successive pitch travel without pretending to be a spectrum analyser. Text adds the two important
states: **Off** when Mix is zero and **Held** while Freeze is active. The app bar retains the shared
peak/clip meter.

## 9. What is removed from source layouts

There is no source hardware panel to preserve. The desk-patch practice contributes the outer-loop
idea, and product references contribute evidence about the technique, not a face to reproduce.
Consequently this panel removes product-specific mode lists, proprietary size laws, fixed interval
menus, decorative hardware metaphors and any reference layout. It exposes the topology in the MXM
card language instead.

The periodic swept-delay envelope and deterministic per-period reseed are deliberate audible
character. A reset discontinuity is not: the window falls to zero at the boundary. The disputed
burst gate is absent from v1. Both interval voices rise: the first audition's opposite-sign pair
left descending energy in the cross-coupled stereo tail and was rejected with the slap-like core.

## 10. Fit, reflow, and zoom

**Resizable: the editor's `REFERENCE` and `MINIMUM`, derived and held by its tests.** Card floors
are computed from each card's tree, and each card is as wide as its floor.
`the_minimum_holds_the_widest_card` pins the minimum width against the widest indivisible card, and
`the_opening_size_is_the_budget_hugged` holds the opening size to the 1920 × 1080 quarter-4K budget
hugged to the cards at 1×. At narrower or
shorter sizes the shared renderer pages the cards and scrolls an indivisible overflow rather than
clipping controls.

The native-window checks at 100%, 150% and 200% zoom in one fixed physical quarter-4K window, in both
themes and at the development machine's DPI scale, remain part of §15 visual QA; headless geometry
tests do not claim those checks passed.

## Sign-off

- [x] §14's ten questions are answered.
- [x] Every parameter appears exactly once and has a one-sentence tooltip.
- [x] The primary topology is visible in card order and the reserved display.
- [x] Off and Held have text channels rather than hue alone.
- [x] The tested logical opening and width floor fit the quarter-4K dimensions at 1×.
- [ ] Dark/light native-window inspection and measured contrast review.
- [ ] Fixed-window 100%, 150%, and 200% zoom inspection at recorded DPI.
- [ ] Owner listening and visual sign-off.
