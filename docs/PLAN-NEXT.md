# Next: everyday use, then sharing

Agreed 2026-10-13, built one step after another. Each step is ticked when built **and** checked
in the running app.

## 1. Windows integration
- [x] **One Venuta at a time.** Opening it again brings the open window forward (from the tray
      too) instead of starting a second player.
- [x] **Tray icon.** Click: show Venuta. Menu: play/pause, next, previous, mini player, show,
      quit. The tooltip says what's playing.
- [x] **Closing the window.** Asked the first time (keep playing in the tray, or quit), then
      remembered; changeable in Settings, Startup and tray.
- [x] **Start with Windows** (off by default): starts in the tray, without a window.
- [x] **Windows media flyout and media keys** (existed, but showed no title: the cover address
      broke every update; fixed): artwork for every album (embedded art,
      not only `cover.jpg`), "stopped" when nothing is loaded, checked by reading what Windows
      itself shows.

Checked in the app: a second launch hands over and exits, and brings a hidden window back; the
first close asks and remembers; --in-tray starts hidden; Windows reports title, artist and
artwork (folder image and embedded). Left for him to try: clicking the tray icon and its menu
(the icon exists, but the taskbar could not be captured), and the Start with Windows switch
(turning it on writes a sign-in entry, so it stays his to flip).

Also fixed on the way: saving in Settings wrote back the copy the screen loaded, so it could
undo a change made elsewhere; now only the changed fields are written.

## 2. Volume levelling for the whole library
- [x] Measure loudness (EBU R128) for every song as a background job with progress and Stop;
      keep results so nothing is measured twice; new music is measured when it comes in. (The
      deep check already measured; Health now offers "Measure loudness", and new music is
      measured after an import once the rest of the library is.)
- [x] Levelling uses the measurements (tags first, then Venuta's own, so nothing is written to
      the files); Health's "Volume levelling" reaches 100%. Checked: 23 songs measured in 19 s
      in a debug build, the signal path says "Volume levelled (song, -10.9 dB)". His library
      (D:\Music) has no measurements yet: the first full run is his to start, in Health.

## 3. Devices, end to end
- [x] Playlists go to cards, written the way each player reads them: each card gets its own
      copy pointing at its own music folder (not assumed "Music"), with only the songs it
      carries; empty ones stay off; Venuta's own lines are left out. The player test decides
      the style: relative (default), from the top of the card, or none. Checked by syncing the
      scratch library to a folder as a Poweramp card (36 entries, all resolve; re-sync changes
      nothing; edits re-copy only that playlist).
- [ ] The player test kit, run on his real cards (needs him and the cards).

## 4. Release and sharing
- [x] GitHub `theSelfSa/venuta` (private for now; he publishes it), README rewritten with
      screenshots of his library (no lyrics in them), signed NSIS installer.
- [x] Updates: the app checks GitHub at most once a day (switchable in Settings, About) and
      installs only when asked. Signed with a key kept in `%USERPROFILE%\.venuta-signing\` (back
      it up). Checked against a local server: a signed 0.1.1 was found and verified, a version
      mismatch and a tampered file were refused. It can only find releases once the repository
      is public. scripts/release.mjs makes a release (draft unless --publish).
- [x] Code signing: his decision, unsigned for now; the README explains the SmartScreen warning.

Also found while taking screenshots: the first read of a big library failed with "database
is locked" (several parts opened the new index at once, with no busy timeout) and the Music
screen then said "Reading…" forever. Fixed both: a 30 s busy timeout, and a "couldn't be
read" screen with Try again. Health's Tools panel ran off the window when its text grew.

## 5. Online sources (later)
- [ ] Your own server (Navidrome / OpenSubsonic).
- [ ] Direct playlist import from Spotify, TIDAL, YouTube (needs developer keys from him).
