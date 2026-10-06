# mxm-shimmer

A stereo pitch-shifted feedback reverb built from the shimmer practice rather than a particular
product. The implementation and all constants are original.

## What it is

The wet signal enters a modulated eight-line reverb. A swept-delay pitch shifter can read the input,
the filtered return, or both. On the return, every lap shifts both independently decorrelated wet
channels upward again. On the input, the shifted layer crosses the room once and stays at
a fixed harmony.

The shifter's gated period and changing read start are deliberate texture. The reset itself is gated
to silence so it does not click. Low Cut and High Cut sit inside the outer loop, where they prevent
low-end mud and let every pass change colour.

## Controls

- **Mix** crossfades dry and wet. At zero the effect empties once and parks.
- **Regen** sets ordinary outer-loop recirculation.
- **Shimmer** sets shifted level separately from tail length.
- **Shift** selects 0–24 upward semitones for both wet channels.
- **Placement** chooses `Input`, `Regen`, or `Both` without reaching inside the reverb core.
- **Grain** chooses forward or reverse swept-delay travel.
- **Freeze** holds a stable wet field while later input receives one shift and accumulates into it.
- **Size** scales the delay network; **Diffusion** moves from distinct returns to a dense wash.
- **Low Cut**, **High Cut**, and **Pre-delay** shape the space and every subsequent loop pass.

Placement and Freeze changes are faded under a live tail. Freeze bypasses repeated shifting and
outer filtering, and preserves the field inside the reverb network without output-derived gain.
Delay-tap modulation pauses while held so fractional reads cannot drain the field; the network keeps
circulating it. Arming Freeze on silence creates no sound but leaves it ready for later input. Host
reset removes every delay, filter, random, transition, and held state.

## Presets

Twelve factory presets cover fixed input harmonies, rising and blended Input/Regen textures, reverse
sweeps, dark and airy spaces, 7/12/19/24-semitone shifts, a plain unshifted hall, and long washes.
`Init` is supplied by the shared preset system.

## Building

```bash
cargo xtask bundle mxm-shimmer --release
```

The bundle is written to `target/bundled/mxm-shimmer.clap` with its factory presets and control map
beside it.

## Licence

MIT — see [`LICENSE`](LICENSE). No third-party implementation code or constants are used.
