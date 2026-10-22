# Output devices: every kind, as well as the best players

How engine 2 chooses, opens, follows and leaves sound outputs on Windows, and what the window
shows of it. It builds on the WASAPI output (section 4.8 of [listen-together.md](listen-together.md),
WP1) and engine 2's control thread (its WP4 notes), and it gives Listen together's delay per
output (4.14, WP19) its store. Engine 1 is unchanged (it still opens the endpoint it is given).

## 1. What Windows gives us (read on the owner's laptop)

Every render endpoint has a property store. Read on the owner's laptop (registry copy of the
stores, 2026-10-22), the properties that tell outputs apart:

| Endpoint | `DeviceDesc` | `DeviceInterface_FriendlyName` | `EnumeratorName` | `FormFactor` | `ContainerId` |
|---|---|---|---|---|---|
| AirPods Pro, stereo | Headphones | ...AirPods Pro | BTHENUM | 3 Headphones | same as below |
| AirPods Pro, calls | Headset | ...AirPods Pro Hands-Free | BTHHFENUM | 5 Headset | same as above |
| FiiO EH11 | Headphones / Headset | FIIO EH11 / FIIO EH11 Hands-Free | BTHENUM / BTHHFENUM | 3 / 5 | shared |
| soundcore Motion X600 | Speakers / Headset Earphone | ... / ... Hands-Free | BTHENUM / BTHHFENUM | 1 / 5 | shared |
| SNOWSKY DISC | Headphones | SNOWSKY DISC | USB | 3 | its own |
| Laptop speakers | Speakers | Realtek(R) Audio | INTELAUDIO | 1 | the computer's |
| Headphone jack | Headphones | Realtek(R) Audio | INTELAUDIO | 3 | |
| NVIDIA HDMI | XG32UCWG, M32UC, NVIDIA Output | NVIDIA High Definition Audio | HDAUDIO | 9 Display | one per sink |
| Nahimic | Speakers | Nahimic mirroring device, Nahimic VAD, Nahimic Easy Surround device | SWD | 1 | none |
| DroidCam | Speakers | DroidCam Audio | ROOT | 1 | none |

So the classification needs no names and no guesses: the enumerator says the bus, the form factor
what it is, the container id which endpoints are one device.

## 2. Classification (`crates/player/src/devices.rs`, platform neutral)

An `Endpoint` is what the platform read: id, description, adapter name, enumerator, form factor,
container id, state (active, disabled, not present, unplugged). `classify` gives it a kind and a
form, in this order:

| Kind | Rule |
|---|---|
| Bluetooth | enumerator starts with `BTH` (classic, hands-free, LE) |
| USB | enumerator starts with `USB` |
| Virtual | enumerator `SWD` or `ROOT` (software devices: Nahimic, NVIDIA's virtual devices, DroidCam, cables) |
| HDMI | form factor 9 (a display: HDMI or DisplayPort) |
| Wired | anything else with an enumerator (the laptop's speakers and jack, a sound card, S/PDIF) |
| Other | nothing known |

**Hands-free.** A Bluetooth endpoint is the hands-free (HFP) one when its enumerator is `BTHHFENUM`
or its form factor is Headset or Handset; otherwise it is the stereo (A2DP) one. Twins are the
Bluetooth endpoints with the same container id (by the adapter name with " Hands-Free" taken off
when a container is missing). The computer's own container (`{00000000-0000-0000-ffff-ffffffffffff}`)
never pairs anything.

**What the picker lists** (`entries`): one entry per device, active endpoints only (and the one you
chose, as not connected, when it is away). A Bluetooth device is one entry, its id the stereo
endpoint's, with the hands-free twin's id kept beside it (hidden); a device that has only its
hands-free endpoint is an entry marked hands-free only. Clear names: a Bluetooth or USB device by
its own name ("AirPods Pro", "SNOWSKY DISC"), a wired output by what it is ("Speakers",
"Headphones") with the adapter as its detail, a display by its own name ("XG32UCWG"), a virtual
device by its adapter name ("Nahimic mirroring device"). Two entries with one name in a group get
Windows' full name.

## 3. Which endpoint plays (`resolve`)

The choice stays what it is today: `OutputChoice::Device { id, exclusive }`, `id: None` following
Windows' default (the option, on by default), `Some` a chosen endpoint, remembered by its id
(saved in Settings: `player.output`, with its name for when it is away). `resolve(choice,
endpoints, default)` gives the endpoint to open and a note:

1. **Chosen and connected:** that device. Chosen by its hands-free endpoint (an old choice, or
   Windows' list) while its stereo twin is connected: the twin, with the note "Playing on the
   stereo connection: the hands-free one is for calls (mono, 8 to 16 kHz)".
2. **Following Windows:** the default, with the same twin rule (Windows makes the hands-free
   endpoint the default more often than people expect). A virtual default is followed: it is
   where Windows sends every app's sound, and the window says it is virtual.
3. **Chosen and away** (unplugged, switched off, a monitor asleep): the fallback, with "SNOWSKY DISC
   isn't connected: playing on Speakers until it's back".
4. **The fallback** is Windows' default unless that is virtual, else the first physical output by
   kind (wired, USB, HDMI, Bluetooth); never a virtual device, never a device whose opens all
   failed. Nothing left: the open fails ("No sound output is connected").
5. **Hands-free only:** played (it is the only way to hear that device), with the warning "AirPods
   Pro is connected for calls only (hands-free: mono, 8 to 16 kHz), so music sounds poor.
   Reconnect it, or check its stereo (audio) service in Windows' Bluetooth settings."

## 4. Devices coming and going (engine 2's deck)

A `Devices` source (trait, `devices.rs`) lists endpoints, says the default, opens an `Output` on an
endpoint and tells changes: `WasapiDevices` on Windows (`wasapi_devices.rs`), `SimDevices` in tests.
`WasapiDevices` owns a thread ("venuta-devices") that holds the enumerator and an
`IMMNotificationClient`; Windows' callbacks only wake it; it lists again (all states, with the
properties above) into a snapshot and tells the control thread `Changed`. The control thread never
touches COM and never waits for a listing.

The deck resolves at every open. Then:

- **A change** (any of added, removed, state, default) is looked at 300 ms later (Windows sends
  bursts). If the endpoint to play is now another one (the chosen device back, the default moved,
  the twin rule), the deck switches as an output switch does (H1: what sounds fades out over
  30 ms on the old device, the old output closes once its tail has played, the new opens and the
  place starts partway with its 15 ms fade-in). A device that has just appeared is waited for until
  it has been connected for 1.5 s (a Bluetooth link settling, a monitor waking in bursts), so a
  device that flaps is never switched to and from.
- **Lost** (unplugged, `AUDCLNT_E_DEVICE_INVALIDATED`, no event for 2 s): the old device is gone,
  so nothing can fade on it. The renderer comes back, the output is closed, and the first reopen is
  at once: resolved again, it lands on the same endpoint when it is still there (a format change, a
  driver restart, waking from sleep) or the fallback when it is not. The place starts where it was
  last heard, with the 15 ms fade-in (no burst, nothing old: every open is a fresh stream). Failed
  opens wait 0.5, 1, 2 and 5 s; a chosen device that fails them all is passed over until it changes,
  and the fallback opens; only when nothing opens does the player pause with the error.
- **Sleep and wake**: the stream is invalidated (or silent for 2 s): `Lost`, so the same path. A
  device not back yet after waking (USB enumerating, Bluetooth reconnecting, a monitor) means the
  fallback until it is back for 1.5 s.
- **Windows changes the format** (Sound settings, an HDMI sink renegotiating): Windows invalidates
  a shared stream, so `Lost` and an immediate reopen at the new mix format (a new renderer; the
  resampler's kernels for the new rate are made once).

Rooms keep their own rules (H4, H4b): after a lost output the room program decides whether it
plays on (the deck opens only when a program asks to play).

## 5. Exclusive mode and USB DACs

- **Probe once.** The first exclusive open of an endpoint asks it (`IsFormatSupported`, exclusive,
  with the drivers' quirks: as given, as a plain `WAVEFORMATEX`, with the standard channel masks)
  for every rate from 44.1 to 768 kHz and every container (16, 24, 24 in 32, 32, float), for the
  channel count asked. The answers are a `FormatTable`, cached per endpoint and channel count for
  the life of the app (a probe that met `AUDCLNT_E_DEVICE_IN_USE` is not cached). Opens plan from it
  without asking again; the window shows what a probed device takes.
- **The plan** (`plan_exclusive`, neutral, tested): the song's own rate at the best container for
  its bits (16 → 16, 32, 24; 24 → 24 in 32, 24, 32; 32 → 32, 24 in 32, float, 16), its channels and
  then 2; then, if the device doesn't take the rate, the nearest rate it takes (the same family
  first: 192 kHz goes to 96, 88.2 to 176.4 or 44.1; the smallest at or over the song's rate, else
  the largest under it), our band-limited resampler converting (still exclusive: no Windows mixer,
  no effects; said as "Resampled to 96 kHz"); a depth the device lacks goes to the deepest it takes.
  Then shared mode, which always works (H5; also when another app holds the device).
- **No reopen between songs that land on one format**: in exclusive mode a song "fits" the open
  output when its plan's first format is the one open, so songs of 44.1 and 48 kHz on a 48 kHz-only
  device play gaplessly.
- **Bluetooth and virtual devices play shared** even with exclusive on: the codec re-encodes
  anyway (nothing is bit-perfect over Bluetooth), and a Bluetooth link reopened for every rate
  change drops sound for a moment each time.
- **The honest state**: bit-perfect only when nothing changes the samples (as now) and the device
  is neither Bluetooth nor virtual. The signal path says why when it isn't: "Shared: another app
  has the device", "Shared: the device takes no exclusive format for this song", "Re-encoded for
  Bluetooth (SBC, AAC or aptX)", "Hands-free connection: mono, 8 to 16 kHz", "A virtual device:
  other software gets the sound and may change it".

The output's `DeviceNotes` gain the probed table and why it fell back to shared; the `Output`
trait gains `notes()` (default `None`, additive, a `contract:` commit) so the deck reads them from
any output.

## 6. The delay per output

- **Measured where Windows reports it**: the presentation pair already places the frame at the DAC
  (the device's buffer and the engine), and the output's learned latency (WP1d) is what the window
  shows as measured.
- **Set where it can't**: Bluetooth's codec and the headset's buffer, an AV receiver, an output's
  own sound effects. One store, `player.output_delay_ms` in Settings, keyed by endpoint id: a value
  (-500 to +500 ms) means the person set it; no entry means unknown. Listen together's delay
  (`together_set_delay`, `my_delay`) reads and writes the same store, keyed by the endpoint the
  player is really on (not "default"), and a room is told again when the player moves to another
  endpoint. Values saved under the old `listening.together_delay` move over once (the old field is
  kept, no longer written).
- **The suggestion**: 170 ms for Bluetooth stereo (RTINGS' typical SBC), offered in the picker as
  a button, never applied silently (D13).
- **Used now by lyrics**: the engine is told the store (`Cmd::OutputDelays`) and puts the extra
  delay of the endpoint it plays on into the place it reports (`position_ms` is what reaches your
  ear: the frame at the DAC a delay ago), so lyrics, the progress bar and anything timed by it
  follow what you hear. A switch to another output picks up its own value at once, with no round
  trip. Rooms keep taking theirs from the app (the plan's `extra_latency_ms`, `set_extra_latency`),
  from the same store. "Line up by ear" stays WP19's.

## 7. What the window shows

`player_devices` gives the entries, grouped by kind in this order: Bluetooth, USB, Wired, HDMI and
DisplayPort, Virtual, Other; the twins as one device; the chosen one (or "not connected"); a
"Follow Windows" switch on top (on: the choice is `None`; off: the device playing now becomes the
choice). For the device playing: bit-perfect or shared and why, its format, and its delay
("74 ms measured, plus 170 ms set"), with the delay slider and the Bluetooth suggestion. The same
picker is in the quality badge's popover and in Settings > Playback. `Signal` gains `endpoint`,
`kind`, `note`, `delay_ms` and `suggested_delay_ms`.

## 8. Tests

- `devices.rs` unit tests: the owner's endpoint set (names made up, ids invented) classified and
  listed; twins merged; resolve in every case above; the exclusive plan for a DAC with a few
  formats; the probe cache asked once.
- `tests/devices.rs` (engine 2 on a simulated clock, through `SimDevices`, whose outputs are
  `SimOutput`s on the endpoints' formats): Bluetooth twins (chosen hands-free, default hands-free,
  hands-free only, exclusive asked); a USB DAC with limited formats (native, a rate it lacks, songs
  that share a format play without a reopen, the probe asked once); HDMI that vanishes and comes
  back (and flaps); a format change while playing; sleep and wake (the clock leaps with nothing
  running); the default moving while followed; a virtual default never picked as fallback; the
  delay in the reported place. Every run of every output is judged by the detectors (music-shaped,
  bit-exact where not resampled, LPC, dips), each run starts from silence with its fade-in and a
  switched-from run ends faded, nothing stale, and the place carries on across the switch.
- `app/tests/outputPicker.test.ts`: grouping, names, the state line, the delay words.
- Screenshots of the picker in headless Chrome from a dev preview page.

The guided test on the owner's devices is [qa/devices-live.md](../qa/devices-live.md).

## 9. Open questions for the owner

- Unplugging wired headphones now moves the music to the speakers (as asked). Phones pause
  instead, so music never suddenly plays out loud; a "pause when headphones go" option is easy to
  add on this design if wanted.
