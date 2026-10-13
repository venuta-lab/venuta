# 0005: Signing in to streaming services (Spotify first)

Date: 2026-10-13. Status: accepted.

## Context

Playlists could only be brought over from export files (Spotify account data, Exportify,
Takeout, TuneMyMusic…) or a public Deezer link. He wants to sign in and read them directly,
done the way desktop apps are expected to do it.

Spotify's rules as of October 2026 (developer.spotify.com, redirect URI concept page; February
2026 migration guide; March and July 2026 changelogs):

- Authorization Code with PKCE is the flow for apps that can't keep a secret.
- Redirect URIs must match exactly, except loopback IP literals, which may use a dynamic port
  when registered without one. `localhost` is not allowed; `http` only for loopback.
- Development-mode apps: the owner needs Premium; 5 users per app; 25 apps per developer;
  quotas are shared per developer; a 429 carries `"reason": "QUOTA_EXCEEDED"`.
- `GET /me/playlists` lists your playlists. `GET /playlists/{id}/items` returns the songs only
  for playlists you own or collaborate on (others: metadata only). The playlist object's
  `tracks` became `items`, and each entry's `track` became `item`.
- `GET /me/tracks` (Liked Songs) still works. Tracks have `external_ids` (ISRC) again.

## Decision

Follow the OAuth 2.0 for native apps best current practice (RFC 8252) with PKCE (RFC 7636):

1. **The system browser**, never an embedded web view, so Venuta never sees the password and
   the browser's own sign-in, passkeys and password manager work.
2. **Loopback redirect**: a one-shot listener on `127.0.0.1` with a port chosen by the system,
   registered as `http://127.0.0.1/callback`. It answers once, then closes; it gives up after
   five minutes or when cancelled.
3. **PKCE S256** and a random **`state`** checked on return; no client secret exists anywhere.
4. **Least privilege**: `playlist-read-private playlist-read-collaborative user-library-read`.
   Nothing that writes.
5. **Tokens**: the refresh token lives in the Windows Credential Manager (the `keyring`
   crate), never in a file or the HTTP cache; access tokens only in memory. Spotify rotates
   refresh tokens: the new one replaces the old.
6. **Disconnect** forgets the token here, and says where to revoke access at Spotify
   (spotify.com/account/apps), since Spotify has no revocation endpoint.
7. **Polite API use**: paging at 50, `Retry-After` honoured on 429, a 401 refreshes once.
   Personal data is never cached.
8. **Bring your own Client ID**: development mode allows five users per app, so each person
   registers a free app (two minutes, steps in Settings). The ID is not a secret.
9. Songs come with their ISRC, so matching is exact far more often. Playlists Spotify won't
   share the songs of (followed, not yours) are listed with the way to bring them over by file.

The OAuth part is generic (endpoints, scopes), so TIDAL and YouTube can follow with their own
provider entries.

## Checked

- Unit tests: the RFC 7636 PKCE vector; the sign-in address; the listener refusing a forged
  state and a favicon, accepting the real answer; "no", time-out and cancel; the token exchange
  and refresh against a local token endpoint; Spotify parsing (item/track, ISRC, episodes,
  readability); paging with a 429 waited out; 401 reported; the vault keyed by Client ID.
- End to end in the app against a stand-in Spotify (scripts/fake-spotify.mjs, switched on by
  VENUTA_SPOTIFY_ACCOUNTS / VENUTA_SPOTIFY_API, which also skip opening a browser): connect,
  list, read three playlists into the review step, refresh after a restart (rotated token
  used), revoked refresh token forgotten. The real Credential Manager entry is
  "account:spotify" under Venuta.
