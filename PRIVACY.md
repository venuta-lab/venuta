# Privacy

Venuta has no account, no analytics, no telemetry and no ads. It never sends anything about you
or your library to us; there is no Venuta server.

Your settings, playlists, lyrics timings and listening history live in your library folder
(`.venuta`, beside your music) and on your PC.

## When Venuta connects to the internet

Only for something you asked for, or the update check you can switch off:

| What | Where it connects | When |
|---|---|---|
| Looking up details (years, composers, lyricists, covers, synced lyrics) | MusicBrainz, the iTunes Search API, the Deezer API, LRCLIB; Discogs and AcoustID only if you add your own keys | When you run **Look up details online** or bring in new music |
| The online catalog | YouTube Music (and YouTube's servers to play a song) | When you search (online results appear below your library), play an online song, or bring in or open a playlist with songs you don't have (to play them online in their places) |
| Signing in to Spotify (optional) | Spotify's sign-in and API | Only if you sign in, in **Settings › Online sources**, to bring in your own playlists |
| Bringing in a playlist from a link | The site of that link: Spotify, Apple Music, YouTube Music, TIDAL, JioSaavn, Deezer, ListenBrainz | When you paste the link and bring it in |
| Finding your songs on a service (Send out) | Deezer, iTunes, ListenBrainz, MusicBrainz | When you send a playlist out to a service |
| Checking for updates | GitHub (venuta-lab/venuta releases: the update file, and if that fails the list of releases) | Soon after Venuta starts and every few hours while it runs, only if **Check automatically** is on (**Settings › System › Updates**) and Local only is off; or when you click **Check for updates** |
| The stream tool (yt-dlp) | GitHub (yt-dlp releases) | When the online catalog needs it and it isn't installed |
| YouTube's security check (only with **Count plays**) | YouTube and Google (www.youtube.com, www.google.com, jnn-pa.googleapis.com), never as your account | When Venuta first tells a play, then about twice a day while it keeps telling them |

These requests carry only what the lookup needs (for example an album's title and artist, or the
link you pasted), plus Venuta's version in the User-Agent. Each service's own privacy policy
applies to what it receives.

## Signing in to YouTube Music (optional)

You sign in in Google's own window, in **Settings › Online**; Venuta never sees your password.
The sign-in is kept in Windows Credential Manager, never in a file, and signing out forgets it.

After you sign in, Venuta asks once, for each account and channel, what it may do with it. Every
choice can be changed later in **Settings › Online**.

What Venuta reads with your account (on unless you turn them off):

- **Recommendations for me**: your Home, radio, Up next and mixes come from your account. Off,
  Venuta asks for YouTube Music's general ones.
- **My library in Venuta**: your playlists, artists, albums, songs, uploads and podcasts show in
  Library.
- **Linked playlists stay in step**: a playlist you linked to YouTube Music syncs when you open
  it and at most every 30 minutes.

What Venuta tells YouTube Music (off until you turn them on):

- **Count plays**: after 30 seconds of an online song, Venuta tells your account you played it,
  so it shows in your YouTube Music history. To tell it the way YouTube Music's own web player
  does, Venuta passes YouTube's security check in a hidden window of its own, which has no
  sign-in and nothing of yours in it. If Venuta can't pass the check, it tells YouTube through
  its web client instead, and the play shows in your YouTube watch history. Your own files are
  never sent.
- **How much I listened**: with Count plays, also how many seconds of each online song you heard,
  as YouTube Music's own app reports it.
- **Likes and dislikes**: a like or thumbs down you press in Venuta is the same on your account.
- **Signed-in search**: the words you type in the Search box go as your account, so they appear
  in your YouTube search history. Venuta's own look-ups (finding a song for your file, a link's
  song) always go without your account.
- **New YouTube likes come in**: songs you like elsewhere join Liked songs in Venuta when it
  starts. Nothing is sent.

Changes you make to your YouTube Music account in Venuta (renaming, adding, removing, voting on
a song in a collaborative playlist, uploading a file you picked, saving a song to your library or
pinning it, Not interested, deleting an upload, a cover, the artists you choose) and plays or
searches you take out of your YouTube Music history are sent only when you make them. Nothing is
uploaded or deleted on its own.

When you open the menu of an online song that was found without your account (a search with
Signed-in search off), Venuta asks YouTube Music, as your account, for that one song's menu so
you can save or pin it. Only the song's id is sent, never the words you searched for, and it is
asked only when you open that menu.

## Questions

Open an issue at https://github.com/venuta-lab/venuta/issues.
