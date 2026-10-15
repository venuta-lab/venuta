import { useEffect, useState } from 'react'
import { musicCatalog, type CatalogTrack } from './engine'

/*
  Every song in the open library, shared by the screens that play music (Music, Playlists, the
  add-to-playlist picker). Read once per library and per scan: `version` is whatever changes
  when the folder was read again (the health report).
*/
let cached: { root: string; version: unknown; tracks: CatalogTrack[]; byPath: Map<string, CatalogTrack> } | null = null
let pending: { root: string; version: unknown; promise: Promise<CatalogTrack[]> } | null = null
const listeners = new Set<() => void>()

function load(root: string, version: unknown): Promise<CatalogTrack[]> {
  if (cached && cached.root === root && cached.version === version) return Promise.resolve(cached.tracks)
  if (pending && pending.root === root && pending.version === version) return pending.promise
  const promise = musicCatalog(root)
    .catch(() => [] as CatalogTrack[])
    .then((tracks) => {
      cached = { root, version, tracks, byPath: new Map(tracks.map((t) => [t.path, t])) }
      pending = null
      listeners.forEach((l) => l())
      return tracks
    })
  pending = { root, version, promise }
  return promise
}

/** The library's songs (null while they load). */
export function useCatalog(root: string, version: unknown): CatalogTrack[] | null {
  const [, tick] = useState(0)
  useEffect(() => {
    const l = () => tick((n) => n + 1)
    listeners.add(l)
    if (root) void load(root, version)
    return () => {
      listeners.delete(l)
    }
  }, [root, version])
  return cached && cached.root === root ? cached.tracks : null
}

/** The songs, read now if no screen has asked yet (playing a playlist from its card, say). */
export function ensureCatalog(root: string): Promise<CatalogTrack[]> {
  if (cached && cached.root === root) return Promise.resolve(cached.tracks)
  if (pending && pending.root === root) return pending.promise
  return load(root, null)
}

/** Songs for library paths, in order (paths not in the library are skipped). */
export function tracksFor(paths: string[]): CatalogTrack[] {
  if (!cached) return []
  const out: CatalogTrack[] = []
  for (const p of paths) {
    const t = cached.byPath.get(p)
    if (t) out.push(t)
  }
  return out
}

/** One song by its library path, if known. */
export const trackAt = (path: string) => cached?.byPath.get(path)

/** The library's songs as read last, if they have been (for menus that are built at once). */
export const catalogNow = (root: string) => (cached && cached.root === root ? cached.tracks : null)
