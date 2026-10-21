# Output delay: does GetPosition include it?

Every device mode needs to know when each output really makes its sound (section 4.14 of
[the design](../design/listen-together.md)). The player knows what Windows tells it:
`IAudioClock::GetPosition` with its performance-counter stamp says which frame is at the
speaker. Whether that includes a Bluetooth headset's codec and buffer (34 to 241 ms measured
by RTINGS) is not documented, so it is measured, once per kind of output, before every device
mode ships. Work package 19 (the delay per output) builds on the answer.

The tool is `output_delay` (in `crates/player/examples`, built with the rest of the kit, see
[listen-together-live.md](listen-together-live.md)). It plays a few short chirps, quietly, and
compares three times for each one:

- **reported**: when `GetPosition` says it reaches the speaker (the app's own WASAPI code);
- **loopback**: when the output's loopback has it, which is after Windows' mixer and the
  output's sound effects, before the driver and anything after it (a Bluetooth link);
- **heard**: when a microphone hears it (optional, and the only way to see a Bluetooth
  delay directly).

## The owner's run

You need the Bluetooth headphones and the Bluetooth speaker paired, a wired output (the
laptop's speakers or its headphone jack), and a microphone that stays the same for every run
(the laptop's built-in one is fine; not a phone used as a microphone over the network, which
adds its own delay). Nothing else may play during a run; the tool refuses if it hears
something.

1. List the outputs and microphones, and note the ids:

   ```text
   output_delay devices
   ```

2. **The wired baseline.** Hold the microphone close to the wired speaker (or put the wired
   headphones' cup over the laptop's microphone). It measures the microphone's own delay and
   the sound's flight, which `compare` then takes off the Bluetooth runs.

   ```text
   output_delay measure --device <wired id> --mic <microphone id> --level-db -30 --label wired --save wired.json
   ```

   It must say `microphone: 5 of 5 chirps found`. If it finds fewer, hold the microphone
   closer or raise the level (`--level-db -20`); the chirps are 25 ms long, every half second.

3. **The headphones.** Put one ear cup over the same microphone:

   ```text
   output_delay measure --device <headphones id> --mic <microphone id> --level-db -30 --label headphones --save headphones.json
   output_delay compare wired.json headphones.json
   ```

4. **The speaker**, the microphone a hand's width from it:

   ```text
   output_delay measure --device <speaker id> --mic <microphone id> --level-db -30 --label speaker --save speaker.json
   output_delay compare wired.json speaker.json
   ```

5. Do steps 3 and 4 twice more each (Bluetooth delays vary by connection; the spread
   matters), and once after switching the output away and back (Mozilla saw delays change on
   switches).

`compare` ends with the verdict:

- `GetPosition INCLUDES this output's delay (within 15 ms)`: every device mode can trust the
  device's report on this output; no extra delay is needed.
- `GetPosition does NOT include about N ms`: the output needs an extra delay of about N ms,
  set by "Line up by ear" or the suggestion (section 4.14). Work package 19 must not let
  every device mode play on it until set.
- `unclear`: redo with the microphone closer.

Without a microphone the tool still compares `GetPosition` against the loopback: if
`GetPosition` puts the Bluetooth output's speaker 80 ms or more after the mixer, it counts at
least that much of the Bluetooth delay; if no more than the wired output, it most likely
leaves the Bluetooth delay out. Only the microphone run proves it.

Write the results here:

| Date | Output | Codec (if shown) | Heard after GetPosition, beyond wired | Spread | Verdict |
|---|---|---|---|---|---|
| | headphones | | | | |
| | speaker | | | | |

## Measured on this computer (2026-10-21, while the kit was built)

No Bluetooth output was connected, so only the wired leg could be measured, and without a
microphone (only test sounds the tool makes, quietly, for a few seconds, system volume as it
was).

| Output | Mode | Buffer | GetPosition: speaker after the buffer is handed over | Loopback against GetPosition | Spread |
|---|---|---|---|---|---|
| Speakers (Realtek(R) Audio), laptop | shared, 48 kHz | 50 ms | 63.0 ms | the loopback has the chirps 104.2, 104.8 and 104.1 ms **after** GetPosition's speaker time (three runs) | 0.1 to 1.6 ms |

What it shows:

- `GetPosition` is steady (the positions fit a line within 0.03 ms) and puts the speaker 63 ms
  after the buffer is handed to the device: about the 50 ms buffer ahead of it, plus some
  13 ms of engine and device.
- But the output's loopback, which is taken after the output's sound effects, has each chirp
  about 104 ms later than that. These speakers run two vendor sound-effect modules (Nahimic,
  listed as `AVOLUTE_NH4APO`, and `ELEVOCAPO` in the endpoint's effect list), and `live_check
  tone` finds the sound changed on the way (a residual of -13 dB against the test music, where
  a clean output gives under -60 dB). So the effects most likely add about 100 ms that
  `GetPosition` does not count: on this output, `GetPosition` would place the sound about
  100 ms early.
- A microphone run (step 2 above, on these speakers) settles it. If it confirms, then for
  every device mode an output with sound effects needs a measured delay as much as a
  Bluetooth one does, and the live matrix runs with the effects off.

The render loop on the same output (`deadline_probe probe --seconds 10`, shared mode, no
stress set, other builds running on the computer): 1015 periods, never dry, never late; the
margin's p99.99 was 99% of the period, and the device always still held 40 ms.

The loopback chain here is clean: `live_check tone` found nothing on the clean test music, and
caught an injected dropped frame (50 dB over the floor) and an injected 5 ms gap (77 dB) with
detector 3. Detector 4 did not see the 5 ms gap through these speakers' effects, which smear
it with their own filter's ringing; on an output without effects it does (the self-test).
