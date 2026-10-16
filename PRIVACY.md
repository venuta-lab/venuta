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
| The online catalog | YouTube Music (and YouTube's servers to play a song) | When you search (online results appear below your library) or play an online song |
| Signing in to Spotify (optional) | Spotify's sign-in and API | Only if you sign in, in **Settings › Online sources**, to bring in your own playlists |
| Bringing in a playlist from a link | The site of that link: Spotify, Apple Music, YouTube Music, TIDAL, JioSaavn, Deezer, ListenBrainz | When you paste the link and bring it in |
| Finding your songs on a service (Send out) | Deezer, iTunes, ListenBrainz, MusicBrainz | When you send a playlist out to a service |
| Checking for updates | GitHub (venuta-lab/venuta releases) | At most once a day, and only if **Check for updates when Venuta starts** is on (**Settings › About**); or when you click **Check for updates** |
| The stream tool (yt-dlp) | GitHub (yt-dlp releases) | When the online catalog needs it and it isn't installed |

These requests carry only what the lookup needs (for example an album's title and artist, or the
link you pasted), plus Venuta's version in the User-Agent. Each service's own privacy policy
applies to what it receives.

## Questions

Open an issue at https://github.com/venuta-lab/venuta/issues.
