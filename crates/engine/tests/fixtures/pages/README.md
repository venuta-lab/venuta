# Saved public playlist pages

What each link reader is tested against, so tests never go online. Fetched 2026-10-14 from big
public editorial playlists (nothing personal), signed out, no key; trimmed to the part a reader
uses (the data script, or the track markup), in a tiny page with the same element ids.

| File | From | What's in it |
|---|---|---|
| `spotify-embed-playlist.html` | `open.spotify.com/embed/playlist/37i9dQZF1DXcBWIGoYBM5M` (Today's Top Hits) | `__NEXT_DATA__` → `props.pageProps.state.data.entity.trackList` (50 songs; the page holds 100 at most) |
| `apple-music-playlist.html` | `music.apple.com/us/playlist/todays-hits/pl.f4d106fed2bd41149aaacabb233eb5eb` | `serialized-server-data` → `data[0].data.sections`, the `trackLockup` section (50 songs) |
| `tidal-embed-playlist.html` | `embed.tidal.com/playlists/36ea71a8-445e-41a4-82ab-6628c581535d` | `<list-item product-type="track">` rows (49) |
| `jiosaavn-playlist.json` | JioSaavn's own page data for `featured/trending-today` (page 1) | `list[]` (43 songs) |
| `deezer-playlist.json` | `api.deezer.com/playlist/3155776842` | `tracks.data[]` (100, with ISRC) |

When a site changes its page, save the new one beside the old (`…-2026-12.html`) and keep both
tests: the reader has to read both, or fail with a plain message.
