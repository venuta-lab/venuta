# Listen together: design and implementation plan

- **Date:** 2026-10-21
- **Status:** decided (section 11), for the owner's review before building
- **Base to build on:** `D:/UIImprovement/ml-together`, branch `work/together` (fa8817a). `work/together-wip` (4386fad) is not a base; its idea (status and file opens off the sound's thread) is part of this design.
- **Inputs:** the sync review report and its notes 00 to 06, how shipped products behave and their use cases (note 07), the first pass's findings (note 08), the code on `work/together`.
- **Platforms:** Windows now (WASAPI). Android next with the same Rust core (AAudio). Nothing in the sync logic may assume Windows.

## Contents

1. Goals, rules and non-goals
2. Where we start from
3. Architecture overview
4. Components
5. Interface contracts
6. Use cases
7. Acceptance gates
8. Keep, rework, delete: file by file
9. Implementation plan
10. Risks
11. Decisions
- Appendix A: every parameter in one table
- Appendix B: words used here

---

## 1. Goals, rules and non-goals

### 1.1 The rules

1. **Clean audio always wins over tight sync.** The music is only ever changed in four ways, all of them inaudible or nearly so:
   - a change of rate of at most 500 parts per million (under one cent of pitch), through a band-limited resampler;
   - a fade from or to silence (15 ms in, 30 ms out, raised cosine);
   - an equal-power crossfade (40 ms) between two places of the same song, at a quiet moment when one can be found;
   - a song boundary, which is either sample-exact or shaped by the room's transition.

   Nothing else. No hard edge, no gap with hard edges, no dropped or repeated frame, no step in gain. If a sync goal can only be met by breaking this, the sync goal loses.

2. **After a room ends, playback is exactly normal.** Everyone's own queue, the host's included, comes back paused where they left it, with nothing of the room left, and what it plays is sample for sample what it plays when you never joined a room. This is guaranteed by structure (the solo path shares no state with rooms), not by cleanup code, and proven by tests.

Two more rules follow from what shipped products learned (note 07):

3. **The room waits for people only at a start, a resume, a seek or a pressed song change, and for 2.5 s at most.** Mid-song, nobody ever waits: a member who stalls, is interrupted, sleeps or switches output drops out alone and fades back in at the live place. The room is never paused for one person (Jellyfin's wait-for-everyone is the counter-example: it needed a deadlock timeout in 2026).
4. **A network problem never touches the audio directly.** Once a person has "content P at host time T" and a clock line, they play on through drops, on their own clock, until the song (and the armed next) is done.

### 1.2 Three ways to listen together

Note 07's main finding: no major product syncs many phones tightly in one room. Spotify Jam in person and Apple Music SharePlay on HomePod or CarPlay play through **one output**, and the guests become remotes. Tight multi-speaker sync is what Sonos, AirPlay 2 and Cast do, on dedicated speakers. So Venuta offers three modes, and offers them in this order:

| Mode | Who plays | When it is offered | Sync needed |
|---|---|---|---|
| **One speaker** ("Play on Asha's speaker") | the host only; guests are remotes that see, add and (if allowed) control | first, when a guest joins on the same network as the host | none (crackle-free by construction) |
| **Every device** ("Play on every device", the same-room mode) | everyone in the room, their speakers heard together | as the second choice on the same network, for people who want several speakers | 1 to 2 ms, so outputs must be calibrated (4.14) |
| **Apart** (default over the internet) | everyone, each on their own copy | always for people on other networks | 10 to 20 ms |

A room can mix them: two speakers at the host's in "every device", a friend abroad apart, a third person in the living room as a remote. The mode belongs to the person, not the room (4.13).

### 1.3 Targets

The "Basis" column says where a number comes from: **measured** (a published measurement or a source listed in note 07, or our own measurement in note 08), or **inferred** (our design target, reasoned from the measured anchors in 1.4).

| What | Target | Basis |
|---|---|---|
| Clicks | 0 per hour in every scenario, measured by four detectors (section 7.1) | the owner's rule; detectors from OboeTester, Essentia (measured methods) |
| Every device on one Wi-Fi (speakers heard together) | 95% of the time within 1 ms in simulation, within 2 ms measured acoustically; never over 5 ms after the first 5 s | measured anchor: below about 2 ms two speakers fuse into one sound (precedence effect); Sonos, AirPlay, Cast and Snapcast reach 1 to 2 ms |
| Apart, over the internet | 95% of the time within 20 ms (direct) or 30 ms (relay); the honest floor is half the path asymmetry, shown in the panel | inferred: well under a call's own latency (G.114: 150 ms is transparent, measured); no vendor publishes an apart figure; our in-process measure was 12.6 to 13.9 ms on a simulated 30/50 ms path (note 08, measured) |
| One speaker | the host's output exactly as solo; guests' displays within 100 ms | inferred (nobody hears the guests) |
| Different copies of a song | Measured offset within 10 ms of the truth when the copy is the same recording | inferred from the apart target |
| Join | First sound within 3 s on one Wi-Fi, 5 s over the internet direct, 8 s through a relay (90th percentile, a copy at hand) | inferred (note 07 C1.2: about 5 s is acceptable) |
| A press (play, seek, skip) | Heard within 1 s on one Wi-Fi, 1.5 s over the internet | inferred (note 07 C2.1: 0.5 to 1 s is fine for music) |
| Rate correction | at most 500 ppm (under one cent) | measured: the pitch threshold is about 5 cents; Metrolist's ±2% (about 35 cents, time-stretched) and Syncplay's 0.95x are audible |
| Render thread | At most 10% of a period at worst, 3% on average at 48 kHz stereo with the resampler on | inferred |

### 1.4 Where the tolerances come from

| Anchor | Value | Basis |
|---|---|---|
| Two speakers heard as one | under about 2 ms; 2 to 50 ms fuses but pulls the image to the earlier one; over about 50 ms (speech) to 100 ms (music) an echo or flam | measured (precedence effect literature, note 07 A9) |
| Talking on a call while listening apart | under 150 ms one way is transparent, 400 ms is the planning limit | measured (ITU-T G.114) |
| Rate change heard as pitch | 1 cent = 578 ppm; threshold about 5 to 6 cents | measured (note 03) |
| Bluetooth A2DP delay | 34 to 241 ms by codec and headset (SBC 173, aptX 166, aptX LL 34, Sony WH-1000XM5 241) | measured (RTINGS) |
| Android timestamps include Bluetooth delay | yes, from the sink's delay report | source (AOSP), to be checked on the handheld |
| Windows `GetPosition` includes Bluetooth delay | unknown | must be measured: WP14 built the tool and the steps (`docs/qa/output-delay.md`); no Bluetooth output was at hand, so the owner's run decides |
| Windows `GetPosition` includes the delay of an output's sound effects (vendor effect modules such as Nahimic) | not on the owner's laptop speakers: their loopback, taken after the effects, has the sound about 104 ms after `GetPosition`'s time (three runs, spread under 2 ms) | measured (WP14, against the loopback); a microphone run confirms |
| Sound card drift | 50 to 100 ppm typical (3 to 6 ms a minute) | measured (roc, Roon, note 03) |
| Same-room buffers in products | Cast 500 ms, Snapcast 1000 ms, Sonos line-in 2000 ms default | measured (note 07 A8) |
| Apart thresholds in open peers | Metrolist soft 50 ms, hard 750 ms; Teleparty 2.5 s; Jellyfin 500 ms | source (note 07 A6, A7) |
| Everything else in section 6 | | inferred unless marked |

### 1.5 Non-goals

- No server of our own. Peers connect with iroh; its public relays are the only third party, and Local only and own-relay modes stay.
- No PCM streaming from the host as the main path. Each peer still plays its own best copy; the host's file stream stays for songs only the host has.
- No more than 10 people in a room (the host's upload carries streamed songs).
- No video, no voice chat.

## 2. Where we start from

The first pass (note 08) found six causes of crackle and fixed them on `work/together` with good building blocks. The click detector reports zero clicks on every simulated case there. What is wrong is the shape: the building blocks are bolted onto an engine where one thread does commands, decoding, room logic, DSP, status callbacks and device writes, and a follower thread translates the room into plans that the engine re-reads.

What we keep as sound building blocks (from note 08, checked against the code):

| Building block | Where now | Why it is sound |
|---|---|---|
| 64-tap Kaiser windowed-sinc variable-rate resampler that lands on a whole frame | `crates/player/src/sync.rs` `Resampler` | Measured -90 dB at 1 and 15 kHz, ±500 ppm; bit-perfect when idle |
| Fades and gain ramps on raised-cosine curves, carried across periods | `engine.rs` `FadeOut`, `GainRamp`, `sync.rs` `fade_gain` | The right curves and durations |
| Clock as a weighted line fit (offset and skew), slewed at 50 µs/s, stepped only for a lasting jump | `crates/together/src/clock.rs` | State of the art (chrony, the Cast patent) |
| One controller for both modes that learns the sound card's drift | `sync.rs` `Controller` | Right idea; becomes a PI loop with feed-forward here |
| Starvation as fade out, wait, fade back in | `engine.rs` `suspend_or_resume` | Right behaviour; becomes a per-person suspension here |
| Room logic kept pure and tested | `crates/together/src/room.rs` | Roles, knocking, activity, undo, queue edits are all good |
| A whole room in one process with a simulated network | `app/src-tauri/src/together/room_check.rs` | The base of the simulation harness |
| The click detector with self-tests | `crates/player/tests/common/mod.rs` | Becomes detector 1 of four |
| MMCSS "Pro Audio", `GetPosition` with its QPC stamp, 50 ms shared buffer | `wasapi_out.rs` | Correct and needed |

What the first pass itself calls workarounds, and this design removes:

| Workaround | Replaced by |
|---|---|
| `fade_out_now` runs `step()` loops inside command handling | A splice stage on the audio thread that fades by itself; commands never wait for audio |
| The follower tracks the engine's queue index | Room entries with stable ids; the room program in the engine knows which entry each voice plays |
| `sync_hold`, `sync_fresh`, `sync_fine`, `settle_until`, `seek_after_fade` | Explicit voice commands at device frames (start here, switch there) |
| Re-planning through `Cmd::Sync` when the clock estimate moves | The engine reads the clock mapping directly each tick |
| Pings on the reliable stream | QUIC datagrams |

The implementer's "if starting clean" list maps to this design as follows: decode thread and lock-free ring (4.6, 4.7); the room engine reads the clock mapping (4.2, 4.9); each song's plan sent with Prepare and switched at the engine's own boundary (4.4); pings on datagrams (4.1); offset measured between copies (4.5); paced uploads and control priority (4.1); starvation as a per-person suspension (4.10). Two open items are also closed here: lossy-seek pre-roll and the packet lost mid-read (4.6).

## 3. Architecture overview

### 3.1 Components

```
+----------------------------------------------------------------------------------+
|  Window (React)                                                                  |
|  room panel, chip, queue, per-member status, delay slider                        |
+-----------------------------+----------------------------------------------------+
                              | commands (Tauri)            ^ View events, 10 Hz
                              v                             |
+----------------------------------------------------------------------------------+
|  App shell (app/src-tauri): Tauri commands, event sink, copy finding (sources)   |
|  No room or sync logic of its own.                                               |
+-------------+-------------------------------------------+------------------------+
              |                                           |
              v                                           v
+-----------------------------+            +--------------------------------------+
|  Room session (together)    |            |  Player (crates/player)              |
|  host: Room v2 (pure)       |  RoomHandle|  control thread: programs, voices,   |
|  guest: mirror + Conductor  +----------->|  device, sync tick (rooms only)      |
|  Clock service (pure)       |  ClockMap  |  decode threads (one per voice)      |
+-------------+---------------+  (shared)  |  render thread (owned by the output) |
              |                            |  status thread                        |
              v                            +------------------+-------------------+
+-----------------------------+                               |
|  Transport (together-net)   |                               v
|  iroh QUIC: control stream, |            +--------------------------------------+
|  clock datagrams, media and |            |  Output (platform)                   |
|  cover streams, pacing      |            |  WASAPI now, AAudio later, Sim tests |
+-----------------------------+            +--------------------------------------+
```

### 3.2 Threads and what they may do

```
render thread      (MMCSS Pro Audio on Windows; the AAudio data callback on Android)
   reads:  voice rings (SPSC), render commands (SPSC), rate target (atomic)
   writes: device buffer, markers ring, render status (seqlock), notes (SPSC)
   never:  allocates, locks, blocks, logs, does I/O, calls the app

decode thread x3   (above normal priority; one per voice slot)
   opens and decodes one source, pre-rolls lossy seeks, measures block loudness
   may block on its own source (file or download), never on anything shared with the render thread

control thread     (normal priority; the engine's command loop)
   owns programs (your own queue, or a room), voice slots, the device, the DSP chains
   in a room only: a 20 ms sync tick that measures the error and sets the rate target

status thread      (normal priority)
   reads render status and control state, builds Status, calls the app's callback, writes the log

session tasks      (tokio)
   transport, clock pings and answers, the room (host), the conductor (each person)
```

### 3.3 What happens in one period (a room, steady state)

```
decode thread A  --blocks-->  ring A  --+
decode thread N  --blocks-->  ring N  --+-->  render: pull, resample (ratio), DSP chain,
                                        |      envelope, mix, master gain, remap, encode
                                        |      --> device buffer
                                        |      --> markers (device frame <-> content place)
control thread, every 20 ms:            |
  presentation (frame at the DAC, time) + markers  -> what content is heard now
  clock map (local -> host time) + room anchor     -> what content should be heard now
  error -> controller -> rate target (atomic)  ------+
  error far and lasting -> prepare voice R, then "switch A to R at frame F" (crossfade)
```

### 3.4 Ownership

| Component | Owns | Runs on | Talks to others through |
|---|---|---|---|
| Transport | the iroh endpoint and connections, stream priorities, pacing | tokio | `Transport` trait: control messages, datagrams, media streams |
| Clock service | ping samples, the fitted line, the published `ClockMap` | tokio (pinger and answerer), pure logic | `ClockMap` snapshot (read by the player's sync tick) |
| Room (host) | the room queue (entries), anchor, prepares, people, roles, activity | tokio, pure logic | messages out; `Changes` in tests |
| Conductor (everyone, host too) | which entry each voice should play, readiness, suspensions, copy choice requests | tokio | `RoomHandle` to the player; `Report` to the room |
| Player control thread | programs, voice slots, device lifecycle, DSP chains, controller | own thread | `Cmd` channel in, render commands out, status out |
| Decode threads | one `Source` each | own threads | voice ring (SPSC) and a command channel |
| Renderer | the splice, gain and resample stage | the output's render thread | SPSC rings and atomics only |
| Output | the device, its render thread, presentation timestamps, device events | platform | `Output` trait |
| Alignment | envelopes and offsets | decode threads (extraction), tokio (correlation) | `Alignment` value handed to `RoomHandle::prepare` |

## 4. Components

### 4.1 Transport

One iroh QUIC connection per guest, guest to host (a star). The ALPN stays `venuta/together/1`; the protocol number in `Hello` goes to 3, and a guest speaking another number gets the existing "different versions" refusal.

| Channel | QUIC object | Priority (`SendStream::set_priority`) | Carries | Notes |
|---|---|---|---|---|
| Control | the first bidirectional stream | 100 | JSON lines, `ToHost` and `ToGuest` v3, strictly checked as now | Small messages only (no envelopes, no covers) |
| Clock | datagrams (`send_datagram`, `read_datagram`) | not applicable | 29-byte binary ping and pong | No retransmit, no head-of-line wait. Falls back to the control stream only if `max_datagram_size()` is `None` |
| Envelopes | one bidirectional stream per request, `Get { get: "env:<key>" }` | 50 | a song's alignment envelope (about 4 to 8 KB, deflated) | Asked by the guest when it prepares the song |
| Host files | one bidirectional stream per range request (`Get` as now) | 0 | file bytes | Paced (below) |
| Covers | one stream per cover (as now) | -10 | JPEG | |

**Clock datagram format** (little-endian): `kind: u8` (1 ping, 2 pong), `id: u32`, `t1: i64`, `t2: i64`, `t3: i64` (µs; ping leaves t2 and t3 zero). Fixed length; anything else is dropped. At most 50 per second per peer are answered.

**Timestamps where they belong.** The host stamps `t2` in the datagram task as `read_datagram` returns, and `t3` immediately before `send_datagram`, on the same task, with nothing queued between. The guest stamps `t4` as its `read_datagram` returns. No channel or JSON encoding sits between a stamp and the wire.

**Pacing of host files.** Each range stream gets a token bucket on the host: a first burst worth 10 s of the song's average bitrate, then 2 times that bitrate. With priorities, the control stream and datagrams are never queued behind file bytes. The guest's reader already asks in 2 MiB ranges; it keeps asking (the whole song still downloads), only more evenly.

**Liveness.** Clock pings at 1 Hz double as a heartbeat. The host shows a member as away after 5 s with no ping; the QUIC idle timeout is 15 s. A guest whose connection closes tries again every 2 s for 2 minutes (backing off to 5 s), keeping its per-room key so the host knows it is the same person (as now). Two minutes covers a Wi-Fi roam or a phone screen sleeping, and is short enough that the host's member list stays honest (note 07 C4.8; Metrolist allows 15 minutes, Jellyfin 60 s). Meanwhile the guest plays on (rule 4).

**Presses while reconnecting.** Held and sent when the connection is back, but only those younger than 10 s; older ones are dropped with a note ("Your pause was too late to send"). Today they are held without age. Whatever the guest missed is applied from the host's latest anchor on reconnect (it carries a sequence number). Note 07 recommends re-sending every held press; decision D10 chooses between the two.

**Path changes.** The transport watches `Connection::paths_stream()` and tells the clock service when the selected path changes (relay to direct, direct to relay, one address to another), so the clock can re-measure quickly (4.2).

**Where it lives.** A new crate `crates/together-net` (iroh, tokio, no Tauri, no app types): endpoint and reach settings (from today's `net.rs`), the host's accept loop and per-person serving, the guest's connect and reconnect, the 127.0.0.1 proxy for host files, the datagram clock tasks, pacing. It exposes a `Transport` trait so sessions run against a simulated network in tests. This matches the Android plan, which moves these files out of the Tauri crate anyway.

### 4.2 Clock service

Each guest estimates the host's clock. The host's own map is the identity.

**Time base.** The room's time is the host's monotonic clock in microseconds, the same clock the audio timestamps use (QPC on Windows, `CLOCK_MONOTONIC` on Android), counted from boot, not from when the session began (today's `net::Clock` counts from session start). A host that restarts its app keeps the same time base, which makes reopening a room possible (section 6, I6).

**Exchange.** A burst of 20 pings 50 ms apart on connect, on reconnect and on a path change; then one per second.

**Estimate.** Kept from `work/together` (f62c2fc):

- a window of the last 64 samples;
- selection of the quickest: delay at most the least delay plus the larger of 1 ms and half the way to the median delay;
- a weighted line fit (offset and skew) once there are 8 selected samples over 30 s; before that, the mean of the 3 quickest;
- what the player reads is slewed toward the estimate at 50 µs per second at most.

Changes:

| Change | Why |
|---|---|
| The published value is a `ClockMap { epoch, local_ref, host_ref, rate, bound }`, a line, not an offset | Today `offset()` returns the offset at the last sample time, ignoring skew until the next sample; a line extrapolates correctly between samples |
| `bound` = least delay / 2 + the 90th percentile of the residuals of the fit | An honest error bound, shown in the panel and used to widen the controller's band (4.9) |
| A step is allowed for a lasting jump: over 5 ms (same room) or 20 ms (apart) lasting 10 s, as now; after a reconnect or a path change, lasting 2 s | The path changed, so the old asymmetry no longer holds |
| A new epoch (window cleared, burst) when a sample is more than 500 ms off the map | A machine that slept, a clock reset; old samples describe another world |
| Ready after 8 samples or 1 s of burst, whichever is later | A ready clock has a known bound |

**How it reaches the player.** The session publishes the `ClockMap` into a shared snapshot (an `ArcSwap` or a seqlock). The player's sync tick reads it every 20 ms. Nobody re-plans anything when the estimate improves: the error the controller sees moves by at most 50 µs per second, which the controller takes up by rate without a sound.

### 4.3 Room session state machines

#### Guest

```
        join(invite)
Idle ---------------> Connecting --(no route 25 s)--> Ended(Unreachable)
                          |
                          | connected, Hello sent
                          v
                      Knocking --(Refused)--> Ended(Refused)
                          |  \--(Waiting)--> AtTheDoor --(Welcome)--+
                          |                                         |
                          +-----------(Welcome)---------------------+
                                                                    v
                                                               Syncing clock
                                                                    | ready (8 samples, 1 s)
                                                                    v
            +-------------------------------------------------> InRoom
            |                                                    |   \
            | back within 2 min (clock: new burst, step allowed) |    \ Leave / Refused(removed) / Ended(host)
            |                                                    |     v
       Reconnecting <------------- connection lost --------------+   Ending (1 s fade) --> Ended --> Idle
            |   plays on through the current song and the armed next         (own program back, paused)
            +--(2 min, then the end of the song playing)--> Ended(Lost)
            +--(HandOver from the old host)--> Connecting to the new host (the music plays on)
```

While `InRoom`, the person's player has its own sub-state (shown beside their name):

| Player sub-state | Meaning |
|---|---|
| Opening | getting this entry ready |
| Ready | ready, waiting for the room's start |
| InStep | playing, error inside the band |
| Correcting | playing, rate being changed (error between band and realign line) |
| Realigning | a crossfade to the room's place is prepared or under way |
| Suspended(reason) | faded out locally: `Buffering`, `OutputLost`, `Interrupted` (Android audio focus), the room goes on |
| SteppedOut | paused just for you; the room goes on |
| CannotPlay | no copy of this entry; back for the next one |
| Remote | one speaker mode: this person plays nothing, sees the room and controls it as their role allows |

#### Host

```
Idle --open--> Opening (endpoint, invite; 6 s at most online) --> Hosting
Hosting --End for everyone (confirmed)--> Ending (Ended to all, 300 ms; 1 s fade) --> Ended --> Idle   (own queue back, paused)
Hosting --Leave, hand over to a member--> HandingOver (4.15) --> Ended --> Idle                   (own queue back, paused)
Hosting --Leave without a hand-over--> as End for everyone
```

The host's own player follows the room through the same conductor as a guest, with the identity clock map. The room's clock is not the host's player: if the host's own stream stalls, the host suspends like anyone, and the room goes on.

Per member, as the host keeps them: `Knocking`, `AtTheDoor`, `In(player sub-state)`, `Away` (connection lost, kept 2 minutes), `Gone`.

#### The room's own state (host)

```
Resting (no song) --prepare--> Preparing(target, purpose) --all ready or 2.5 s--> Playing(anchor, next?)
Playing --pause--> Held(anchor)        Held --play--> Preparing(purpose Resume) --> Playing
Playing --seek/skip/jump--> Playing + Preparing(target)   (the current anchor keeps playing until the commit)
Playing --boundary of the last entry--> Resting (end of the queue)
Playing --every member, the host included, suspended for 3 s--> Held (auto-hold, decision D11; never for one person)
Playing --nobody can play the entry (everyone CannotPlay) for 5 s--> skip to the next entry ("Skipped X: no one could play it")
Held --no action for 60 min--> the room ends itself, with a notice to the host
```

The big change from today: a seek, skip or jump no longer holds the room silent while everyone gets ready. Everyone keeps playing the current place until the room commits the new one, then everyone switches at the same moment. This is how a seek feels in a normal player, a little later.

### 4.4 Shared timeline model

#### Entries, anchors, next

```rust
struct Entry   { id: EntryId /* u64, given by the host, never reused */, song: Song }
struct Anchor  { entry: EntryId, content_us: i64, host_us: i64, playing: bool, seq: u64 }
struct Next    { entry: EntryId, boundary_host_us: i64, transition: Transition, seq: u64 }
enum Transition { Gapless, Crossfade { ms: u32 }, Cut }
```

- **Anchor:** the reference copy's content time `content_us` is heard at host time `host_us`; while `playing`, it moves at exactly one second per second of host time.
- **Reference copy:** the copy the room's times are measured on. For the host's file, that file. For an online song, the catalog stream as the host decodes it. If the host cannot play an entry, the catalog's stated length stands in, and every copy is "approximate" for that entry.
- **Entry ids** replace (index, rev). Moving, adding and removing songs never changes which entry is playing or armed; the conductor never maps indexes to the player's queue.
- **Positions in microseconds**, not milliseconds.

#### Prepare and commit (start, resume, seek, skip, jump)

```
presser        host Room                       each conductor / player
  | Ask(seek)    |                                   |
  |------------->| Preparing{prep 17, entry 4, 95 s} |
  |              |---- Prepare{17, entry 4, 95 s} -->| voice R: open (or reuse the download),
  |              |                                   | seek with pre-roll to 95 s + my offset,
  |              |                                   | fill, align
  |              |<----------- Ready{17} ------------| (voice A keeps playing meanwhile)
  |              | all ready, or 2.5 s passed        |
  |              | at = now + lead                   |
  |              |---- Commit{anchor 4, 95 s, at} -->| at the device frame for `at`:
  |              |                                   |   seek or jump: crossfade A to R, 40 ms
  |              |                                   |   skip: A fades out 30 ms ending there, R starts
  |              |                                   |   start or resume: R starts from silence, 15 ms fade-in mid-song
```

- **Lead:** `max(floor, 2 x p95 RTT + 50 ms)`, floor 300 ms when everyone is on one network, 500 ms otherwise, never over 2 s.
- **Late commit** (it arrives after `at`): the player starts R at the room's place for now, skipping ahead inside R's decoded audio if it can, else preparing again and joining late.
- **A newer prepare** replaces an older one (a new `prep` id; Ready for an old id is ignored). Seek drags send only on release.
- **Ready means** (per voice): at least 2 s decoded at the target for a local file; for a stream, at least 3 s decoded and either 10 s of bytes ahead or the whole file; and the alignment measured or given up on (at most 1 s).

#### Pause

The room schedules a pause instead of applying it at once: `at = now + pause lead` (150 ms on one network, `max(150 ms, 2 x RTT)` capped at 400 ms otherwise), and the held content time is where the anchor is at `at`. Each player fades out over 30 ms starting at that frame. A player that got the message late (after `at`) fades out at once and then, while silent, moves to the held place, so everyone ends up at the same place (SharePlay's `pauseSnapsToMediaTimeOfOriginator`; a move in silence is never heard). Resume prepares at the held place and starts from silence with a 15 ms fade-in; the 30 ms that faded out is heard again, which nobody notices. Speakers in one room therefore fall silent together instead of one after another.

#### Entrances mid-song

A late joiner, a rejoin after a suspension and a catch-up after stepping out all come in partway through a song. They fade in over 300 ms (a musical entrance; 15 ms is only the least that is click-free, note 07 C1.4). A joiner whose copy is slow or only approximate may choose "Start with the next song" instead: silence until the room's next boundary, then a clean start with no seek at all.

#### The next song: armed ahead, switched by each player at its own boundary

```
host: 20 s before the reference copy's end
   ArmNext{entry 5, boundary = anchor.host_us + (ref_len - anchor.content_us) - crossfade, transition}
each player: prepares voice N, reports next_ready
at the boundary (device frame computed from the clock map, no message needed):
   switch A -> N as the transition says (table below)
host, after the boundary: Commit{anchor entry 5, content 0 at boundary}; a player already there treats it as a no-op
```

What each player does at the boundary, by how its own copies compare with the reference:

| My copy of the playing song, at the boundary | My next copy | What I do |
|---|---|---|
| Ends within 5 ms of the boundary, both copies exact | Exact | Sample-exact join at my copy's end; the few ms are taken up by rate afterwards |
| Has ended already (shorter copy, or offset) | any | Silence until the boundary, then N starts (from its start, or with a 15 ms fade-in if my offset puts me mid-waveform) |
| Still sounding (longer copy) | any | A fades out over 150 ms ending at the boundary; N starts at the boundary |
| any | any, transition is a crossfade | Equal-power crossfade over the room's crossfade length, starting at the boundary |

- **The room's transition, not yours.** In a room, the host sets "Between songs: gapless, or crossfade 2, 4, 6 or 8 s, never within an album". Your own crossfade setting is set aside while you are in a room (it would put your next song at another moment than everyone else's) and comes back with your own queue.
- **Short songs.** A song shorter than 20 s is armed as soon as it starts. A song shorter than the lead still plays; the next is armed with its boundary at once.
- **Late arming.** A player whose next voice is not ready by the boundary minus the lead joins the next song late (catch-up), alone.

### 4.5 Source selection and alignment

#### Which copy each person plays

Kept from `sources.rs` (`best_copy`), with the order made to depend on the person's mode:

| Mode | Order |
|---|---|
| Apart (default) | own file, catalog stream, host's file streamed, close catalog match |
| Every device (speakers that hear each other) | host's file streamed (exact), own lossless file whose decoded audio matches the host's (exact), own file, catalog stream, close catalog match |

On one network the host's stream costs nothing worth noticing, and an exact copy is the only way to be within a millisecond from the first frame (decision D5).

#### Copy classes

| Class | How it is known | Offset |
|---|---|---|
| Exact | the host's own bytes; or a lossless copy whose first 10 s of decoded PCM hash the same as the host's (the host sends a 16-byte hash with the envelope) | 0 |
| Measured | envelope correlation with enough confidence | measured, refined during the song |
| Approximate | no reference envelope (the host can't play it), or low confidence, or the copy is another edit | 0, plus known codec delays |

#### Measuring the offset

- **Envelope** (the host, for its reference copy; each guest, for its own): mono, 10 ms hop, onset strength from the rise of log energy in four bands (below 200 Hz, 200 Hz to 2 kHz, 2 to 6 kHz, above 6 kHz), one byte per hop. Windows: the first 30 s, then 10 s every 60 s through the song. Deflated, about 4 to 8 KB for a song. Made by the decode thread as it prepares the song (local files decode 30 s in well under a second; streams as their bytes come).
- **Correlation** (guest): normalised cross-correlation over lags of -5 s to +5 s on the first window, refined by a parabola through the peak (resolution about 2 ms).
- **Confidence:** the peak at least 8 standard deviations over the mean, and at least 1.3 times the best peak more than 50 ms away. Otherwise "approximate".
- **Refinement:** each later window re-measures. A change under 20 ms moves the offset gradually, which the controller takes up by rate, without a sound. A change over 20 ms between windows means another edit or another tempo: the copy becomes approximate from there, and the panel says so.
- **Where the offset is used:** `my content time = room content time + offset`. Every prepare, every boundary and every error measurement uses it. The host never needs to know it.

### 4.6 The player: programs, voices and decode threads

#### Two programs, never mixed

```rust
enum Program { Own(OwnProgram), Room(RoomProgram) }
```

- `OwnProgram` is today's queue (shuffle, repeat, stop after current, sleep timer, crossfade setting, your levelling) and where you are in it.
- `RoomProgram` holds the room's voices, the attached `ClockMap`, the anchor, the armed next, your copy offsets, your mode, the controller.
- **A guest joining:** the own program fades out if sounding, its voices are dropped, and its state is kept in memory (and on disk, as today's "where you were" already does). When the room detaches, the room program is dropped whole, and the own program comes back paused, exactly where it was. The window offers "Save the room's queue as a playlist". Today's set-aside file juggling (`set_aside`, `give_back`, the room loading its songs into your queue with `Cmd::Set`) goes away.
- **The host opening a room** must not touch the host's audio at all (note 07 C1.1): no hold, no seek, no fade. The room program is made from the own program in place: the voice playing goes on, its queue becomes the room's queue, and the room's first anchor is the host's live place (from the presentation pair). The host's own queue and place at that moment are snapshotted, as a guest's are at joining.
- **Any ending, for anyone, the host included** (decision D1): the music fades out over 1 s, the room program is dropped whole, and the own program comes back paused at its snapshot. Nothing of the room stays in it; the room's queue is offered as a playlist to save.
- "Where you were" is written from the own program only, so a crash in a room never saves the room's song as your queue.

#### Voice slots

Three voice slots, each with its own decode thread and ring:

| Slot use | Example |
|---|---|
| Current | the song playing |
| Next | the song armed for the boundary, or (solo) the next queue item decoded ahead for gapless |
| Spare | the target of a seek or a realign while the next is armed |

Each voice is a `Source` (as today), a start place, and a ring of preallocated blocks:

```rust
struct Block {
    voice: VoiceId, epoch: u32,     // a seek gives a new epoch; old blocks are skipped
    first_frame: u64,               // content frame of the block's first sample
    frames: u16,                    // up to 1024
    rms_db: i8,                     // loudness of the block, for quiet-moment search
    flags: u8,                      // END_OF_SOURCE, FIRST_AFTER_SEEK
    samples: [i32; 1024 * MAX_CH],  // interleaved, left-justified as today
}
```

Blocks go from the decode thread to the render thread on one SPSC queue and come back empty on another, so nothing is allocated or freed on the render thread.

#### Decode threads

- **One per slot.** It may block on its own source (a file read, a download that is behind), which is now harmless: the render thread never waits for it.
- **Depth:** local files 2 s decoded ahead; streams 3 s decoded; downloads continue as today (the whole song, progressively).
- **Lossy seeks pre-roll:** decode from 80 ms before the target for Opus, from one packet before it for AAC, Vorbis and MP3, and drop that part (fixes `decode.rs:862-891`).
- **No packet lost on a stall:** the decode thread decodes a packet only once the bytes for it are downloaded (it waits on the download's condition variable, with its cancel flag), so `GrowReader` never returns `WouldBlock` in the middle of a packet (closes the possible loss in `decode.rs:795-822` with `218-247`).
- **Downloads in chunks:** `GrowBody` keeps an append-only list of 64 KiB chunks instead of one growing `Vec` under a mutex (no 2x copies of a 30 MB song, short locks).
- **One download per song, shared:** voices that play the same media key (a seek, a realign, a resume) read the same `GrowBody`, so a seek on a stream never downloads again. Today this is true only by the follower's care (`follow.rs:284`).
- **Analysis on the side:** the decode thread fills each block's `rms_db` and, on prepare, makes the alignment envelope and the PCM hash.

### 4.7 The real-time render path

#### Stages, in order, per period

```
for each active voice:
   1  take input frames from its ring (enough for this period at its ratio, plus the filter's reach)
   2  if fewer are there than needed and the source hasn't ended: start a starvation fade (below)
   3  resample: ratio = source rate / device rate x (1 + ppm / 1e6), gliding at most 1000 ppm/s;
      at ratio 1 and on a whole frame: the samples pass untouched (bit-perfect)
   4  the voice's sound tools (EQ, levelling), built on the control thread and handed over lock-free
   5  the voice's envelope: fade in, fade out, crossfade gain (raised cosine or equal power), across periods
mix the voices
   6  master gain: volume and the sleep timer's fade, ramped per sample over 20 ms
   7  channel remap, encode into the device's buffer
   8  markers: (device frame, voice, content place, ratio) for this period; notes of every shaped event
```

One stage does all splicing, all gain and all resampling. Nothing upstream or downstream changes samples.

#### The splice table: every discontinuity has one shape

| Event | Shape |
|---|---|
| Pause, stop | 30 ms raised-cosine fade-out, then at least one device buffer of silence, then the device stops |
| Leaving a room, being removed, the room ending (guests) | a musical 1 s fade-out, then as a pause |
| Start, resume partway into a song | starts from silence on the exact device frame, 15 ms raised-cosine fade-in |
| Join, rejoin, catch-up partway into a song | starts from silence on the exact device frame, 300 ms fade-in |
| Start at a song's very first frame | no fade (the song's own start) |
| Seek, realign (same song, another place) | 40 ms equal-power crossfade from the old place to the new |
| Skip, jump (another song, now) | the old fades out over 30 ms ending at the switch frame; the new starts there |
| Song boundary, gapless, exact copies | sample-exact join |
| Song boundary, my copy longer than the room's | 150 ms fade-out ending at the boundary |
| Song boundary, crossfade | equal-power crossfade over the set length |
| Volume, sleep timer | per-sample ramp over 20 ms |
| A stream running dry | 30 ms fade-out over the frames still in hand (the guard), then silence |
| Coming back after running dry | from silence, 15 ms fade-in |
| Output switch, device lost and reopened | the old output fades out (if it still works), the new starts from silence with a 15 ms fade-in at the room's place |

#### The guard: a dry ring never makes a hard edge

The renderer never plays the last `guard = 30 ms + 33 frames + 1 period` of a voice whose source has not ended, except as a fade. When a period would need frames inside the guard, it starts the starvation fade over them and then plays silence. So a stream that falls behind fades out by construction. The 30 ms costs nothing: it is part of the decoded depth.

#### Status by atomics

The renderer publishes, through a seqlock and plain atomics: the voices and their content places, device frames written, the current ppm, its state (playing, fading, starved, silent), dry count, and a histogram of deadline margin (event to buffer released, as a share of the period). The status thread reads these 10 times a second, builds `Status` (the app's type, unchanged in shape), and calls the app. Once a minute, and when the output stops, the status thread also writes the histogram to the playback log as one line, so the 7.5 gate can be read on hardware (WP14's `deadline_probe log`): `render: deadline margin over <s> s: <n> periods, ran dry <d>, late <l>; per 1% of the period from 0%: <100 counts>`, the counts space-separated, bucket 0 holding margins from 0 to 1% of the period and late periods (no margin left) counted apart. The format is `crates/player/examples/live_kit/deadline.rs`'s `Histogram::to_log_line`. The app's status callback, which today runs on the sound's thread and takes several locks, spawns threads and writes files (`playback.rs:425-616`), now runs on the status thread.

#### Rules for the render thread, checked by tests

- No allocation (a counting allocator in tests fails the test on any).
- No lock, no syscall except the output's own buffer calls, no logging, no channel that can block.
- Kernel tables (the resampler's) built when the player starts, never lazily.
- Every buffer sized at device open for the largest period.

### 4.8 Output devices and latency compensation

#### The output owns the render thread

Today the engine's one thread calls `wait()` and `write()` on a `Device`. AAudio, like most audio APIs, calls you. So the new `Output` trait is callback shaped (section 5.2): the engine hands the output a renderer, and the output calls `render` from its own real-time thread. This is one model for both platforms.

#### WASAPI (Windows, now)

- The output creates its COM objects on its own render thread (COM objects stay where they were made), registers it with MMCSS "Pro Audio", and runs the event-driven loop: wait for the event (2 s timeout), find the free space, `GetBuffer`, call the renderer straight into the device buffer, `ReleaseBuffer`.
- Exclusive mode: kept as today (formats best first, the aligned-period retry, about 10 ms periods, two buffers).
- Shared mode: event-driven, 10 ms engine period, 50 ms buffer (as now). The format choice is decision D2:
  - **(recommended)** open at the mix format and let our own resampler convert. The device never reopens between songs of different rates (gapless across rates, no device-latency change mid-room), drift and rate conversion are one stage, and Android works the same way;
  - or keep Windows' converter (`AUTOCONVERTPCM`), adding `SRC_DEFAULT_QUALITY`, and reopen between rates as now.
- Presentation: after each buffer, `IAudioClock::GetPosition` with its QPC stamp is published as (device frame at the speaker, QPC time). A reading with `S_FALSE` (delayed) is skipped.
- Device events: an `IMMNotificationClient` reports default-device changes and devices removed; `AUDCLNT_E_DEVICE_INVALIDATED` reports a lost device. Both go to the control thread as `OutputEvent`s, which fades and reopens.
- Stop: the renderer has already faded and written a buffer of silence (the splice table), so the device stops on silence. A start at another place resets the stream first, so no stale samples play.

#### AAudio (Android, later)

- The data callback is the renderer. Shared mode, performance mode none or power saving, a large buffer, no forced sample rate (the mixer is usually 48 kHz; with D2 recommended, our resampler converts).
- Presentation: `AAudioStream_getTimestamp(CLOCK_MONOTONIC)` polled every 20 ms on a small timer thread (not in the callback), published the same way.
- Disconnects come through the error callback and are handled on the control thread, as on Windows.
- Whether Bluetooth's codec delay is inside the timestamp must be checked on the device (the Android readiness spike, check 4).

#### Latency compensation

- **What the device knows** is in the presentation pair: the frame reaching the DAC at a moment. This already includes the device's buffer and the engine.
- **What it may not know** (Bluetooth's codec and the headset's buffer, 34 to 241 ms measured; an AV receiver; an output's sound effects, about 100 ms on the owner's laptop speakers by WP14's loopback measure) is a per-output extra delay, kept by output endpoint id and picked up again on an output switch. Whether Windows' `GetPosition` already includes the Bluetooth delay is not documented (Mozilla saw unpredictable delays on switches), so it is measured, not assumed: on Windows on the owner's hardware first (WP14), and in every room by calibration (4.14). On Android the Bluetooth stack subtracts the headset's own delay report from the timestamp (AOSP source), which is checked on the handheld.
- **Apart**, the extra delay matters little (it is under a call's own latency) and the slider (-500 to +500 ms) is enough; the panel says "Bluetooth: about 200 ms behind" when a value is set. **Every device** mode needs it measured (4.14).
- **Heard time** of the frame at the DAC at local time t is `t + extra`. Lyrics and the spectrum follow the same heard time, so they match what you hear in a room and alone.

### 4.9 Drift and error control

#### Measuring the error, exactly

Every 20 ms the sync tick (control thread, rooms only) computes:

1. Take the newest presentation pair `(f_p, t_p)` and extrapolate to now: `f = f_p + (now - t_p) x device rate`.
2. Find the marker period containing device frame `f`: its voice, its content place at its first frame, its ratio. Content place `c = marker.content + (f - marker.frame) x marker.ratio`. This holds through resampling, crossfades and the device's latency without any guesswork.
3. My content in room terms: `c_room = c / source rate - my offset`.
4. When it is heard: `now + extra`. Where the room is then: `expected = anchor.content + (host(now + extra) - anchor.host)` using the `ClockMap`.
5. `error = c_room - expected` (positive: ahead).

This replaces "written frames minus queued frames minus the room's place", which needed `heard_delay_ms`, the first-sound special cases and the re-plans.

#### The controller

| Part | Same room | Apart |
|---|---|---|
| Filter | median of the last 10 errors (200 ms), then an average with a 1 s time constant | same |
| Feed-forward | `ff = host clock rate against local (the clock map's skew) - DAC rate against local (slope of presentation pairs over 10 s windows)`, so the content advances at the host's rate from the start | same |
| Proportional | 250 ppm per ms, applied while outside the band | 25 ppm per ms |
| Integral | learns what feed-forward misses, at most ±200 ppm, only while the error is under 20 ms (no wind-up during big errors) | same, slower (60 s time constant against 30 s) |
| Band | start correcting at 1 ms, stop at 0.3 ms | start at 10 ms, stop at 3 ms |
| Band widening | the band is at least half the clock's bound (a relay with a 30 ms bound doesn't chase 10 ms of noise) | same |
| Rate limit | ±500 ppm total, gliding at most 1000 ppm per second | same |
| Realign line | over 50 ms for 2 s | over 250 ms for 3 s |
| Settling | no integral and no realign for 400 ms after a start, a switch or a realign | same |
| While suspended | frozen | frozen |
| Approximate copy | uses the apart column whatever the mode | apart |

At 500 ppm, 1 ms closes in 2 s and 20 ms in 40 s; 100 ppm of card drift is held at zero by the feed-forward from the first seconds.

**Never audible.** The 500 ppm cap is under one cent of pitch and stays. The closest open peer, Metrolist, corrects by ±2% speed through a time-stretcher (about 35 cents, a warble on held notes); Syncplay's 0.95x is 89 cents. Neither is acceptable for music, and no setting in Venuta allows more than 500 ppm.

**Every device mode, a device that can't hold.** If a device in every-device mode stays over 5 ms for 30 s (its clock path or output is unstable), it fades out over 1 s rather than smear the room's sound, and says "Can't keep in step here: listening apart". It rejoins at apart quality, or as a remote if it is in the same room as the host's speaker (the person chooses).

**Bit-perfect in rooms.** With feed-forward, the resampler runs at a steady small rate (tens of ppm) for the whole room, so the output in a room is not bit-perfect, and the signal path says so ("Kept in step with the room: +43 ppm"). The resampler at that rate is clean to -90 dB. The other choice is to let it rest at the song's own rate inside the band and correct in bursts (today's apart behaviour), which is bit-perfect most of the time but cycles. Decision D3.

#### When a crossfade realign is allowed

All of these must hold:

1. The error has been past the realign line for the time in the table.
2. More than 3 s from a song boundary (the boundary realigns anyway, from silence or by its transition).
3. The target is in hand: a local file, or a stream with at least 5 s of bytes beyond the target.
4. No other fade, crossfade or transition under way, and none in the next second.
5. At most one realign per 10 s (same room) or 30 s (apart).

**When exactly:** the sync tick looks through the decoded blocks' loudness for the next 2 s for 50 ms under -40 dBFS. If none is found, it keeps looking as more decodes, for up to 10 s (same room) or 30 s (apart), then takes the quietest 50 ms it has seen ahead. The spare voice is prepared at the place the room will be at that frame, and the renderer crossfades there over 40 ms.

### 4.10 Suspensions, buffering and who waits for whom

Modelled on SharePlay's suspensions (its rule: the group waits at play and seek; a participant that stalls or is interrupted is suspended alone and "jumps forward to rejoin"). Jellyfin's opposite rule (a member buffering pauses everyone; a late joiner pauses everyone) is what not to do.

| Moment | Who waits | For how long | Then |
|---|---|---|---|
| Start, resume, seek, skip, jump (a prepare) | the room waits for everyone not suspended, not stepped out, not unable, not a remote | until all are ready, at most 2.5 s | it commits; the late ones join late, alone |
| A natural song boundary | nobody (the next is armed 20 s ahead, so it is ready) | | late ones join the next song late |
| One person's stream stalls mid-song | nobody | | that person fades out (`Suspended(Buffering)`), refills to 5 s ahead of where the room will be, then fades in at the room's place |
| One person's output is lost, switched or busy | nobody | | `Suspended(OutputLost)`, reopened with back-off (0.5, 1, 2, 5 s), latency measured again, then fades in |
| Headphones unplugged, Bluetooth disconnected | nobody | | paused for that person only ("Paused: headphones disconnected. Rejoin"), never the room |
| A computer or phone goes to sleep | nobody | | it fades out and says "sleeping" if it can; on waking it drops every buffered sample, starts a new clock epoch and rejoins as a late joiner (within the 2 minutes, as the same member) |
| Android: a call or another app takes audio focus | nobody | | `Suspended(Interrupted)`, joins when focus returns |
| A person steps out ("Pause for me") | nobody | | `SteppedOut`; Catch up joins at the room's place. "Ask the room to wait" is a request to whoever controls, never a command |
| A person misses three prepares in a row | nobody | | the host is told "Ravi's connection is slow", with "Give Ravi a lighter copy" and "Start with the next song" |
| Every member, the host included, suspended for 3 s | the room holds itself (auto-hold, decision D11) | | resumes through a prepare when anyone is ready again, or on Play |

The host is a member like the others in this table. Mid-song, nobody ever waits for anybody, so one slow connection never stops the music for the others.

### 4.11 Solo mode

Solo playback must be as if rooms did not exist. This is guaranteed by structure:

1. The own program has no field, no flag and no code path that refers to a room. The room program is a separate type, created on attach and dropped on detach.
2. The renderer knows nothing about rooms. It knows voices, commands at device frames, and a rate target that is 0 unless a room program sets it. Solo sets ratios only for device-rate conversion (shared mode with D2), never a trim.
3. The sync tick runs only while a room program exists.
4. Detach drops the room's voices; the own program builds fresh ones. No resampler history, no learned drift, no anchor and no offset carries over (the learned DAC drift per output is kept in a separate per-output store, read only by room programs).
5. Tests prove it (gates in 7.6): after every way of ending a room, the own queue plays bit-identical to a player that never joined a room; no room thread is alive; `status.sync` is `None`.

### 4.12 Status, logging and what the window shows

**Per person, in the room panel** (one short line, plain words):

| State | Line |
|---|---|
| Opening | Getting ready |
| Ready, waiting for the start | Ready |
| InStep, same room | In step, ±2 ms |
| InStep, apart | In step, about ±15 ms |
| Correcting | Catching up |
| Realigning | Catching up |
| Suspended(Buffering) | Buffering |
| Suspended(OutputLost) | Sound output unavailable |
| Suspended(Interrupted) | Interrupted |
| SteppedOut | Stepped out |
| CannotPlay | Not available for you, back with the next song |
| Remote | Listening on Asha's speaker |
| Away | Reconnecting |
| Joining | Joining... buffering 60% |

The "about" figure is the clock bound plus the measured error, never better than the clock allows. Coarse words come first ("In step", "Approximate: different version", "Relayed", "Bluetooth delay not measured"); the milliseconds, the round trip and the path are behind a disclosure. The host also sees each person's copy: "own file", "YouTube Music", "your file, streamed", with "matched" (exact or measured) or "another version (+12 s)" (approximate); a guest on another version can switch to the host's copy with one tap.

**Buttons:** "Leave" for everyone; for the host, "End for everyone" (confirmed) and "Hand over to..." (4.15). While the host is reconnecting, room controls are greyed with "Host reconnecting".

**Every action has a name:** "Asha skipped", "Ravi added 2 songs", "Meera joined", "Ali left" (as today's activity line); each queue item shows who added it.

### 4.13 The three modes in detail

The mode is per person (decision D4), kept in the room view as `Member.mode`. The host's own mode is always the one its speaker plays in.

#### One speaker

- **What it is.** The host's device plays; a person in one speaker mode is a remote. Their Venuta plays nothing at all: their own music fades out and is kept, as for any guest, and no room program is attached to their player (so nothing in it can crackle, and their own queue comes back paused at its place after the room). It shows the room's now playing, queue, lyrics and progress (computed from the anchor and the clock line), and sends presses and additions as their role allows.
- **Why it comes first.** It is what Spotify Jam in person and Apple Music on HomePod or CarPlay ship, and nothing has to stay in sync, so nothing can crackle. Most people in one room want one good speaker, not five phones.
- **When it is offered.** When a guest joins and the transport sees a direct path with a same-network round trip (median under 8 ms), the join screen asks "Play on Asha's speaker" (selected) or "Play on this device too". Over the internet the question is not asked; the guest is apart.
- **The host's player in a one-speaker room** (every other member a remote): no controller runs and the rate is never changed, so the host's output is exactly solo playback. The room's anchor follows the host's player instead: the host re-anchors from its presentation pair whenever the drift between its sound card and its clock moves the heard place by more than 20 ms, which only moves remotes' progress bars. As soon as anyone plays (every device or apart), the host's player follows the room's clock like everyone else's.
- **Volume.** Remotes may change the host's volume only if the host allows it ("Let guests change the volume", off by default); the change is ramped as any volume change.
- **Changing mode.** A remote can tap "Play here too" (becomes every device, or apart for headphones) and back. Changing to playing is a late join; changing to remote is a leave of the audio only (a 1 s fade), not of the room.

#### Every device (same room)

- Everyone's speaker plays and they are heard together, so the 1 to 2 ms target applies.
- Copy order prefers the host's exact file (decision D5); an approximate copy can't take part and is offered "Listen on Asha's speaker" or apart.
- An output must have a known delay to take part (4.14). An output with an unknown delay joins at apart quality with a warning ("Bluetooth delay not measured: you may hear an echo").
- Eligibility: all of that person's paths direct, median round trip under 8 ms over 60 s; it switches that person to apart above 15 ms for 30 s (with a note), never mid-phrase (the gains change; nothing is heard).

#### Apart

As in the rest of section 4. Each person on their own best copy, apart controller gains, honest accuracy in the panel.

### 4.14 Output delay: measured where possible, remembered per output

| Platform and output | Where the delay comes from | Status |
|---|---|---|
| Android, any output | the timestamp already includes the Bluetooth headset's own delay report (AOSP source) | to check on the handheld |
| Windows, wired and built-in | `GetPosition` with its QPC stamp covers the device buffer and the engine, but not (on the owner's laptop) the output's sound effects | checked on hardware in WP14: steady (positions on a line within 0.03 ms), but the laptop speakers' effects add about 100 ms it does not count (`docs/qa/output-delay.md`; a microphone run confirms). An output with sound effects is treated like Bluetooth: its delay must be measured or set |
| Windows, Bluetooth | unknown whether `GetPosition` includes it | WP14's `output_delay` measures it against a microphone, with the owner's headphones and speaker (`docs/qa/output-delay.md`); if it does not, the extra delay must be set |
| AV receivers, HDMI, USB DACs with buffers | not reported anywhere | set by the person |

**The setting now** (decision D13): an extra delay per output, keyed by the output's endpoint id, remembered, and picked up on every switch to that output. The slider runs -500 to +500 ms. Two helpers make it quick:

- **"Line up by ear"** (every device mode): the host's device and this one play alternating short clicks, a beat apart; the person slides until they hear the clicks evenly spaced. No microphone, no recording.
- **A starting value** for Bluetooth outputs: the typical 170 ms (SBC, measured by RTINGS) as a suggestion the person confirms, never applied silently.

**Later** (decision D13): a microphone calibration that plays a chirp from each device at scheduled host times, records it on one device's microphone, and measures each output's full delay. Audio is processed on the device and never kept or sent.

### 4.15 Leaving, ending and handing over

| Action | Who | What everyone hears | Afterwards |
|---|---|---|---|
| **Leave** | a guest | only the leaver: a 1 s fade-out | their own queue back paused at its place; the others see "Ravi left" |
| **End for everyone** (confirmed) | the host | everyone, the host included: a 1 s fade-out | everyone's own queue back paused at its place (decision D1); "Asha ended the room" |
| **Leave, hand over to...** | the host | nothing, for the others; the host fades out | the chosen member hosts the same room with the same timeline; the old host's own queue back paused |
| **Remove** (and block) | the host, on a guest | only that guest: a 1 s fade-out | as Leave, with a neutral message |
| **Host crash, kill or lost network** | | nothing at first: everyone plays on through the current song and the armed next | after 2 minutes, at the end of the song playing, each guest stops cleanly and gets their own queue back paused ("The host went offline") |
| **A guest's network lost for good** | | that guest plays on to the end of the current song | then stops cleanly ("Lost connection to the room") |
| **Idle room** | | | a held room with no action for 60 minutes ends itself, with a notice to the host |

**Hand-over, how.** The host may hand over only to a member who can play the queue's songs without the host's files; if some songs are only the host's, the host is warned ("2 people will lose songs only you have"), and those entries become CannotPlay for those people. Steps:

1. The old host picks the new host. The new host opens its own endpoint and makes an invite with the same room secret (so every guest's per-room key, and so their identity, stays the same).
2. The old host sends `HandOver { to, ticket, room state, anchor and next, the old host's clock line as the new host knows it }` to everyone. The new host converts the anchor and next into its own time with its clock line to the old host (it is a guest, so it has one).
3. Guests connect to the new host as a reconnect (same person, same member id), do a clock burst, and take the converted anchor. They play on throughout (rule 4), so nobody hears anything.
4. The old host fades out and its own queue comes back paused.

An automatic hand-over when the host crashes (to the member present longest) is possible on the same mechanism, but needs everyone to have kept the room state; it is decision D16 and not in the first build.

**After a restart.** A guest whose app crashed or was restarted is offered "Rejoin Asha's room" (automatic after a crash) while the invite is valid; it rejoins as the same member, as a late joiner. A host that restarts is offered "Reopen your room" (decision D8): the room's endpoint key and secret are kept on disk for the invite's 24 hours, and guests still reconnecting find it; the room comes back held at its last place.

**For you:** the room chip keeps its colours (in step in the accent, catching up in the warning colour, reconnecting faded). A press shows at once as "Pausing...", "Moving to 1:35...", "Skipping..." until the commit, so a 1 s wait never feels like nothing happened.

**Log:** every shaped event (start, switch, realign, rate change of over 100 ppm, starvation, suspension, output switch) is a note, written by the log thread as today.

## 5. Interface contracts

These are the contracts agents build against. They are sketches: names and shapes, not final code. Each has a fake (in the simulation harness) so both sides can be built and tested apart.

### 5.1 Time

```rust
/// The platform's monotonic clock, the same one audio timestamps use (QPC, CLOCK_MONOTONIC).
pub trait MonoClock: Send + Sync {
    fn now_ns(&self) -> u64;
}
// Real: from Instant (QPC on Windows, CLOCK_MONOTONIC on Android). Sim: virtual time.
```

### 5.2 Output

```rust
pub trait Output: Send {
    fn name(&self) -> String;
    fn mode(&self) -> Mode;                                      // Exclusive, Shared, Sim
    fn open(&mut self, want: &OpenRequest) -> Result<OutputInfo, OutputError>;
    /// Start calling `render` from the output's own real-time thread.
    fn start(&mut self, render: Box<dyn Render>) -> Result<(), OutputError>;
    /// Stop after the renderer's silence tail has played; hands the renderer back.
    fn stop(&mut self) -> Option<Box<dyn Render>>;
    fn close(&mut self);
    /// The newest (device frame at the DAC, mono time) pair, lock-free.
    fn presentation(&self) -> Arc<Presentation>;
    fn events(&self) -> Receiver<OutputEvent>;                   // Lost, DefaultChanged, Xrun
}

pub struct OutputInfo { pub format: DeviceFormat, pub period_frames: u32, pub buffer_frames: u32 }

pub trait Render: Send {
    /// Fill `out` (device format, interleaved) for the period starting at device frame `at`.
    fn render(&mut self, out: &mut [u8], frames: u32, at: u64);
}
```

The simulated output (`SimOutput`) takes: clock skew (±ppm), wake jitter (0 to 3 ms), position granularity (1 or 10 ms), missed periods, a held buffer that plays on after a stop without a reset, and a lost-device event on demand. The output conformance suite (open, render a sine for 10 s, presentation monotonic and within 1 ms of the frames rendered, silence before stop) runs on every implementation: Sim in CI, WASAPI and AAudio on hardware.

### 5.3 Voices and render commands

```rust
pub struct VoiceFeed  { /* producer side of the block SPSC + the empty-block return */ }
pub struct VoiceTap   { /* consumer side, owned by the renderer */ }

pub enum RenderCmd {
    /// Start `voice` (from silence) so its content frame `content` is at device frame `at`.
    StartAt { voice: VoiceId, at: u64, content: u64, fade_in: bool },
    /// Move from `from` to `to` at device frame `at`, shaped as `how`.
    SwitchAt { from: VoiceId, to: VoiceId, at: u64, to_content: u64, how: Splice },
    /// Fade out and fall silent, starting at device frame `at`.
    StopAt { voice: VoiceId, at: u64 },
    Drop { voice: VoiceId },
    Gain { target: f32 },
    Chain { voice: VoiceId, chain: Box<Chain> },                 // old one comes back on a return queue
}
pub enum Splice { Gapless, Crossfade { frames: u32 }, FadeOutThenIn { out_frames: u32 }, EndFade { frames: u32 } }

pub struct Marker { pub frame: u64, pub voice: VoiceId, pub content: f64, pub ratio: f64 }
```

The rate target is an atomic (`AtomicU64` holding `f64` bits, ppm), written by the sync tick.

### 5.4 The room's hold on the player

```rust
pub trait RoomPlayer: Send + Sync {
    /// The room takes the player: your own program is set aside (kept), a room program begins.
    fn attach(&self, clock: ClockMapHandle, mode: SyncMode) -> Box<dyn RoomHandle>;
}

pub trait RoomHandle: Send {
    fn prepare(&self, prep: PrepId, entry: EntryId, item: Item, room_content_us: i64, align: Alignment);
    fn arm_next(&self, entry: EntryId, item: Item, boundary_host_us: i64, transition: Transition, align: Alignment);
    fn commit(&self, anchor: Anchor);            // start, switch, resume, or a no-op if already there
    fn hold(&self, anchor: Anchor);              // scheduled pause
    fn set_mode(&self, mode: SyncMode);          // every device or apart (per person); a remote detaches instead
    fn set_extra_latency(&self, us: i32);
    fn step_out(&self);
    fn rejoin(&self);                            // catch up at the room's place
    fn status(&self) -> RoomPlayStatus;          // ready, next_ready, sub-state, error, bound, copy class
    /// The room lets go: the room program is dropped, your own program comes back paused.
    fn detach(self: Box<Self>);
}
```

The fake `RoomHandle` records calls and answers readiness on a script, so the conductor and sessions are built before the player side exists. The fake `RoomPlayer` on the other side is a script of room calls, so the player side is built before the sessions exist.

### 5.5 Clock map

```rust
#[derive(Clone, Copy)]
pub struct ClockMap { pub epoch: u32, pub local_ref_ns: u64, pub host_ref_us: i64, pub rate: f64, pub bound_us: u32 }
impl ClockMap {
    pub fn host_of(&self, local_ns: u64) -> i64;
    pub fn local_of(&self, host_us: i64) -> u64;
}
pub type ClockMapHandle = Arc<ArcSwap<Option<ClockMap>>>;   // None until the clock is ready
```

### 5.6 Transport

```rust
pub trait Transport: Send + Sync {
    fn send(&self, to: PeerId, msg: ControlMsg) -> Result<(), Gone>;
    fn send_datagram(&self, to: PeerId, bytes: [u8; 29]) -> Result<(), Gone>;
    fn open_media(&self, to: PeerId, get: Get) -> BoxFuture<Result<MediaStream, Gone>>;
    fn events(&self) -> Receiver<TransportEvent>;   // Message, Datagram, Joined, Lost, PathChanged
}
```

`SimTransport` grows from `room_check.rs`'s `Link`: delay, jitter, asymmetry, loss, bufferbloat episodes, a path change (a step in delay and asymmetry), and cuts.

### 5.7 Protocol v3 (wire)

```
ToHost:  Hello{protocol: 3, name, proof, app}
         Ready{prep, entry} | Unable{prep, entry, why}
         State{entry, sub_state, error_us?, bound_us?, copy: {kind, class}}   (on change, at most every 2 s)
         Mode{one_speaker | every_device | apart}
         Ask{action} | Request{action} (no role: for whoever controls to accept) | Undo | Bye
ToGuest: Waiting | Welcome{you, room} | Refused{why} | Ended{why}
         Room{view}                                   (people with modes, queue of entries, anchor, next, settings)
         Prepare{prep, entry, content_us, purpose}    (Start, Resume, Seek, Skip, Jump)
         Commit{anchor} | Hold{anchor} | ArmNext{prep, next}
         HandOver{to, ticket, anchor, next, clock line}   (4.15)
Datagram: ping {1, id, t1} | pong {2, id, t1, t2, t3}
Streams:  Get{get: "<key>" | "cover:<key>" | "env:<key>", from, to?}
```

`Action` gains entry ids (`Jump { entry }`, `Remove { entry }`, `Move { entry, to }`). `RoomView` gains `transition`, per-member `mode`, and the anchor and next. All strings and lists stay bounded and checked as today.

### 5.8 Alignment

```rust
pub struct Envelope { pub hop_ms: u8, pub windows: Vec<(u32 /* start ms */, Vec<u8>)>, pub pcm_hash: Option<[u8; 16]> }
pub fn envelope(decoded: &mut dyn Iterator<Item = &[i32]>, rate: u32, ch: u16) -> Envelope;
pub fn align(reference: &Envelope, mine: &Envelope) -> Alignment;
pub struct Alignment { pub offset_us: i64, pub class: CopyClass, pub confidence: f32 }
pub enum CopyClass { Exact, Measured, Approximate }
```

### 5.9 The window's view

`View` (today's shape in `together/mod.rs`) gains: per member `{ mode, sub_state, line, copy }`, for you `{ sub_state, pressing: Option<String> }`, the room's `transition`, and per output `{ delay_ms, bluetooth }`. A fixture JSON of every state goes in `app/tests/fixtures` so the window is built and tested against it.

## 6. Use cases

### 6.0 How shipped products do it (from note 07)

Source labels as in note 07: [official], [source] (open code), [RE] (reverse-engineered), [patent], (anecdotal), (inferred).

| Product | What it does for people | What we take | What we do differently, and why | Here |
|---|---|---|---|---|
| Spotify Jam, in person | one output plays (phone, speaker, TV, car); guests are remotes that add, and control if the host allows; per-member listening and controlling flags [official, RE] | **One speaker mode**, offered first on the same network; host toggles for control and adding; "added by" | | 1.2, 4.13 |
| Spotify Jam, remote | each Premium guest plays on their own device; position from `position_as_of_timestamp` on wall clocks; users report seconds to minutes off (anecdotal) | each person plays their own copy | a measured clock line on monotonic time, not wall clocks; tens of ms, not seconds | 4.2, 4.5 |
| Spotify Jam, ending | the host leaving ends it for everyone [official]; a patent resumes each person's own queue where they left it, and promotes a member before the host leaves [patent] | own queue back where it was; hand-over | Leave and End are separate buttons; hand-over is explicit | 4.15 |
| Spotify patents | an early finisher plays silence (a "dummy track") until the group's next start; a joiner computes its offset from the shared timestamp | shorter copies play silence until the boundary | | 4.4 |
| Apple SharePlay (`AVDelegatingPlaybackCoordinator`) | time-anchored Play (rate, item time, host clock time); a seek never resumes by itself; the group waits at play and seek; a stalled or interrupted participant is suspended alone and jumps forward to rejoin; pause can snap everyone to the originator's time; Leave and End are separate [official] | prepare and commit; suspensions; pause snap; Leave and End | we keep playing until a seek's switch rather than pausing for it (D7); our wait is bounded at 2.5 s (Apple publishes no number) | 4.3, 4.4, 4.10 |
| Apple Music on HomePod, Apple TV, CarPlay | one output; guests scan a QR code, wait for approval, then control the shared Up Next [official] | one speaker mode; approval at the door | | 4.13 |
| Jellyfin SyncPlay | a member buffering pauses everyone; a late joiner pauses everyone; it needed a 30 s group-wait timeout in 2026 after rooms stuck forever; drift correction off by default since 2023 [source] | the minimum-delay clock filter and the lead rule | **never** pause the room mid-song for one person; a joiner never disturbs anyone | 4.10 |
| Syncplay | the room follows the slowest member; file mismatches are a warning, never a block; music files always ready [source] | mismatch is a note, not a block | the room never follows the slowest | 4.5, 4.10 |
| Metrolist Listen Together | host only control, approval, kick, transfer host; 15 min reconnect grace; the room does not wait after the first song; ±2% speed correction from 50 ms, hard seek at 750 ms [source] | hand-over; no waiting mid-song; a reconnect grace | 500 ppm cap (±2% time-stretch is audible); a 2 min grace; a disconnected host does not lock everyone out (controls greyed, then the room ends cleanly) | 4.9, 4.15 |
| spotify-listen-together (Spicetify) | a late joiner makes everyone reload; one person's ad locks the room [source] | | the counter-example: one person never disturbs the others | 4.10 |
| Sonos, AirPlay 2, Google Cast, Snapcast | dedicated speakers on deep buffers (0.5 s to minutes) synced within 1 to 2 ms; manual delay sliders per device; one speaker dropping never stops the others [official, measured] | every device mode at 1 to 2 ms; a per-output delay; a device that can't hold leaves alone | our audio is not streamed live, so a 300 ms start lead is enough (the copies are decoded seconds ahead) | 4.9, 4.13, 4.14 |
| Amazon Echo, Bose | Bluetooth outputs are left out of multi-room, or synced only for "optimized" products [official] | Bluetooth in every device mode only with a known delay | | 4.14 |
| Teleparty, Watch2Gether, Discord, Plex | host or shared control; "people are buffering" with a cap; a 30 min idle pause (partly unverified) | roles, an idle end (60 min held) | | 4.3 |

What no vendor publishes: sync tolerances, buffer deadlines, default suspension policies. Every apart-mode figure in this design is from open-source peers or inferred (1.3, 1.4).

### 6.1 Catalogue

Columns: what happens, the tolerance it must meet, what the window shows, and which component handles it. "Lead" and "ready" are as in 4.4. "Clean" means the splice table applies and the click gate (7.1) holds. Every case of note 07's catalogue (C1 to C8) is here; its number is given in brackets where it adds to or changes the case. **Tolerances are inferred design targets unless marked (m)**, which means a measured anchor from 1.4 sets them.

#### A. Opening, joining and the door

| # | Case | Behaviour | Tolerance | Window | Component |
|---|---|---|---|---|---|
| A1 | Host opens a room with music playing [07 1.1] | The host's audio is not touched: the room program is made from the own program in place, the room's first anchor is the host's live place, and guests join it there | 0 ms gap and no shaping for the host | "Room open", invite and QR code | Room, Player programs |
| A2 | Host opens a room with nothing playing | Room rests; the first song added starts it | | "Add songs to start" | Room |
| A3 | Guest joins with a link or code, internet [07 1.2] | A preview first ("Asha's room, 3 listening, now playing X"); then connect, knock, Welcome, clock burst, prepare at where the room will be once ready, fade in over 300 ms at the exact frame | First sound under 5 s direct, 8 s relay | Joining... buffering 60%, then "In step" | Transport, Session, Clock, Conductor |
| A4 | Guest joins on the same Wi-Fi | As A3, direct on the local network | Under 3 s | | Transport |
| A5 | Local only | No relay, no lookup; the invite carries local addresses; online copies unavailable, so own file or the host's stream | | "Local only" | Transport, sources |
| A6 | Hide my IP | Always through the relay; same room unavailable | | Same room switch greyed with why | Transport, Room |
| A7 | Knock, wait, admitted or denied | As today | | Host sees the knock and a notification | Room, Session |
| A8 | Let in without asking | As today | | | Room |
| A9 | Room full, blocked, wrong version, invite ended | Refused with the reason in words, as today | | The reason | Room, Session |
| A10 | Join mid-song [07 1.4] | Catch-up: prepare at the room's place plus the time to get ready, align, fade in over 300 ms; the room never pauses or re-buffers for a joiner | Others: nothing at all; joiner starts within the band | "Getting ready", then "In step" | Conductor |
| A11 | Join while a start is preparing | Joins the prepare; the room may wait for them within the same 2.5 s | | | Room |
| A12 | Join a paused room | Prepared at the held place, silent | | "Paused" | Conductor |
| A13 | Join a resting room (queue ended) | Nothing plays | | "The room's queue has ended" | Room |
| A14 | Already in a room, opens another invite | Asked to leave first (as today) | | | App shell |
| A15 | Rooms nearby on the same network [07 1.3] | Shown as "Rooms nearby" (mDNS); joining one always needs the host's approval; the host has "Visible on this network" (on by default); nobody is pulled in automatically | | "Rooms nearby" list | Transport, Room |
| A16 | Guest on the same network as the host [07 C7] | Asked "Play on Asha's speaker" (selected, one speaker mode) or "Play on this device too" (every device, or apart for headphones) | | the join choice | Session, Window |
| A17 | Joiner prefers a clean start [07 1.5] | "Start with the next song": silence until the room's next boundary, then a clean start, no seek | | option on the join screen | Conductor |
| A18 | Joiner can't get the song [07 1.6] | Stays in the room; joins with the next song; never stalls the room | Others: nothing | "Not available for you, back with the next song" | Conductor |
| A19 | The host alone in its room [07 1.7] | Exactly solo playback (one speaker case, no controller) | Bit-identical to solo | | Player programs |
| A20 | Group size and upload [07 1.8] | At most 10 people; guests on the host's files limited by the host's measured upload (2 x bitrate x guests at most 70% of it); others cost only control traffic | | "Asha's upload is full: you'll stream from YouTube Music" | Transport, sources |

#### B. Playback control

| # | Case | Behaviour | Tolerance | Window | Component |
|---|---|---|---|---|---|
| B1 | Play (first start) | Prepare, wait for ready (2.5 s at most), commit at now plus lead; everyone starts from silence on one frame | Same room: within 1 ms from the first frame with exact copies; apart: within the clock bound | "Starting... waiting for Meera" | Room, Conductor, Renderer |
| B2 | Pause for everyone | Scheduled at now plus the pause lead; everyone fades out from the same content moment | Same room: the fades start within 2 ms of each other | "Pausing..." then "Paused" | Room, Renderer |
| B3 | Resume | Prepare at the held place, commit, start from silence with a fade-in | as B1 | | Room, Conductor |
| B4 | Seek while playing | Everyone keeps playing until the commit, then a 40 ms crossfade to the new place | as B1; heard within 1 s (LAN), 1.5 s (internet) | "Moving to 1:35..." | Room, Conductor, Renderer |
| B5 | Seek while paused | The held anchor moves; nothing sounds; voices re-prepared silently at the new place | | | Room, Conductor |
| B6 | Next, skip | Prepare the next entry; keep playing; at the commit the old fades out over 30 ms, the new starts | as B1 | "Skipping..." | Room, Renderer |
| B7 | Previous | Over 3 s in: this entry from its start (a seek to 0); else the entry before (a skip) | as B6 | | Room |
| B8 | Jump to an entry | As a skip | | | Room |
| B9 | Dragging the seek bar | Only the release is sent | | The bar follows the finger; the music moves on release | Window |
| B10 | Several presses in a row (skip, skip, skip) | Each newer prepare replaces the older; one commit | No more than one switch heard | | Room |
| B11 | Two people press at once | The host applies them in arrival order; the later wins; both in the activity line | | "Meera skipped", "Ali paused" | Room |
| B12 | A press while reconnecting | Held, sent when back if under 10 s old; else dropped with a note | | "Reconnecting: your press will reach the room..." / "Your pause was too late to send" | Session |
| B13 | A press without the role [07 2.1] | A skip or seek becomes a request the host (or anyone with control) can accept with one tap; adding without the role is explained | | "Ravi asks to skip: Accept" / "The host is in control" | Room, Window |
| B16 | Pause for me only [07 2.4] | Local suspension; resume rejoins at the live place with a 300 ms fade-in; "Ask the room to wait" sends a request, never a command | Others: nothing | "Paused for you", "Catch up" | Conductor |
| B17 | Guests changing the speaker's volume, one speaker mode [07 2.8] | Only if the host allows it; ramped | Clean | volume control on remotes | Room, Renderer |
| B14 | Undo (host) | As today: queue back; the song before the change at where it would be now | as B4 | | Room |
| B15 | Media keys, tray, mini player | Go to the room while you are in one (as today) | | | App shell |

#### C. The queue

| # | Case | Behaviour | Tolerance | Window | Component |
|---|---|---|---|---|---|
| C1 | Add songs (end, or next) | Entries with new ids; nothing playing is touched | | "Added..." | Room |
| C2 | Add to an empty or resting room | It starts (prepare) | as B1 | | Room |
| C3 | Remove a song that isn't playing | As today; the playing entry can't be removed (skip it) | | | Room |
| C4 | Move songs, the playing one included | Ids keep the playing entry playing | Nothing heard | | Room |
| C5 | A change makes another song next | The host arms the new next (a new prep id); players drop the old armed voice if it hasn't started | Nothing heard | | Room, Conductor |
| C6 | Change within 3 s of a boundary | The armed next stays for this boundary; the change applies to the one after | | | Room |
| C7 | End of the queue | The last song ends; the room rests | | "The room's queue has ended" | Room |
| C8 | Autoplay additions | Added quietly (as today) before the end | | In the activity line | App shell |

#### D. From one song to the next

| # | Case | Behaviour | Tolerance | Window | Component |
|---|---|---|---|---|---|
| D1 | Gapless, everyone on exact copies | Sample-exact join at each player's own boundary; no message needed at the moment | 0 samples of gap; same room within 1 ms | | Room (ArmNext), Renderer |
| D2 | My copy shorter | Silence until the boundary, then the next | Clean | | Conductor, Renderer |
| D3 | My copy longer | 150 ms fade-out ending at the boundary | Clean | | Renderer |
| D4 | A gapless album on mismatched copies | As D2 or D3 per song; the joins are fades, not cuts | Clean | | Renderer |
| D5 | Room crossfade set | Equal-power crossfade, same moment for all; never within an album | as B1 | "Between songs: crossfade 4 s" | Room, Renderer |
| D6 | Your own crossfade setting | Set aside while in a room; back after | | Sound panel says "the room's setting applies" | Player programs |
| D7 | Next song at another sample rate | Shared mode (D2 recommended): no reopen, converted. Exclusive: the device reopens at the boundary, from silence | Clean; exclusive may add up to 100 ms of silence at that boundary only | | Output, Renderer |
| D8 | Very short songs | Armed at once; still joined at the boundary | | | Room |
| D9 | Someone's next isn't ready in time | They join the next song late, alone | | "Getting ready" for them | Conductor |
| D10 | Someone has no copy of the next | Silence for them during it; back for the one after | | "Not available for you, back with the next song" | Conductor |
| D11 | Nobody can play an entry [07 3.7] | The host skips it after 5 s | | "Skipped X: no one could play it" | Room |

#### E. Copies and alignment

| # | Case | Behaviour | Tolerance | Window | Component |
|---|---|---|---|---|---|
| E1 | Everyone plays the host's file | Exact, offset 0 | Same room within 1 ms | "your file, streamed, matched" | Sources, Alignment |
| E2 | Own lossless copy, same decoded audio | Exact (PCM hash matches) | as E1 | "own file, matched" | Alignment |
| E3 | Own copy from another master or encoding | Measured offset, refined through the song | within 10 ms of the truth | "own file, matched" | Alignment |
| E4 | YouTube Music copy, the host on a file | Measured (encoder delay, intros) | within 10 ms | "YouTube Music, matched" | Alignment |
| E5 | Another edit (radio edit, video intro) | Approximate; aligned at the start as well as can be; boundaries fade | best effort, never a sound for it | "another version" | Alignment, Renderer |
| E6 | The host can't play the song itself | No envelope; everyone approximate for it | best effort | "about in step" | Alignment |
| E7 | No copy anywhere | Can't play this one; back for the next | | | Sources |
| E8 | A catalog stream's address expires mid-song | Refreshed as today (decode.rs) behind the decoded depth | Nothing heard | | Decode threads |
| E9 | Every device mode, a copy only approximate | That person can't take part at 1 to 2 ms: offered the host's exact file, one speaker, or apart | | "another version" | Controller, sources |
| E10 | Another version over 2 s longer or shorter [07 3.2] | Shown to that person and the host (D14) with the difference; one tap switches that person to the host's copy | | "Different version (+12 s): use Asha's copy" | Window, sources |
| E11 | Mobile data (Android) [07 4.4] | Prefer resolving your own YouTube Music stream over pulling the host's FLAC; a low-data switch | | "Using your data" | sources |

#### F. Network

| # | Case | Behaviour | Tolerance | Window | Component |
|---|---|---|---|---|---|
| F1 | Same Wi-Fi, direct [07 4.1] | One speaker offered first; every device available (median RTT under 8 ms over 60 s, all direct) | Every device: p95 1 ms (m: precedence effect) | the join choice | Transport, Room |
| F2 | Internet, direct | Apart | p95 20 ms | "about ±N ms" | Clock, Controller |
| F3 | Internet, relay | Apart | p95 30 ms | | Clock, Controller |
| F4 | Relay to direct mid-song | The clock re-measures (burst) and may step after 2 s; the player takes the change by rate or one crossfade realign | Nothing heard; back inside the band within 30 s | | Transport, Clock, Controller |
| F5 | Jitter, bufferbloat (someone uploads) | Minimum-delay selection ignores the slow samples; datagrams don't wait behind streams | Error stays inside the band | | Clock, Transport |
| F6 | 1 to 5% packet loss | Datagram pings lost are just missing samples; control retransmits | | | Transport |
| F7 | A cut under 2 minutes [07 4.8] | Reconnecting; the guest plays on from its plan by its clock line (skew known) through the current song and the armed next; nothing it hears changes | Drift under 1 ms per minute on a fitted line (m: card drift) | "Reconnecting... the music goes on" | Session, Clock |
| F8 | A cut over 2 minutes [07 6.5] | The room ends for that guest at the end of the song playing (never mid-phrase); then the own queue comes back paused | Clean | "Lost connection to the room" | Session |
| F13 | The host's own network blips [07 4.9] | Everyone plays on; room controls greyed for the grace | Nothing heard | "Host reconnecting" | Session, Window |
| F14 | A clock wrong or jumping on a machine [07 4.10] | Never matters: only the measured line over monotonic time is used, never wall clocks | | | Clock |
| F9 | Guest changes network (Wi-Fi to hotspot) | QUIC migrates or the guest reconnects; a new clock epoch if needed | as F4 | | Transport, Clock |
| F10 | The host's upload is the limit | Host streams paced; control and clock unaffected; the slow guest buffers alone | | "Buffering" for them | Transport |
| F11 | UDP blocked | iroh relays over HTTPS; as F3 | | | Transport |
| F12 | Same room eligibility lost | Off when someone's median RTT is over 15 ms for 30 s; the host is told; players change gains, nothing heard | | "Same room turned off: Ali's connection is too slow" | Room, Controller |

#### G. Buffering and stalls

| # | Case | Behaviour | Tolerance | Window | Component |
|---|---|---|---|---|---|
| G1 | A guest's stream stalls mid-song | Starvation fade (guard), suspended, refills 5 s past the room's place, joins from silence | Clean; back within 1 s of refilled | "Buffering" | Renderer, Conductor |
| G2 | The host's own stream stalls | The same; the room goes on | | | as G1 |
| G3 | Everyone stalls (a catalog outage) | Each suspends; when every member, the host included, has been suspended for 3 s, the room holds itself; resumes together. One person stalling never holds the room | Clean | "Waiting for the music to arrive" | Room (auto-hold) |
| G7 | One person misses three prepares in a row [07 4.6] | The host is told, with "Give Ravi a lighter copy" and "Start with the next song" | | "Ravi's connection is slow" | Room |
| G4 | A slow guest at a start | The room waits at most 2.5 s, then goes; they join late | | "Starting... waiting for Ali" | Room |
| G5 | A seek past what has downloaded | The prepare waits for those bytes (within the 2.5 s), else joins late | | | Decode threads |
| G6 | A stalled stream that never returns | As today: said, and moved on at the next song for that person | | "Can't play this one" | Decode threads, Conductor |

#### H. Devices and output

| # | Case | Behaviour | Tolerance | Window | Component |
|---|---|---|---|---|---|
| H1 | Output switched mid-room | Fade out on the old, open the new, start from silence at the room's place | Clean; sound within 1.5 s; in step at once (exact start) | | Output, Player |
| H2 | Windows' default output changes (Default chosen) | As H1 | as H1 | | Output |
| H3 | Bluetooth headphones, apart [07 5.1] | Platform's delay used where it reports one (Android); else the remembered per-output setting; accepted as is otherwise | under a call's latency (m: G.114) | "Bluetooth: about 200 ms behind" if set | Latency comp |
| H3b | Bluetooth output, every device mode [07 5.2] | Takes part only with a known delay (reported by the platform, or set with "Line up by ear", remembered per output); otherwise joins at apart quality with a warning | 1 to 2 ms once set (m: precedence effect) | "Bluetooth delay not measured: you may hear an echo" | Latency comp (4.14) |
| H4 | Output lost [07 5.5] | `Suspended(OutputLost)`, retried with back-off; latency measured again; fades in at the live place | Clean | "Sound output unavailable" | Output, Conductor |
| H4b | Headphones unplugged, Bluetooth disconnects [07 5.4] | Paused for that person only, never the room | Clean | "Paused: headphones disconnected. Rejoin" | Output, Conductor |
| H12 | Laptop lid closed, explicit sleep [07 5.7] | Fades out, says "sleeping"; on waking drops every buffered sample (no old-buffer blip), new clock epoch, rejoins as the same member within 2 minutes | Clean | | Output, Session, Clock |
| H13 | App minimised, screen off [07 5.6] | The room goes on (Windows; on Android a media playback foreground service) | | | Session |
| H5 | Exclusive mode refused (another app has it) | Falls back to shared (as today) | | Signal path says shared | Output |
| H6 | Volume, mute | Personal; ramped | Clean | | Renderer |
| H7 | Sleep timer in a room | Personal: fades you out over its last 10 s and steps you out | Clean | "Stepped out (sleep timer)" | Player programs |
| H8 | Personal EQ and levelling | Personal; no effect on timing | | | Renderer |
| H9 | Under load (CPU, disk, window resizing, the host serving 3 guests hi-res) | The render thread never waits; deep decoded buffers | 0 dry, deadline margin over 30% at p99.99 | | Render path |
| H10 | Laptop on battery, timer coarse | Event-driven output, not timer-driven | as H9 | | Output |
| H11 | Lyrics and spectrum in a room | Follow the heard place (presentation plus extra delay) | Within 20 ms of what is heard | | Status |

#### I. Leaving and ending

| # | Case | Behaviour | Tolerance | Window | Component |
|---|---|---|---|---|---|
| I1 | Guest leaves [07 6.3] | Bye, 1 s fade-out, detach, own program back paused; nothing changes for the others (no "pause on leave") | Clean; own queue exact (7.6) | "You left Asha's room"; others: "Ravi left" | Session, Player programs |
| I2 | Host removes a guest (and blocks) [07 6.7] | As I1 for the guest, with a neutral message | as I1 | "Asha took you out of the room" | Room, Session |
| I3 | Host ends the room (End for everyone, confirmed) [07 6.1] | Ended to all; everyone, the host included, fades out over 1 s and gets their own queue back paused (D1) | as I1 | "Asha ended the room" | Session |
| I3b | Host leaves but the room goes on [07 6.2] | "Leave, hand over to...": the chosen member hosts the same timeline; guests reconnect to it and play on; a warning if some songs are only the host's | Guests: nothing heard | "Asha handed the room to Ravi" | Session (4.15) |
| I4 | Host app killed or crashes [07 6.4] | Guests reconnect for 2 minutes, playing on (F7); then each finishes the song playing and stops cleanly, own queue back paused | as F7, F8 | "Reconnecting", then "The host went offline" | Session |
| I5 | Host machine sleeps | As I4; on wake within 2 minutes guests reconnect, the clock starts a new epoch, the host re-commits where the room is | Clean | | Session, Clock |
| I6 | Host restarts the app | Offered "Reopen your room" while the invite is valid; same key and invite, the room held where it was; guests still reconnecting find it (decision D8) | | "Reopen your room?" | Session (later phase) |
| I7 | Guest app crashes or is restarted | Own queue restored (never the room's song); offered "Rejoin Asha's room?" while the invite is valid | | Prompt | Session, Player programs |
| I8 | Guest machine sleeps and wakes | Reconnect; new clock epoch; catch up | as A10 | | Session, Clock |
| I9 | Invite validity ends during the room | The room goes on; new joins refused | | | Room |
| I10 | After any ending [07 6.8] | Own queue back, paused, at its place; nothing of the room left; "Save the room's queue as a playlist" offered; plays exactly as alone | Bit-identical (7.6) | | Player programs |
| I11 | A held room idle for 60 minutes [07 6.9] | Ends itself, with a notice to the host | | | Room |

#### J. Modes and personal settings

| # | Case | Behaviour | Tolerance | Window | Component |
|---|---|---|---|---|---|
| J1 | A person's mode changed (one speaker, every device, apart) | Per person (D4); every device needs a direct, same-network path and a known output delay; one speaker to playing is a late join, playing to one speaker is a 1 s fade of the audio only | Nothing heard by others | the mode beside each person | Room, Controller |
| J2 | A mixed room (two speakers at the host's, a friend abroad, a remote in the kitchen) | Every device for the two, apart for the friend, one speaker for the remote, one timeline | Each within their mode's tolerance | | Controller |
| J7 | One speaker room [07 C7] | Only the host plays; remotes see now playing, lyrics and progress from the anchor, and control by role; the host's output is never resampled | Host bit-identical to solo; remotes' progress within 100 ms | "Listening on Asha's speaker" | Session (4.13) |
| J8 | Every device, a device that can't hold the band [07 C7] | Over 5 ms for 30 s: it fades out over 1 s rather than smear the sound, and offers apart or one speaker | (m: precedence effect) | "Can't keep in step here" | Controller |
| J3 | Step out and catch up | Local; catch-up from silence at the room's place | as A10 | "Stepped out" / "Catch up" | Conductor |
| J4 | Listening history and play counts | Room songs are counted as heard, marked as from the room | | | App shell |
| J5 | Android in the background, screen off | The AAudio callback keeps rendering; the session keeps its connection (foreground service) | as on desktop | | Output, Session |
| J6 | Android audio focus lost (a call) | `Suspended(Interrupted)`; joins when focus returns | Clean | "Interrupted" | Output, Conductor |

## 7. Acceptance gates

A work package is done only when its gates pass. A release is done only when all of these pass.

### 7.1 Clicks: zero per hour, four detectors

Every detector proves itself first on injected faults: one dropped frame, one repeated frame, a 1-sample step of -40 dBFS at a peak, a 5 ms gap, a hard cut, a gain step.

1. **Bit-exact with declared edits** (today's counting songs, extended): every output frame equals the source frame, except inside the regions the renderer declared in its notes (fades, crossfades, resampled spans).
2. **Sine lock** (after OboeTester's GlitchAnalyzer): 1 kHz and 15 kHz at -6 dBFS; magnitude and phase fitted over 8 periods and re-estimated every period; any residual over -60 dB outside a declared fade is a click; inside a fade the envelope must change by at most 1/240 per sample.
3. **LPC residual on real music** (after Essentia's ClickDetector): order 12 to 32, 512-sample frames, peaks 30 dB over the robust floor (the clipped mean error power); each hop is predicted by the filter learned on the 512 samples before it, never on a frame that holds the samples it judges (a model learned on a fault whitens the fault too: measured in WP14, a dropped frame then stands only about 30 dB over the floor, against 60 dB and more when learned before it); calibrated on the same tracks played solo; a room run may flag nothing that solo doesn't.
4. **Dip and gap**: 5 ms RMS windows, one starting at every sample (so a 5 ms gap fills one window whole), more than 20 dB under the median of the surrounding 200 ms, away from song boundaries and declared pauses.

Plus today's music-shaped detector (steps, third-difference breaks, hard-edged gaps) stays as a fast first check.

**Gate:** 0 flags from all of them, in every scenario of 6.1 that makes sound, run for one simulated hour each in CI (the simulation runs faster than real time), and on hardware in the live matrix (7.7).

### 7.2 Sync accuracy

| Setting | In simulation (CI) | On hardware (acoustic: two line outs into one stereo USB interface, cross-correlated in 1 s windows) |
|---|---|---|
| Every device, one Wi-Fi, exact copies, wired or measured outputs | p95 within 1 ms, max 3 ms after the first 5 s | p95 within 2 ms, max 5 ms |
| Every device with a Bluetooth output whose delay was set by "Line up by ear" | | p95 within 5 ms (inferred: the ear's lining up is the limit) |
| Apart, one Wi-Fi | p95 within 5 ms | p95 within 10 ms |
| Internet, direct (simulated 20 ms up, 40 ms down, 10 ms jitter, 1% loss) | p95 within the path asymmetry / 2 + 5 ms | p95 within 20 ms |
| Internet, relay | as above | p95 within 30 ms, max 60 ms |
| Measured copies (FLAC against YouTube) | offset within 10 ms of the known offset | within 10 ms |
| Clock | bound shown is never smaller than the true error in 99% of samples | |

### 7.3 Time to sound

| What | Same Wi-Fi | Internet direct | Relay |
|---|---|---|---|
| Join to first sound (copy at hand), p90 | 3 s | 5 s | 8 s |
| Join to first sound, streaming the host's file, p90 | 4 s | 7 s | 10 s |
| Play, seek or skip pressed to heard (all ready), p90 | 1.0 s | 1.5 s | 2.0 s |
| Clock ready after connect | 1 s | 1.5 s | 2 s |

### 7.4 Recovery

| What | Gate |
|---|---|
| A stall | heard again within 1 s of 5 s refilled, at the room's place, clean |
| A cut under 2 minutes | in step within 2 s of the connection back, nothing heard meanwhile |
| The host gone for good | each guest stops cleanly at the end of the song playing after the 2 minutes; never mid-phrase |
| Hand-over | guests hear nothing; in step under the new host within 5 s |
| Output switch | sound within 1.5 s, in step from the first frame |
| Path change relay to direct | nothing heard; inside the band within 30 s |
| Device lost and back | sound within 2 s of the device back |

### 7.5 Cost

| What | Gate |
|---|---|
| Render callback | p99 under 10% of the period at 192 kHz stereo with the resampler on; average under 3% at 48 kHz |
| Deadline margin on hardware | p99.99 over 30% of the period; `ran_dry` 0 in an hour, including under the stress set (two CPU-bound threads, a large file copy, window resizing, the host serving 3 guests hi-res FLAC) |
| App CPU in a room against solo | at most 2 percentage points of one core more |
| Memory in a room against solo | at most 40 MB more |
| Allocations on the render thread | 0 (counting allocator in tests) |
| Control message one-way time while the host streams to 3 guests | p99 under RTT / 2 + 20 ms |

### 7.6 After a room

For every way of ending (I1 to I5, I3b for the old host, I7, I8, I11, F8), for every person, the host included, in simulation:

- your own queue plays bit-identical to a player that never joined (bit-perfect file, exclusive mode, gain 1);
- within 100 ms of the ending, the own program is back, paused, at its place;
- no room thread or task is alive; `status.sync` is `None`; no rate target is set;
- the "where you were" file never held a room song.

### 7.7 The live two-copy test matrix

Run by hand before each release, with two copies of Venuta (two machines, or two installs with two outputs), loopback capture on each (detectors 3 and 4) and, where both outputs can be wired to one interface, the acoustic measure.

**The script** for each network: join; start; three songs through gapless; pause and resume; seek; skip; throttle the guest's network (clumsy) until it buffers; switch the guest's output; then end the room one of the ways below.

| Network | Script | Ways of ending |
|---|---|---|
| N0 same Wi-Fi, one speaker (the guest a remote) | full, pressed from the guest | E1, E2, E9 hand-over to the guest |
| N1 same Wi-Fi, every device, exact copies | full | E1 host ends, E2 guest leaves, E3 host removes guest, E4 host app killed, E5 guest app killed and reopened, E6 cut 20 s then back, E7 cut 150 s, E8 host sleeps, E9 hand-over |
| N1b as N1, the guest on Bluetooth headphones or speaker, delay set by "Line up by ear" | full | E1 |
| N2 same Wi-Fi, apart, guest on YouTube copies | full | E1, E2 |
| N3 internet through the relay (Hide my IP on the guest) | full | E1, E2, E4, E6, E7 |
| N4 internet direct (guest on a phone hotspot) | full | E1, E2 |

**Pass:** 0 clicks in every capture; N0 host output bit-identical to solo (loopback compared with a solo run); N1 acoustic p95 within 2 ms, N1b within 5 ms; the throttled guest buffers alone and the host never pauses; after each ending everyone's own queue resumes paused at its place and plays clean; the playback log has no room lines after the ending.

**Also measured once on the rig (WP14), before every device mode ships:** whether Windows' `GetPosition` includes the Bluetooth delay on the owner's headphones and speaker (acoustic delay against the reported one), by `output_delay` and the steps in `docs/qa/output-delay.md`. The matrix itself, with the exact steps and the playback log lines, is `docs/qa/listen-together-live.md`.

### 7.8 Mode and fairness gates (simulation)

| Gate | How |
|---|---|
| One speaker: the host's output is bit-identical to solo; no rate change ever | every case of 6.1 run with all guests remote |
| One speaker: remotes' progress within 100 ms of the host's heard place | compared in simulation |
| Nobody is ever paused mid-song for one person | in every scenario, the anchor stays `playing` through any single member's stall, suspension, sleep, output switch or drop; only prepares wait, and never over 2.5 s |
| A joiner disturbs nobody | others' output bit-identical with and without the joiner (same copies, same drift) |
| A device that can't hold in every device mode leaves alone | the others' error unchanged when one device's clock is made unstable |

## 8. Keep, rework, delete: file by file

Paths are on `work/together`. "Keep" means kept as it is or moved unchanged; "rework" means its idea or parts stay and its shape changes; "delete" means removed once its replacement has passed its gates.

### crates/player/src

| File | Verdict | What |
|---|---|---|
| `engine.rs` | Rework (split) | Into `engine/control.rs` (command loop, programs, voice slots, device lifecycle, sync tick), `engine/program.rs` (`OwnProgram`, `RoomProgram`), `engine/status.rs` (status thread). Keep: `Player`, `Cmd`, `Status`, `Signal`, `Item`, the reuse of the song decoded ahead on Next and Jump, the equal-power crossfade and raised-cosine ramp math, the tests (adapted). Delete: `follow_room`, `rate_front`, `set_sync`, `fade_out_now`'s `step()` loop, `sync_hold`, `sync_fresh`, `sync_fine`, `settle_until`, `seek_after_fade`, `heard_delay_ms`, `room_place_now`, `skip_front`, `write_silence`, decoding inside `step`, the 20 ms sleep before a reopen |
| `sync.rs` | Rework | Keep: `Resampler` (kernel, glide, landing on a whole frame), `fade_gain`, `SyncNote` (becomes `RenderNote`, extended), `Controller`'s idea (becomes PI plus feed-forward in `control/sync_tick.rs`). Rework: `Resampler::process` without allocation (its `past` Vec goes, and so does the caller's `make_contiguous` on the song's buffer; the history is a fixed ring), with any base ratio for rate conversion. Delete: `SyncPlan`, `is_nudge`, `NUDGE_MS`, `APART_NUDGE_MS`, `Correction`, `Trimmer`, `respace`, `repeat_frames`, `drop_frames`, `resample_into` |
| `output.rs` | Rework | `Device` becomes `Output` (callback shaped, presentation, events). Keep `DeviceFormat`, `SampleKind`, `Mode`, `bit_perfect`, `encode` and `remap` (made to write into a given slice, no allocation). `MemoryDevice` and `Pace` move to `sim.rs` as `SimOutput` with skew, jitter, granularity, held buffer, missed periods |
| `wasapi_out.rs` | Rework | Keep device listing, format candidates, the exclusive aligned-period retry, `ProAudio`, the QPC-extrapolated position. Rework: the output owns its render thread; publishes presentation; device notifications; shared mode at the mix format (D2) |
| `decode.rs` | Rework | Keep `Source`, `SourceFormat`, `FileInfo`, remote download with refresh and the stall watch, `mock_host`, all its tests. Rework: pre-roll on lossy seeks; `GrowBody` as a chunk list; decode a packet only once its bytes are in; a shared body per media key |
| `dsp.rs` | Keep, small rework | `Chain` built on the control thread, handed to the renderer lock-free, processing in place without allocation (to check) |
| `queue.rs`, `spectrum.rs`, `waveform.rs`, `verify.rs`, `remote.rs` | Keep | The spectrum is fed from a tap ring written by the status thread from render notes, not from the render thread |
| new `voice.rs` | New | Voice slots, decode threads, block rings |
| new `render.rs` | New | The renderer (stages, splice, guard, markers) |
| new `analysis.rs` | New | Block loudness, alignment envelopes, PCM hash, correlation |
| new `sim.rs` | New | `SimOutput`, `MonoClock` sim |

### crates/player/tests

| File | Verdict |
|---|---|
| `common/mod.rs` | Keep; moves to a dev crate `crates/audio-check` with detectors 2 to 4 added, so player, together and app tests share it |
| `room_clicks.rs`, `room_flow.rs`, `room_smooth.rs`, `sync.rs`, `room_end.rs` | Rework: same scenarios, driven through `RoomHandle` in the simulation; the parts that test `SyncPlan` semantics are deleted with it |
| `bit_perfect.rs`, `playback.rs`, `pause_seek.rs`, `opus.rs`, `waveform.rs`, `remote_fixture.rs` | Keep, run against the new engine |

### crates/together/src

| File | Verdict | What |
|---|---|---|
| `room.rs` | Rework | Keep knock, admit, deny, remove, roles, activity, undo, queue edits and their tests. Rework: entries with ids; prepare that keeps the current anchor playing; scheduled pause; `ArmNext` with boundary and transition; the reference length in µs; auto-hold; auto-skip when nobody can play; idle end; per-member mode (one speaker, every device, apart; remotes never waited for); requests from guests without the role; hand-over state; lead from p95 RTT |
| `timeline.rs` | Rework | `Timeline` becomes `Anchor` and `Next` (entry ids, µs); `lead_us` and `catch_up_target` kept with new numbers |
| `clock.rs` | Keep, extend | Publishes `ClockMap` (line with skew), bound, epochs, path-change and reconnect step rules |
| `proto.rs` | Rework | Protocol 3 (5.7); keep every check and bound; `Hearing` becomes the player sub-states |
| `invite.rs` | Keep | |
| new `session/host.rs`, `session/guest.rs` | New | The state machines of 4.3, pure, driven by `Transport` events |
| new `conductor.rs` | New | Replaces `follow.rs`: turns the room into `RoomHandle` calls, reports readiness and state |

### New crate `crates/together-net`

From `app/src-tauri/src/together/net.rs` (kept: `Reach`, `endpoint`, `ticket`, `guest_key`, `Path`, `Lines`, `send_line`, the same-room RTT rules), the networking halves of `host.rs` and `guest.rs` (accept, serve, connect, reconnect, the 127.0.0.1 proxy, file and cover streams), plus new: datagram clock tasks, stream priorities, pacing, path events, `Transport` and `SimTransport`.

### app/src-tauri/src/together

| File | Verdict | What |
|---|---|---|
| `follow.rs` | Delete | Replaced by `together::conductor` and `RoomHandle`. Its retry rule for a failing player (`on_failure`) moves into the conductor |
| `host.rs`, `guest.rs` | Rework | Networking to `together-net`; what stays is thin: building the session, the host's files and covers, events to the window |
| `net.rs` | Move | To `together-net` |
| `sources.rs` | Keep, extend | Copy order by mode; returns the copy kind for alignment |
| `covers.rs` | Keep | |
| `room_check.rs` | Rework | Into the simulation harness (whole rooms of 3 to 5 people), every scenario of 6.1 |
| `mod.rs` | Keep, extend | Commands and `View` v2 |

### app/src-tauri/src/playback.rs

| Part | Verdict |
|---|---|
| Status callback | Rework: runs on the status thread; `room_tap` removed (the conductor reads `RoomHandle::status`) |
| `room_load`, `room_queue_next`, `room_sync`, `room_pause`, `room_next`, `claim_room`, `release_room`, `set_room_tap` | Delete (replaced by `RoomPlayer::attach` and `RoomHandle`) |
| `set_aside`, `give_back`, the aside file | Delete (the own program is kept by the engine; "where you were" covers crashes) |
| `room_buttons`, `press` | Keep |
| `describe_sync` | Keep, for the new notes |

### app/src (window)

| File | Verdict |
|---|---|
| `lib/together.ts` | Keep the store and route; rework the view types (entry ids, sub-states, per-member mode, copy, accuracy, pressing) |
| `lib/roomRoute.ts` | Keep |
| `components/together/RoomChip.tsx` | Rework: per-member lines (4.12), the mode per person, the join choice ("Play on Asha's speaker" or "Play on this device too"), the remote view, requests to accept, Leave / End for everyone / Hand over, the room's transition setting, the delay per output with "Line up by ear" and the Bluetooth hint, rooms nearby, rejoin and reopen prompts |
| `components/together/RoomQueue.tsx` | Keep; entry ids |

## 9. Implementation plan

### 9.1 How the work is cut

- **Contracts first.** Work package 0 writes the interfaces of section 5 and the fakes and simulation behind them. After that, agents build each side against fakes, in parallel.
- **Tests first** in every package: the gate tests are written and fail before the code exists.
- **The desktop stays releasable at every merge.** The new engine is built beside the old one and selected by a setting (`player.engine = 2`, default 1) until it passes every gate; a compatibility adapter lets today's room follower drive it, so rooms keep working on either engine. Only then is the default switched, and the old engine deleted one release later.
- **Each package is sized for one agent** (S: up to 2 days, M: 3 to 5, L: 6 to 8), touches its own files, and names the files it may not touch.

### 9.2 Work packages

#### Phase 0 (serial, one agent)

**WP0 Contracts and the simulation harness** (L)
- Writes: the types and traits of section 5 (in new files, not yet used by the app); `MonoClock` real and sim; `SimOutput`; the output conformance suite; `crates/audio-check` with the four detectors and their self-tests; `SimTransport` from `room_check.rs`'s `Link`; fakes for `RoomHandle`, `RoomPlayer`, `Transport`; the window's fixture JSON of every state.
- Tests first: detector self-tests (each injected fault caught; each clean signal passes); `SimOutput` timing (presentation within 1 µs of the model, skew applied).
- Merge: no behaviour change.

#### Phase 1 (in parallel, after WP0)

| WP | Size | Owns these files | Builds against | Tests written first |
|---|---|---|---|---|
| **WP1 Output v2 for WASAPI** | M | `wasapi_out.rs`, `output.rs` | `Output`, `Render` (5.2) | conformance suite on hardware (ignored in CI): sine 10 s, loopback capture clean, presentation monotonic, silence before stop; device-change event fires |
| **WP2 Voices and decode threads** | L | `voice.rs`, `decode.rs` | `VoiceFeed`, `VoiceTap`, `Block` (5.3) | lossy seek with pre-roll equals straight decode from that frame (Opus, AAC, MP3, Vorbis); a stall mid-packet loses nothing (bit compare with a full decode); a seek on a stream doesn't download again; no allocation on the consumer side |
| **WP3 Renderer** | L | `render.rs`, `sync.rs` (resampler only), `dsp.rs` | `Render`, `RenderCmd`, `VoiceTap`, `Marker` | each splice table row clean by all detectors; gapless sample-exact; `StartAt` on the exact device frame; untouched passthrough at ratio 1 and gain 1; starvation fade from the guard; 0 allocations; CPU budget benchmark |
| **WP6 Clock v2** | M | `together/src/clock.rs` | `ClockMap` (5.5) | simulated asymmetry, jitter, bufferbloat, path change, a sleep: error within bound 99%; epochs; step after 2 s on a path change |
| **WP7 Protocol v3 and Room v2** | L | `together/src/{proto,room,timeline}.rs` | 5.7 | today's room tests ported; new tests for every row of groups B, C, D, G (room side) |
| **WP10 Alignment** | M | `analysis.rs` | `Envelope`, `align` (5.8) | known offsets of ±3 s found within 2 ms; encoder delays; an edit detected; repetitive music rejected, not mis-aligned; PCM hash matches only identical audio |
| **WP12 Window v2** | M | `app/src/lib/together.ts`, `components/together/*` | the fixture JSON (5.9) | node tests for the view logic (lines, pressing states, permissions); screenshots of every state |
| **WP14 Hardware test kit** | S | `crates/player/examples/`, `docs/qa/` | | loopback capture tool runs detectors 3 and 4 on a live session; acoustic two-machine measure script; deadline histogram readout; the 7.7 checklist; **measures whether Windows' `GetPosition` includes the Bluetooth delay** on the owner's headphones and speaker (feeds WP19) |

WP8's first step (moving `net.rs` and the networking halves of `host.rs` and `guest.rs` into `crates/together-net`, behaviour unchanged, with characterisation tests first) can also start in Phase 1.

#### Phase 2

| WP | Size | Depends on | Owns | Tests written first |
|---|---|---|---|---|
| **WP4 Engine control, programs, status thread** | L | WP1, WP2, WP3 | `engine/*` (new engine beside the old) | the whole existing player suite run against both engines; the status callback never on the render thread (thread name check); programs switch and come back exact |
| **WP5 Room playback in the engine** | L | WP4, WP6 (types only) | `engine/program.rs` (room), `engine/control.rs` (sync tick) | the room scenarios of today's tests ported to the simulation through `RoomHandle`; 7.2 simulation gates; the compatibility adapter for `Cmd::Sync(SyncPlan)` passes today's `room_clicks.rs` |
| **WP8 together-net** | L | WP6 | `crates/together-net`, `app/.../together/{net,host,guest}.rs` | datagram clock end to end in simulation; priorities hold control latency under a file upload (7.5); pacing; held presses expire; reconnect keeps identity; path events reach the clock |

**Release R1** after WP5: the new engine is the default. Rooms still speak protocol 2 through the compatibility adapter. This release removes the crackle in solo and in rooms (render thread, splice stage, guard, decode fixes) and must pass 7.1, 7.5, 7.6 and the 7.7 matrix.

#### Phase 3

| WP | Size | Depends on | Owns | Tests written first |
|---|---|---|---|---|
| **WP9 Sessions and conductor** | L | WP5, WP7, WP8 | `together/src/{session,conductor}.rs`, `app/.../together/mod.rs`, `playback.rs` room parts | whole rooms (3 to 5 people) in simulation for every case of 6.1; 7.3, 7.4, 7.6 gates |
| **WP11 Alignment in the room** | M | WP9, WP10 | `env:` stream, conductor's alignment, `sources.rs` order by mode | E1 to E6, E9 in simulation; FLAC against a lossy copy on hardware within 10 ms |
| **WP18 One speaker mode** | S | WP9, WP12 | the session's `Remote` mode, the join choice, the remote view (progress from the anchor, lyrics), the host's free-running player with re-anchoring, guests' volume toggle | 7.8 one speaker gates; J7, A16, B17 in simulation. Cheap by design: a remote attaches no player, and the control path is today's room control |
| **WP19 Output delay per output** | M | WP5, WP14's Bluetooth measurement | per-output delay store keyed by endpoint id, picked up on switches; "Line up by ear" (alternating clicks between the host's device and this one); the Bluetooth suggestion; every device mode refuses an unknown delay | H3, H3b, J8; the N1b row of 7.7 |
| **WP13 Removal** | M | WP9 (and R2 shipped) | the old engine, `SyncPlan` adapter, `follow.rs`, aside files, unused `sync.rs` parts | the whole suite green without them |

**Release R2** after WP9, WP11, WP18 and WP19: protocol 3, with the three modes. Both sides must update; a guest on protocol 2 gets the "different versions" message. Must pass every gate in section 7.

#### Phase 4 (after R2, in parallel)

| WP | Size | What |
|---|---|---|
| **WP15 Life cycle** | M | Host reopen after an app restart (D8), guest rejoin prompt and automatic rejoin after a crash, learned drift remembered per output |
| **WP16 Shared mode at the mix format** | M | Decided (D2): our resampler converts to the mix format, no reopen between rates; quality gate -90 dB, CPU gate 7.5. It can move into Phase 2 if WP3 finishes early |
| **WP17 Android readiness** | S | The output conformance suite and presentation polling contract for the AAudio output (built in the Android plan's AAudio package); measures WP3's resampler on the handheld for D12 |
| **WP20 Host hand-over** | M | `HandOver` (4.15): the new host's endpoint with the room's secret, the anchor converted to its time, guests reconnecting as the same members; gate: guests hear nothing (7.4) |
| **WP21 Microphone calibration** | M | Later (D13): chirps at scheduled host times, recorded on one device, each output's full delay measured and stored |

### 9.3 Order and parallel lanes

```
Phase 0      WP0 ─────────────┐
                              │
Phase 1      ┌── WP1 Output ──┤
             ├── WP2 Voices ──┼──► WP4 Engine ──► WP5 Room playback ──► R1
             ├── WP3 Render ──┘                        │
             ├── WP6 Clock ─────► WP8 together-net ────┤
             ├── WP7 Room v2 ──────────────────────────┼──► WP9 Sessions, conductor ──┬► WP11 ──┐
             ├── WP10 Alignment ───────────────────────┼──────────────────────────────┤          ├► R2 ──► WP13
             ├── WP12 Window ──────────────────────────┘ (wired in WP9)              ├► WP18 ──┤
             └── WP14 Test kit (used from R1 on; its Bluetooth measure feeds WP19)   └► WP19 ──┘
Phase 4      WP15, WP16, WP17, WP20, WP21 after R2
```

After WP9, three agents run WP11, WP18 and WP19 in parallel.

- Up to eight agents in Phase 1 (WP1, WP2, WP3, WP6, WP7, WP10, WP12, WP14), plus the start of WP8.
- Phase 2: the audio lane (WP4 then WP5) and the network lane (WP8) in parallel.
- WP9 is the join point; it needs a steady hand and should be one agent.

### 9.4 Rules that keep parallel work safe

- Every package lists the files it owns; touching another package's files needs that package's owner to agree (or waits for its merge).
- Contracts change only by a small PR to WP0's files, announced to all packages; fakes change in the same PR.
- `engine.rs` is not edited by anyone until WP4, and then only by WP4 (the old engine stays as it is until WP13).
- Each merge runs: the Rust suite (both engines while both exist), the window's node tests, the simulation gates of the package, and a manual smoke run (play, queue, YouTube Music search, a room host and guest).

## 10. Risks

| Risk | Effect | What reduces it |
|---|---|---|
| The engine rewrite breaks solo playback (bit-perfect, gapless, exclusive quirks, streams) | The thing most people use gets worse | Built beside the old engine behind a setting; the full existing suite against both; R1 gated on solo characterisation; the owner dogfoods engine 2 before the default switches |
| COM and thread ownership in the WASAPI render thread | Crashes or stalls on device changes | Conformance suite on hardware; COM objects made and released on the render thread; device events through a channel |
| Bluetooth delay unknown to Windows | Bluetooth listeners out of step by 34 to 241 ms (measured range) until it is set | Measured once on the rig (WP14); per-output delay remembered; "Line up by ear"; every device mode refuses an unknown delay; a microphone calibration later (D13) |
| People expect every device to play when on one network | Surprise that only the host's speaker plays | The join screen asks, with one speaker selected and "Play on this device too" beside it; the mode can be changed any time |
| Hand-over moves the room's time base to another computer | A step heard at the hand-over | Guests play on through it (rule 4); the anchor is converted with a clock line the new host already has; gate 7.4 |
| Alignment picks a wrong peak (repetitive music, loops) | A copy off by a beat, steady | Confidence and second-peak rules, several windows that must agree, approximate when unsure; never a jump for alignment alone |
| The clock's floor over the internet (path asymmetry) | Apart sync no better than about half the asymmetry | Honest bound in the panel; datagrams and minimum-delay selection get as close as physics allows |
| The host's upload carries several hi-res streams | Guests on host streams buffer | Pacing, priorities, copy order (own and catalog first when apart); the panel says who is buffering |
| Simulation passes, hardware fails | False confidence | The live matrix (7.7) gates every release; the output conformance suite on hardware |
| No quiet moment for a realign in dense music | A realign mid-sound | A 40 ms equal-power crossfade of one song at two close places is a soft smear, not a click; it is rare (only far errors), limited to one per 10 to 30 s |
| Protocol 3 breaks rooms between versions | Friends on different versions can't join | A clear "different versions" message (exists); R1 keeps protocol 2 |
| AAudio timestamp behaviour (and callback rules) differs from WASAPI | Android sync worse than planned | Presentation polled off the callback; conformance suite on the handheld early (Android spike) |
| Scope: lifecycle extras (reopen, rejoin) grow | Delays R2 | Kept in Phase 4, after R2 |

## 11. Decisions

### 11.1 Decided (2026-10-21, by the owner's delegation)

| # | Question | Decided |
|---|---|---|
| D1 | When the host ends the room, what does everyone hear? | The music fades out (1 s), and each person's own queue, the host's included, comes back paused where it was |
| D2 | Shared mode: who converts the sample rate? | Our resampler converts to the mix format: no reopen between songs of different rates, one stage for rate and drift, the same on Android |
| D3 | In a room, steady resampling at the measured drift, or bursts? | Steady: the resampler runs all through a room; bit-perfect only outside rooms (and in one speaker mode for the host) |
| D4 | Same room per person or for the whole room? | Per person; mixed rooms allowed |
| D5 | In every device mode, prefer the host's exact file? | Yes |
| D6 | The room's transition setting in a room, your crossfade back with your queue? | Yes |
| D7 | A seek or skip keeps everyone playing until the switch? | Yes (note 07 suggested passing through silence; keeping the music going is closer to a normal player) |
| D8 | A host can reopen its room after restarting the app? | Yes; the room's key kept on disk for the invite's 24 h |
| D9 | Pauses scheduled ahead so speakers fall silent together? | Yes, 150 to 400 ms ahead |
| D10 | Presses held while reconnecting expire? | Yes, after 10 s |
| D11 | The room holds itself when everyone is suspended? | Yes, after 3 s, and only when every member, the host included, is suspended; never for one person |
| D12 | Which resampler? | Keep and extend our tested Kaiser sinc (no new dependency), unless WP3 or WP17 measures it too slow on the Android target; then `rubato` |
| D13 | Microphone calibration for output delay? | Later (WP21); a remembered per-output delay setting now (WP19), with "Line up by ear" |
| D14 | Who sees "another version"? | The person it affects and the host, not everyone |
| D15 | One speaker mode? | Yes, offered first when a guest is on the host's network, beside every device; apart for other places (directed with note 07) |
| D16 | When the host crashes, hand the room over automatically? | Not in the first build; guests finish the song and stop cleanly. Automatic hand-over can come on WP20's mechanism later |
| D17 | Reconnect grace for a guest | 2 minutes |
| D18 | A held room ends itself after | 60 minutes without an action |
| D19 | Rooms visible to others on the same network | On by default, and only while the room is open; joining always needs the host's approval |
| D20 | Guests may control playback by default | On for rooms of up to 4 people, off above; the host can change it |

### 11.2 Still open

None. Every decision above is taken; a measurement in WP14 or WP17 that contradicts one reopens it.

## Appendix A: every parameter in one table

| Parameter | Value | Section |
|---|---|---|
| Rate change cap | ±500 ppm | 4.9 |
| Rate glide | 1000 ppm per s | 4.9 |
| Fade in / fade out | 15 ms / 30 ms, raised cosine | 4.7 |
| Entrance mid-song (join, rejoin, catch-up) | 300 ms fade-in | 4.4, 4.7 |
| Leaving, removed, room ended | 1 s fade-out | 4.7, 4.15 |
| Seek or realign crossfade | 40 ms, equal power | 4.7 |
| Longer copy end fade | 150 ms ending at the boundary | 4.4 |
| Gain ramp | 20 ms per sample, raised cosine | 4.7 |
| Guard | 30 ms + 33 frames + 1 period | 4.7 |
| Decoded depth | local 2 s, streams 3 s | 4.6 |
| Ready for a prepare | local 2 s decoded; stream 3 s decoded and 10 s of bytes ahead or all; alignment or 1 s | 4.4 |
| Rejoin after a stall | 5 s past where the room will be | 4.10 |
| Ready wait at a prepare | 2.5 s | 4.10 |
| Start lead | max(floor, 2 x p95 RTT + 50 ms), floor 300 ms one network / 500 ms otherwise, at most 2 s | 4.4 |
| Pause lead | 150 ms one network; max(150 ms, 2 x RTT) up to 400 ms otherwise | 4.4 |
| Next song armed | 20 s before the boundary (at once for songs under 20 s) | 4.4 |
| Exact gapless window | 5 ms | 4.4 |
| Clock pings | burst 20 x 50 ms (connect, reconnect, path change), then 1 Hz | 4.2 |
| Clock window, fit | 64 samples; fit after 8 over 30 s; slew 50 µs/s | 4.2 |
| Clock step | over 5 ms (same room) / 20 ms (apart) lasting 10 s; 2 s after a reconnect or path change | 4.2 |
| New clock epoch | a sample over 500 ms off | 4.2 |
| Sync tick | 20 ms, rooms only | 4.9 |
| Error filter | median of 10, then 1 s average | 4.9 |
| Same room band, gain | in at 1 ms, out at 0.3 ms; 250 ppm per ms | 4.9 |
| Apart band, gain | in at 10 ms, out at 3 ms; 25 ppm per ms | 4.9 |
| Integral | ±200 ppm, only under 20 ms of error; 30 s / 60 s | 4.9 |
| Realign line | 50 ms for 2 s / 250 ms for 3 s | 4.9 |
| Realign spacing | one per 10 s / 30 s | 4.9 |
| Quiet moment | 50 ms under -40 dBFS, looked for 10 s / 30 s | 4.9 |
| Settle after a start or switch | 400 ms | 4.9 |
| Same room on / off | median RTT under 8 ms over 60 s, all direct / over 15 ms for 30 s | 4.1, F12 |
| Away, idle timeout, reconnect | 5 s / 15 s / every 2 to 5 s for 2 minutes | 4.1 |
| The host gone | guests stop at the end of the song playing after 2 minutes | 4.15 |
| Held press age | 10 s | 4.1 |
| Auto-hold | every member, the host included, suspended for 3 s | 4.10 |
| Auto-skip | nobody can play the entry for 5 s | 4.3 |
| Idle room end | held 60 minutes without an action | 4.3 |
| Slow member note | three prepares missed in a row | 4.10 |
| One speaker re-anchor | when the host's heard place moves 20 ms from the anchor | 4.13 |
| Every device: can't hold | over 5 ms for 30 s, fades out alone | 4.9 |
| Host file pacing | 10 s burst, then 2 x the song's bitrate | 4.1 |
| Stream priorities | control 100, envelopes 50, files 0, covers -10 | 4.1 |
| Envelope | 10 ms hop, 4 bands, first 30 s then 10 s every 60 s | 4.5 |
| Alignment confidence | peak 8 sd over the mean and 1.3 x the next peak 50 ms away | 4.5 |
| Offset refinement | under 20 ms change: glide; over: approximate | 4.5 |
| Voice slots | 3 | 4.6 |
| Lossy seek pre-roll | Opus 80 ms; AAC, Vorbis, MP3 one packet | 4.6 |
| Download chunks | 64 KiB | 4.6 |
| Shared-mode buffer | 10 ms period, 50 ms buffer | 4.8 |
| Output delay | -500 to +500 ms, per output endpoint, remembered; Bluetooth suggestion 170 ms (SBC, measured) | 4.8, 4.14 |

## Appendix B: words used here

- **Anchor:** "the reference copy's content time C is heard at host time T", plus whether it moves.
- **Boundary:** the host time at which the next entry begins (or its crossfade begins).
- **Clock map:** a line that turns this computer's time into the host's.
- **Conductor:** the part of each person's Venuta that turns the room into calls on its player.
- **Copy:** the file or stream a person plays for an entry. **Reference copy:** the one the room's times are measured on.
- **Entry:** one song in the room's queue, with an id that never changes.
- **Guard:** the last few milliseconds of decoded audio the renderer keeps so it can always fade out.
- **Marker:** what the renderer writes each period: which device frame carried which content place.
- **Prepare / commit:** get a place ready everywhere, then start it everywhere at one moment.
- **Presentation:** the device's report of which frame is at the DAC at which moment.
- **Program:** what the player plays from: your own queue, or a room.
- **Suspension:** one person faded out locally while the room goes on.
- **Voice:** one decoded stream (a song from a place) feeding the renderer.
