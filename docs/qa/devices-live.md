# Output devices: the guided test on your own devices

What [the design](../design/devices.md) promises, checked by ear on the devices you have. The
simulation gates (`cargo test -p player --test devices`) pass before this; this is what only
real hardware can show. Each section takes a few minutes. Write what you saw in the table at
the end, and send the playback log with it.

## Before you start

- The build under test, with nothing else playing. System volume low, and the Bluetooth
  devices charged and paired.
- **A test album** of your own lossless files, with at least one song at 44.1 kHz, one at
  96 kHz and one at 192 kHz (24-bit) if you have them, and one song with synced lyrics.
- **Where to look**: the quality badge beside the song (click it: the signal path, then "Play
  through" with the devices), and Settings > Playback > Play through (the same picker).
- **The playback log**: `<library folder>\.venuta\logs\playback.log`. Every time the music moves
  to another output it writes a line `output: on <device> (<kind>, <mode>, <rate> Hz): <note>`.
  Note the time you start.

What counts as a fault anywhere below: a pop, click or burst of loud sound when the music
moves; music that stops and needs Play pressed (unless a section says so); old sound from
before a move; the wrong device in the badge.

## 1. Laptop speakers (Realtek, with Nahimic)

1. Settings > Playback: **Follow Windows** on, **Exclusive** on. Play the 44.1 kHz song.
2. Open the badge. Expect: the list grouped as Bluetooth, USB, Wired, HDMI and DisplayPort,
   Virtual devices; **Speakers** under Wired with "Windows' default" and a green dot (playing);
   under it "Bit-perfect, exclusive, 16-bit, 44.1 kHz" (or 24-bit for a 24-bit song).
3. Play the 192 kHz song. Expect either bit-perfect at 192 kHz, or "Exclusive, 24-bit,
   48 kHz, resampled" with "Resampled to 48 kHz" in the signal path (the speakers don't take
   it): no gap longer than a moment between songs, no pop.
4. Turn **Exclusive** off. Expect "Shared with other apps, 32-bit float, 48 kHz" and a measured
   delay of about 70 ms. (Nahimic's effects play only in shared mode.)
5. The **Nahimic** devices: they are listed under Virtual devices, never ticked, never green.

## 2. Wired headphones in the jack

1. Follow Windows on, a song playing on the speakers.
2. Plug the headphones in. Expect: if Windows moves its default to Headphones, the music fades
   out of the speakers and in on the headphones within about 2 s, from where it was. If Windows
   keeps the speakers, nothing moves (choose Headphones in the picker to move it).
3. Choose **Headphones** in the picker (Follow Windows turns off). Unplug them. Expect: the
   music **pauses** where it was, with "Paused: Headphones disconnected." under the player, and
   nothing comes out of the speakers. Plug them back and press Play: it plays on the headphones.
4. Settings > Playback: turn **Pause when headphones or Bluetooth disconnect** off. Play on the
   headphones and unplug them again. Expect: the music goes on from the speakers within about a
   second, at the same place, starting quietly (a fade in, no burst), with the note "Headphones
   isn't connected: playing on Speakers until it's back." Plug them back: after about 2 s it
   fades back to the headphones. Turn the setting on again.

## 3. SNOWSKY DISC (USB DAC)

1. Plug it in. Choose **SNOWSKY DISC** in the picker, Exclusive on.
2. Play the 44.1 kHz, 96 kHz and 192 kHz songs in turn. Expect each "Bit-perfect, exclusive"
   at its own rate where the DAC takes it (the device line under its name then shows what it
   takes, for example "exclusive 44.1 to 384 kHz, up to 32-bit"); a rate it lacks says
   "resampled" and "Resampled to ..." in the signal path.
3. While a song plays, **pull the USB cable**. Windows calls the DAC headphones, so with the
   pause setting on (the default) expect the music to **pause** where it was, with "Paused:
   SNOWSKY DISC disconnected." Plug it back and press Play: bit-perfect on the DAC again.
4. Turn the pause setting off and pull the cable again. Expect: within about a second the music
   goes on from the speakers at the same place, from quiet, with "SNOWSKY DISC isn't connected:
   playing on Speakers until it's back." It must not stop. **Plug it back**: about 2 s later it
   fades out of the speakers and back in on the DAC. Turn the setting on again.
5. Play a video in a browser while Venuta has the DAC in exclusive mode: the video is silent
   (Venuta has the device). Turn Exclusive off: both play.

## 4. AirPods Pro

1. Connect them. Expect **one** entry, "AirPods Pro", under Bluetooth (not a second
   "Hands-Free" one).
2. Choose it (Exclusive on). Expect "Bluetooth, re-encoded by the link (48 kHz)", not
   bit-perfect, "Not exclusive: Bluetooth plays shared" in the signal path, and a measured
   delay (often 100 to 250 ms).
3. **The delay.** Play the song with synced lyrics, lyrics open. Press **Use 170 ms**. Expect
   the lines to change with the singing; move the slider until they do exactly. Switch to the
   speakers: their delay shows nothing set. Back to the AirPods: your value is back.
4. **The hands-free trap.** Windows Settings > System > Sound: set the output to "Headset
   (AirPods Pro Hands-Free)". In Venuta turn Follow Windows on. Expect the music to stay full
   quality, with the note "Playing on AirPods Pro's stereo connection: its hands-free one is for
   calls". Set Windows' output back.
5. **Gone and back.** Put them in their case while playing (AirPods chosen). Expect the music to
   pause, with "Paused: AirPods Pro disconnected.", and nothing from the speakers. Take them
   out, wait for them to connect, press Play: on the AirPods again.

## 5. FiiO EH11

1. Connect; one entry "FIIO EH11" under Bluetooth. Choose it; same state as the AirPods.
2. With both Bluetooth devices connected, choose one, then the other, while playing. Expect a
   short fade out and in each time, never both at once, never a pop.
3. Switch the FiiO off while playing: paused, as for the AirPods; on again and Play: back.

## 6. soundcore Motion X600

1. Connect; one entry under Bluetooth. Choose it and play.
2. Press **Use 170 ms** and judge with lyrics, as for the AirPods (a speaker's delay is often
   longer).
3. Switch it off while playing: paused ("Paused: soundcore Motion X600 disconnected."). With the
   pause setting off: on the speakers, and back to the speaker about 2 s after it reconnects.
4. If Windows ever shows only "Headset Earphone (soundcore Motion X600 Hands-Free)" (its stereo
   service off), Venuta plays on it with the warning "connected for calls only ... music sounds
   poor". It should never pick it silently.

## 7. NVIDIA HDMI (the monitor)

1. Choose **XG32UCWG** (or M32UC) under HDMI and DisplayPort, Exclusive on, and play.
2. Turn the monitor off with its power button (or let it sleep). Expect the music on the
   speakers within about a second, with "XG32UCWG isn't connected: playing on Speakers until
   it's back."
3. Turn it on. Expect the music back on the monitor about 2 s after Windows sees it again, even
   if the sound device comes and goes a few times while the monitor wakes (no back and forth).

## 8. Sleep and wake

1. Choose SNOWSKY DISC, play, then **Start > Power > Sleep** (or close the lid).
2. Wake it. Expect: if the DAC is back at once, the music carries on (from where it was heard,
   give or take a fraction of a second), never with a burst of the old sound. If it is not back
   yet, the music is paused ("Paused: SNOWSKY DISC disconnected.", the DAC being headphones);
   with the pause setting off it plays on the speakers until the DAC is back, then on the DAC.

## 9. Windows changes the format while playing

1. Follow Windows on, Exclusive **off**, playing on the speakers.
2. Windows Sound settings > Speakers > Properties > Advanced: change the default format (for
   example 48000 Hz to 44100 Hz) and press Apply.
3. Expect a short break (well under a second), then the music goes on at the same place, from
   quiet; the badge's format shows the new rate. Set it back.

## Results

| Date | Section | Device | Passed | What you heard or saw (and the log's time) |
|---|---|---|---|---|
| | 1 | Laptop speakers | | |
| | 2 | Wired headphones | | |
| | 3 | SNOWSKY DISC | | |
| | 4 | AirPods Pro | | |
| | 5 | FiiO EH11 | | |
| | 6 | soundcore Motion X600 | | |
| | 7 | NVIDIA HDMI | | |
| | 8 | Sleep and wake | | |
| | 9 | Format change | | |
