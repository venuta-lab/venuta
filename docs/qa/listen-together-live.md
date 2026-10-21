# Listen together: the live test matrix

Run by hand before each release that touches listening together (R1 on), after the
simulation gates pass. It is section 7.7 of [the design](../design/listen-together.md): two
copies of Venuta on real hardware and a real network, every way a room ends, every capture
checked for clicks, and the playback log read for what happened.

A run of one network takes about 20 minutes. Write the results in the table at the end and
keep the captures until the release is out.

## 1. What you need

- **Two computers**, A (the host) and B (the guest), both on the build under test. Two
  installs on one computer with two outputs also works for N1 and N2, but not for the network
  rows.
- **The kit**, built once on a computer with the Rust toolchain and copied to both:

  ```text
  set CMAKE=C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin\cmake.exe
  cargo build --release -p player --examples
  ```

  The tools are in `target\release\examples\`: `live_check.exe`, `acoustic_offset.exe`,
  `deadline_probe.exe`, `output_delay.exe`. Each prints its commands when run without any.
- **Three songs that run into each other gapless** (a live album or a mix), each 2 to 4
  minutes, on both computers as the same files. For N2 the guest uses the YouTube Music copies
  instead.
- **For N1 and N1b, the acoustic rig**: a stereo USB audio interface with a line input, and
  two cables. A's headphone or line out goes into the left input, B's into the right. The
  interface can be on a third computer, or on A.
- **clumsy** on B, to throttle the network (the owner downloads it from its project page; the
  kit does not).
- **Bluetooth headphones or a speaker** for N1b.
- **A phone hotspot** for N4.

## 2. Before the first network

On **each** computer:

1. **Shared mode.** In Venuta, Settings, Playback: turn **Exclusive (bit-perfect)** off. The
   loopback capture hears only shared-mode sound; in exclusive mode it hears nothing and
   `live_check` says `NO RESULT`.
2. **No sound effects on the output.** Windows' "audio enhancements" (and vendor suites such
   as Nahimic or Dolby) change the sound after Venuta and before the loopback. They blunt
   detector 4, make captures differ from the file, and add delay `GetPosition` may not know
   of (on the owner's laptop speakers about 100 ms, see [output-delay.md](output-delay.md)).
   Turn them off for the output under test (Settings, System, Sound, the output, Audio
   enhancements: Off), or use an output that has none (most USB DACs).
3. **Prove the kit on this computer**, nothing else playing (each plays the test music at
   -30 dBFS for 3 s):

   ```text
   live_check selftest
   live_check tone
   live_check tone --inject drop
   live_check tone --inject gap
   ```

   Passing: `selftest` says `Self-test passed`; `tone` says `the loopback is the music itself`
   and `PASS`; each `--inject` run says `detector 3 caught it`. If `tone` warns that Windows
   changes the sound, go back to step 2.
4. **The render deadline here** (plays silence for a minute, then the same under load):

   ```text
   deadline_probe probe --seconds 60
   deadline_probe probe --seconds 60 --stress
   ```

   Passing: `PASS` both times (p99.99 of the margin over 30% of the period, never dry).
5. **Note the playback log's place**: `<library folder>\.venuta\logs\playback.log`. Each line
   starts with the time in Unix milliseconds; `live_check` prints both forms.
6. **Solo captures** (the reference that tells the music's own peaks from the room's). Play
   the three songs from the start of the first, alone, no room, and capture them:

   ```text
   live_check capture --out solo-A.wav       (on A; on B: solo-B.wav)
   ```

   Press Enter after the third song. Keep these files for every network that uses these songs.

## 3. The script

The same steps on every network; section 4 says what changes per network. Times in brackets
are how long to wait before the next step.

| # | Who | Do | Expect |
|---|---|---|---|
| 0 | A and B | Start the captures (below). A: `live_check capture --out N1-E1-A.wav --solo solo-A.wav --log <A's playback.log>`. B: the same with `-B`. Where the rig is wired: `acoustic_offset record --input <id> --out N1-E1-acoustic.wav --gate n1` | Each says `Recording ...` |
| 1 | A | Open the room (the listen together button, **Start a room**), **Copy invite**, send it to B | |
| 2 | B | **Join a room**, paste the invite. A: **Let in** | Both see each other |
| 3 | A | Play the first of the three songs from its start | Both play; in step by ear within a few seconds |
| 4 | | Let all three songs play through their boundaries (or, to save time, seek to 30 s before the end of each and let the boundary pass; that adds a seek per song) | No gap, no click at the boundaries |
| 5 | A (or B with the role) | Pause [5 s], then play | Both fall silent together and start together |
| 6 | A | Seek to 1:30 in the song playing | Both move, smoothly |
| 7 | A | Skip to the next song | Both change |
| 8 | B | clumsy: filter `outbound or inbound`, **Lag** 400 ms and **Drop** 40%, Start, until B's panel shows **Buffering**; then Stop clumsy | B fades out alone and comes back in step; **A never pauses** |
| 9 | B | Change B's output (the output picker, or plug in headphones) [10 s], then back | B is silent briefly and comes back in step; A goes on |
| 10 | | End the room by this run's way (section 5) | As section 5 says |
| 11 | A and B | Each presses play on their own queue [10 s], then pause | Each queue is back **paused where it was before the room**, and plays clean |
| 12 | A and B | Press Enter in each capture window | Each prints its report (section 6) |
| 13 | A and B | `live_check log <playback.log> --from <the time of step 1>` | `PASS` lines (section 7) |

## 4. The networks

| Network | Setup | Script | Ways of ending | Captures and measures |
|---|---|---|---|---|
| **N0** same Wi-Fi, one speaker (R2 on) | B joins with **Play on Asha's speaker** (B is a remote) | full, every press made from B | E1, E2, E9 | A only. Also: `live_check compare N0-E1-A.wav solo-A.wav --seconds 60` must say `bit-identical` (step 3's first minute against the solo run's) |
| **N1** same Wi-Fi, every device, exact copies | both on the same Wi-Fi; R1: **Same room (tight sync)** on; R2: **Play on this device too** | full | E1, E2, E3, E4, E5, E6, E7, E8, E9 | A, B, and the acoustic rig with `--gate n1` |
| **N1b** as N1, B on Bluetooth | B's output Bluetooth headphones or speaker; B's delay set by **Line up by ear** (R2), or the **Your delay** slider (R1) | full | E1 | A, B (loopback of the Bluetooth output), and the rig with `--gate n1b`: B's Bluetooth headphone cup held to a microphone into the right input, or the speaker's line out |
| **N2** same Wi-Fi, apart, B on YouTube copies | **Same room** off (R1) or B in apart mode (R2); B's songs from YouTube Music, A's from files | full | E1, E2 | A and B. No acoustic gate (apart); the rig with `--gate apart` if wired |
| **N3** internet through the relay | B: Settings, Listening, **Hide your IP address** on | full | E1, E2, E4, E6, E7 | A and B |
| **N4** internet direct | B on a phone hotspot, A on the home network | full | E1, E2 | A and B |

E9 (hand over) and N0 come with release R2 (work packages 18 and 20). Until then, skip them
and say so in the results.

## 5. The ways of ending

| Way | Do | Expect |
|---|---|---|
| **E1** host ends | A: **End the room** (R2: **End for everyone**, confirm) | Both fade out over about a second; both own queues back paused |
| **E2** guest leaves | B: **Leave the room** | B fades out; A plays on, alone, and the room stays open |
| **E3** host removes the guest | A: the guest's **Remove** button | B fades out with a neutral message; A plays on |
| **E4** host app killed | A: Task Manager, Venuta, End task (or `taskkill /IM Venuta.exe /F`) | B plays on to the end of the song (and the next if it was armed); 2 minutes after the kill, at the end of the song playing, B stops cleanly with "The host went offline" |
| **E5** guest app killed and reopened | B: end Venuta's task, open it again, rejoin with the invite (R2: the **Rejoin** prompt) | A plays on; B comes back in step as the same person |
| **E6** cut 20 s, then back | B: Wi-Fi off (or airplane mode) for 20 s, then on | B plays on through the cut; in step within 2 s of the connection back; nothing heard |
| **E7** cut 150 s | B: Wi-Fi off for 150 s | B plays on to the end of the song playing when the 2 minutes run out, then stops cleanly ("Lost connection to the room") |
| **E8** host sleeps | A: Start, Power, Sleep; wake it after 30 s | B plays on; A, on waking, comes back as the host or ends cleanly, never a burst of old sound |
| **E9** hand over (R2) | A: **Hand over to...** B | B hears nothing change; A fades out; B hosts |

For E4, E5, E7 and E8 the computer that was killed, cut or asleep has a capture with a hole in
it: stop that capture after it comes back and start a new one for the rest.

## 6. Reading a capture report

`live_check capture` (and `live_check analyze <file> --solo ... --log ...` later) prints one
line per flag:

```text
  CLICK     83.412 s  14:02:11.204 UTC (1792622531204)  lpc residual   41.3 dB     0.4 ms
            97.030 s  ...                               dip            26.0 dB    11.0 ms  at a shaped event the log names: "sync: lined up ..."
```

- **CLICK** lines are counted. The run passes with none: `PASS: no clicks or dips from
  detectors 3 and 4`.
- Lines without CLICK are set aside, with the reason: also in the solo run (the music's own),
  next to a capture glitch (the recorder itself was late; rerun if there are many), or a dip
  at a shaped event the log names (a pause, a seek, a skip, a stall; never a click).
- For each CLICK, find the playback log's lines at that time (the Unix ms in brackets) on both
  computers: they say what the player did then.
- Detector 3 is most sensitive on clean, tonal music and least on noisy passages; `tone`
  proves it at full sensitivity on this computer. Detector 4 finds gaps and dips of 5 ms and
  longer.
- The analysis holds a capture in memory (about 0.8 GB for 10 minutes with its solo run):
  keep each capture under 30 minutes, one per run of the script.

`acoustic_offset` prints the offset of B against A after the first 5 s (p50, p95, worst, the
drift in ppm) and `PASS` or `FAIL` against the gate given.

## 7. What the playback log must show

`live_check log <playback.log> --from <ms>` checks the parts that can be checked by machine:
no room lines after the ending, never `ran dry`, and the render deadline histograms when the
new engine writes them (section 4.7 of the design: once a minute and when the output stops,
`render: deadline margin over <s> s: <n> periods, ran dry <d>, late <l>; per 1% of the period
from 0%: <100 counts>`; `deadline_probe log <playback.log>` prints them added up). Read the rest
by eye. The lines below are today's (engine 1); work package 9 renames some, and keeps the
`room:` and `sync:` prefixes the checker looks for.

| Step | Host (A) | Guest (B) |
|---|---|---|
| 1 Open | `starting a room`, `room: endpoint open`, `room: invite ready`, `room: follower started`, `room: playing on` | |
| 2 Join | `room: <id> is in` | `room: ...` lines of joining |
| 3 Start | `follow: prepare <n> at <ms>` or `sync: the room's song started at <ms> ms, on its frame` | the same |
| 5, 6 Pause, seek | `sync: lined up from <ms> to <ms> (faded out and in)` for a seek | the same |
| 8 Throttle | **nothing about waiting**: no `waiting for the stream`, no pause | `waiting for the stream pos_ms=...`, then `done waiting for the stream ...` |
| Any time | no `the output ran dry` | no `the output ran dry` |
| Any time | `sync: rate <+n> ppm` now and then is fine (the drift held); never more than 500 ppm | the same |
| 10 E1 | `room: the host ended it`, `room: the player is on its own again` | `room: over for this guest (...)`, `room: the player is on its own again` |
| 10 E2 | `room: <id> left` | `room: over for this guest (left)`, `room: the player is on its own again` |
| 10 E3 | `room: the host took <id> out (blocked: false)` | `room: over for this guest (...)` |
| 10 E6 | | `room: the connection to the host dropped; trying again`, then `room: back in touch with the host` |
| After the ending | **no line** starting `room:`, `sync:` or `follow:`, and none `from listening together` | the same |

## 8. Pass

A network passes when all of these hold (section 7.7):

- 0 CLICK lines in every capture of it, on both computers.
- N0: A's output bit-identical to the solo run (`compare`).
- N1: acoustic p95 within 2 ms, worst within 5 ms (`--gate n1`). N1b: p95 within 5 ms.
- The throttled guest buffered alone; the host's capture and log show no pause.
- After each ending, both own queues came back paused at their places and played clean.
- `live_check log` passes on both computers: no room lines after the ending, never ran dry.

## 9. Results

Copy this table for each release.

| Release | Network | Ending | A capture | B capture | Acoustic | Log A / B | Notes |
|---|---|---|---|---|---|---|---|
| | N1 | E1 | PASS, 0 | PASS, 0 | p95 0.0 ms, max 0.0 ms | PASS / PASS | |

Also once on the rig, before every device mode ships: whether `GetPosition` includes the
Bluetooth delay on the owner's headphones and speaker, in [output-delay.md](output-delay.md).
