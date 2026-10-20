// The YouTube Music links Copy link puts on the clipboard: an album by its page, a playlist or a
// podcast by its list, an artist by their channel. Your own uploads have none to share.
//
// Pure (no app imports), so tests/shareLinks.test.ts reads it as it is.

const BASE = 'https://music.youtube.com'

export function albumLink(albumId: string): string | null {
  const id = albumId.trim()
  return id ? `${BASE}/browse/${encodeURIComponent(id)}` : null
}

export function collectionLink(collectionId: string): string | null {
  const id = collectionId.trim().replace(/^VL/, '').replace(/^MPSP/, '')
  if (!id || id.startsWith('FEmusic_')) return null
  return `${BASE}/playlist?list=${encodeURIComponent(id)}`
}

export function artistLink(artistId: string): string | null {
  const id = artistId.trim()
  return id ? `${BASE}/channel/${encodeURIComponent(id)}` : null
}
