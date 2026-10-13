// Run: node scripts/fake-spotify.mjs (REVOKE=1 to refuse refresh tokens), then start a debug copy
// with VENUTA_SPOTIFY_ACCOUNTS=http://127.0.0.1:8899 VENUTA_SPOTIFY_API=http://127.0.0.1:8899/v1
// and any 32-character Client ID. See docs/decisions/0005-accounts-oauth.md.
// A stand-in for Spotify's accounts service and Web API, for testing Venuta's sign-in end to end.
import { createServer } from 'node:http'
import { createHash } from 'node:crypto'
const PORT = 8899
const B = `http://127.0.0.1:${PORT}`
let challenge = ''
let issued = new Set(['never'])
const log = (...a) => console.log(new Date().toISOString().slice(11, 19), ...a)
const tr = (name, artist, album, ms, isrc) => ({ type: 'track', name, duration_ms: ms, artists: [{ name: artist }], album: { name: album }, external_ids: isrc ? { isrc } : {} })
const json = (res, status, body) => {
  res.writeHead(status, { 'content-type': 'application/json' })
  res.end(JSON.stringify(body))
}
createServer((req, res) => {
  const u = new URL(req.url, B)
  let body = ''
  req.on('data', (c) => (body += c))
  req.on('end', () => {
    log(req.method, u.pathname + u.search.slice(0, 60))
    if (u.pathname === '/authorize') {
      challenge = u.searchParams.get('code_challenge')
      const ok = u.searchParams.get('code_challenge_method') === 'S256' && u.searchParams.get('scope')?.includes('user-library-read')
      const back = new URL(u.searchParams.get('redirect_uri'))
      back.searchParams.set(ok ? 'code' : 'error', ok ? 'CODE123' : 'invalid_request')
      back.searchParams.set('state', u.searchParams.get('state'))
      res.writeHead(302, { location: back.toString() })
      return res.end()
    }
    if (u.pathname === '/api/token') {
      const f = new URLSearchParams(body)
      if (f.get('grant_type') === 'authorization_code') {
        const proof = createHash('sha256').update(f.get('code_verifier') ?? '').digest('base64url')
        if (f.get('code') !== 'CODE123' || proof !== challenge || !f.get('redirect_uri')?.startsWith('http://127.0.0.1:'))
          return json(res, 400, { error: 'invalid_grant', error_description: 'PKCE proof or code did not match' })
        issued.add('AT1')
        log('  PKCE proof checked: ok')
        return json(res, 200, { access_token: 'AT1', token_type: 'Bearer', expires_in: 3600, refresh_token: 'RT1', scope: 'playlist-read-private user-library-read' })
      }
      if (f.get('grant_type') === 'refresh_token') {
        log('  refresh with', f.get('refresh_token'))
        if (process.env.REVOKE || !['RT1', 'RT2'].includes(f.get('refresh_token'))) return json(res, 400, { error: 'invalid_grant', error_description: 'Refresh token revoked' })
        issued.add('AT2')
        return json(res, 200, { access_token: 'AT2', token_type: 'Bearer', expires_in: 3600, refresh_token: 'RT2' })
      }
    }
    if (u.pathname.startsWith('/v1/')) {
      const token = (req.headers.authorization ?? '').replace('Bearer ', '')
      if (!issued.has(token)) return json(res, 401, { error: { status: 401, message: 'The access token expired' } })
      const p = u.pathname.slice(3)
      if (p === '/me') return json(res, 200, { id: 'me', display_name: 'Test Listener' })
      if (p === '/me/tracks') {
        const items = [{ track: tr('Garda', 'Daler Mehndi', 'Atrangi Re', 224000) }, { track: tr('Vida Karo', 'Arijit Singh', 'Amar Singh Chamkila', 270000) }]
        return json(res, 200, { total: 2, items: u.searchParams.get('limit') === '1' ? items.slice(0, 1) : items, next: null })
      }
      if (p === '/me/playlists') {
        if (!u.searchParams.get('offset'))
          return json(res, 200, { items: [{ id: 'p1', name: 'Road songs', owner: { id: 'me', display_name: 'Test Listener' }, collaborative: false, items: { total: 3 }, images: [] }], next: `${B}/v1/me/playlists?offset=1&limit=50` })
        return json(res, 200, {
          items: [
            { id: 'p2', name: 'Shared mix', owner: { id: 'friend', display_name: 'A Friend' }, collaborative: true, items: { total: 1 }, images: [] },
            { id: 'p3', name: "Today's Top Hits", owner: { id: 'spotify', display_name: 'Spotify' }, collaborative: false, items: { total: 50 }, images: [] },
          ],
          next: null,
        })
      }
      if (p === '/playlists/p1/items')
        return json(res, 200, {
          items: [
            { item: tr('Tere Rang', 'Haricharan', 'Atrangi Re', 274000) },
            { item: tr('Ishq Mitaye', 'Mohit Chauhan', 'Amar Singh Chamkila', 273000) },
            { item: { type: 'episode', name: 'A podcast' } },
            { item: tr('Kun Faya Kun', 'A.R. Rahman', 'Rockstar', 472000) },
          ],
          next: null,
        })
      if (p === '/playlists/p2/items') return json(res, 200, { items: [{ item: tr('Hawa Sun Hawa', 'Sonu Nigam', 'Ada - A Way Of Life', 358000) }], next: null })
      if (p === '/playlists/p3/items') return json(res, 403, { error: { status: 403 } })
    }
    json(res, 404, { error: 'not found' })
  })
}).listen(PORT, '127.0.0.1', () => log('fake Spotify on', B))
