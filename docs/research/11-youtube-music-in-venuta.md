# 11: YouTube Music in Venuta (for songs you don't have, and for people with no collection)

Asked 2026-10-13: "We need some wrapper for YouTube Music so people who don't have a collection can
also use it, or I can discover new music through it and then get my own music if I like it. For
example, for playlists where we don't have the actual files we can use YouTube Music there, and
if I like that song I can add it to my collection."

Read 2026-10-13. A hard line kept from earlier decisions: **Venuta never downloads or extracts audio
from YouTube** (against YouTube's terms and against Venuta's own rules). "Add to my collection"
means: on the To get list, with places to buy it; when the file arrives, the playlist slot fills.

## Two legitimate tools, for different jobs

| | Official IFrame player (embed) | music.youtube.com in a Venuta window |
|---|---|---|
| For | Songs a playlist waits for, inside Venuta's own queue; "listen before you get it" | People with no collection; discovery, radio, the full YouTube Music |
| Who controls the queue | Venuta (one video at a time, `loadVideoById`, `onStateChange` ended → next local file) | YouTube Music (Venuta only reads what's playing) |
| Account | None needed | The user's own Google account, signed in in that window |
| Rules | Player visible, at least 200x200 (480x270 advised); autoplay only when more than half visible; nothing drawn over it; no audio-only; ads untouched; no caching; YouTube credited | The site as it is: no ad blocking, no page changes; reading "now playing" is fine |
| Risk | Embedding turned off by some labels (errors 101/150); Error 153 if the app sends no proper identity | Google sign-in in WebView2 works today but isn't officially supported; the page changes often |
| Effort | 2-4 days plus a spike | About a day, plus a small read-only bridge |

Sources: [IFrame API](https://developers.google.com/youtube/iframe_api_reference) (2026-09-29),
[Developer Policies](https://developers.google.com/youtube/terms/developer-policies) and
[Required Minimum Functionality](https://developers.google.com/youtube/terms/required-minimum-functionality)
(2026-09-14), [Error 153](https://til.simonwillison.net/youtube/fixing-153-embed),
[WebView2 + Google sign-in](https://github.com/MicrosoftEdge/WebView2Feedback/issues/1647),
[Tauri issue](https://github.com/tauri-apps/tauri/issues/11612).

### What the rules mean for Venuta

- **A YouTube song plays only while its player is on screen.** With Venuta in the tray or
  minimised, a YouTube slot pauses (or is skipped, the user's choice); it never starts hidden.
- **Since late 2025, embeds need an app identity** (a Referer whose domain is the app id in
  reverse-DNS form, set through WebView2's request headers). Without it: Error 153. Tauri serves the
  UI from `http://tauri.localhost`; whether YouTube accepts that is untested.
- **Ads may show in embeds**, Premium or not (no official word; to test).
- **No API key in an open-source app** (policy III.D.1.d). The Data API's search is now 100 calls a
  day per project. So matching should not depend on Google's API.
- **Stored API data** is kept at most 30 days unless refreshed; a video id the user chose is their
  own data, but Venuta re-checks it before playing anyway.
- Precedent: Musi (embeds plus its own ads) was pulled from the App Store after YouTube complained;
  Streamus lost its key over background play. Keep the player visible, unmodified, ads intact.
  ([Musi](https://www.mactech.com/2026/03/18/musis-lawsuit-against-apple-has-been-dismissed-with-prejudice/amp/),
  [Streamus](https://www.tubefilter.com/2015/07/21/streamus-youtube-music-chrome-extension-takedown/))

## Finding the right YouTube song for a wanted one (no Google key)

1. The wanted song's ISRC → Deezer's public API (`track/isrc:<ISRC>`) → its Deezer link.
2. song.link / Odesli turns that into YouTube and YouTube Music links (free, about 10 a minute).
3. oEmbed checks the candidate (no key): a "Artist - Topic" channel ("Provided to YouTube by", the
   label's own art track) is preferred; the player's duration must be within 3 s.
4. Without an ISRC, or when unsure: the user picks, or pastes a YouTube / YouTube Music link.
   An optional own Data API key adds text search (100 a day).

Ruled out: ytmusicapi / InnerTube (unofficial, automated access), Invidious / Piped (blocked by
YouTube, unreliable), yt-dlp and anything that extracts streams.
([ytmusicapi](https://ytmusicapi.readthedocs.io/en/1.1.1/), [Invidious](https://docs.invidious.io/youtube-errors-explained/))

## How other apps do it

| App | How | Standing |
|---|---|---|
| Pear Desktop (ex th-ch/youtube-music, 34k stars) | Electron window of music.youtube.com, injected plugins, local API | Grey; its ad-skip and downloader plugins break the terms |
| Nuclear, Spotube, ViMusic, InnerTune, Metrolist, Harmony | Stream extraction | Not clean; Spotify sent Spotube a cease-and-desist in 2025 |
| Musi | Official embeds plus its own ads | Pulled from the App Store |
| Roon | Library vs TIDAL/Qobuz marked by an icon; "Add to Library" | The model for mixing sources |
| Plexamp | Mixed TIDAL until Oct 2024, then TIDAL left: placeholders stayed | Don't depend on a partner for the library itself |

No known desktop player mixes official YouTube embeds and local files in one queue: Venuta would
be the first.

## Quality, said honestly

YouTube Music streams 128 kbps by default and 256 kbps at most, AAC/Opus
([Google](https://support.google.com/youtubemusic/answer/9076559)). A YouTube slot shows a
"YouTube · lossy" chip; the bit-perfect mark goes grey while it plays; its sound plays in the
window, never through Venuta's exclusive output. Expect a loudness step at the seam.

## Listen before you get it, and where to buy (India)

- **iTunes Search API**: free, no key, 30-second previews, `country=in`; Apple's terms want the
  preview beside a store badge linking to the purchase. Fits the To get list.
  ([Apple](https://performance-partners.apple.com/search-api))
- **Deezer**: free 30-second previews, ISRC on tracks.
- **Bandcamp**: full-length embeds where the artist allows; lossless downloads; works in India.
- **Buy**: Bandcamp (FLAC), CDs via Discogs and Amazon.in, MusicBrainz's purchase links, the iTunes
  Store India (256 kbps AAC: lossy, labelled so). Qobuz isn't in India; HDtracks is effectively US.
- Not used: Spotify Web Playback SDK (Premium, no embedded hosts, strict), Apple MusicKit (paid
  developer account).

## To test first (one-hour spike, nothing signed in, nothing paid)

1. Does `youtube.com/embed` play in a Tauri window, or give Error 153/152? With `origin` /
   `widget_referrer`, and with a Referer set in WebView2's request handler.
2. How many of ~20 art tracks (Universal, Sony, Warner, T-Series, Saregama, indie) embed; does
   oEmbed predict it?
3. The hand-over: gap and loudness from a local FLAC (exclusive output) to YouTube and back.
4. Does WebView2 block the scripted start after a local song ends?
5. (With him) Google sign-in in a second window; does it last a week? Ads on embeds with Premium?
6. Does music.youtube.com fill `navigator.mediaSession` (title, artist, album) reliably?
7. On 50 wanted songs with ISRCs: how often does Deezer → Odesli find the YouTube Music song?
