import { createContext, useContext } from 'react'
import { isHiRes, quality, type CatalogTrack } from '../../lib/engine'

/*
  The Music home's view of the library: the flat catalog of songs grouped into albums and
  artists once, so every tab, shelf and search reads the same shapes.
*/

export interface Album {
  dir: string
  title: string
  artist: string
  year: string
  genre: string
  category: string
  /** In play order: disc, then track number. */
  tracks: CatalogTrack[]
  duration_ms: number
  /** The most common quality among its songs ("FLAC 24/96"). */
  quality: string
  hiRes: boolean
  discs: number
}

export interface Artist {
  key: string
  name: string
  /** Newest first. */
  albums: Album[]
  songs: number
}

export interface Library {
  tracks: CatalogTrack[]
  /** By title. */
  albums: Album[]
  byDir: Map<string, Album>
  /** By name. */
  artists: Artist[]
  byArtist: Map<string, Artist>
  /** Categories by album count, most first. */
  categories: [string, number][]
  hiRes: Album[]
  /** Lower-cased text to search, one per track and one per album (same order as the lists). */
  trackText: string[]
  albumText: string[]
}

const collator = new Intl.Collator(undefined, { sensitivity: 'base', numeric: true })
export const compareText = collator.compare

/** One spelling for one artist, whatever the letter case or stray spaces ("A. R." and "A.R."). */
export const artistKey = (name: string) =>
  name
    .trim()
    .replace(/\s+/g, ' ')
    .replace(/\.\s(?=\S)/g, '.')
    .toLowerCase()

function mostCommon(values: string[]): string {
  const counts = new Map<string, number>()
  let best = ''
  let top = 0
  for (const v of values) {
    if (!v) continue
    const n = (counts.get(v) ?? 0) + 1
    counts.set(v, n)
    if (n > top) {
      top = n
      best = v
    }
  }
  return best
}

const folderName = (dir: string) => dir.split(/[\\/]/).filter(Boolean).pop() ?? dir

export const yearOf = (a: Pick<Album, 'year'>) => Number.parseInt(a.year, 10) || 0

export function buildLibrary(tracks: CatalogTrack[]): Library {
  const groups = new Map<string, CatalogTrack[]>()
  for (const t of tracks) {
    const g = groups.get(t.dir)
    if (g) g.push(t)
    else groups.set(t.dir, [t])
  }

  const albums: Album[] = []
  for (const [dir, list] of groups) {
    list.sort((a, b) => (a.disc || 1) - (b.disc || 1) || a.track - b.track || compareText(a.title, b.title))
    const hi = list.filter(isHiRes).length
    albums.push({
      dir,
      title: mostCommon(list.map((t) => t.album)) || folderName(dir),
      artist: mostCommon(list.map((t) => t.album_artist)) || mostCommon(list.map((t) => t.artists[0] ?? '')) || 'Unknown artist',
      year: mostCommon(list.map((t) => t.year)),
      genre: mostCommon(list.map((t) => t.genre)),
      category: mostCommon(list.map((t) => t.category)) || 'Other',
      tracks: list,
      duration_ms: list.reduce((s, t) => s + t.duration_ms, 0),
      quality: mostCommon(list.map(quality)),
      hiRes: hi > 0 && hi * 2 >= list.length,
      discs: new Set(list.map((t) => t.disc || 1)).size,
    })
  }
  albums.sort((a, b) => compareText(a.title, b.title) || compareText(a.artist, b.artist))

  const byArtist = new Map<string, Artist>()
  for (const a of albums) {
    const key = artistKey(a.artist)
    const found = byArtist.get(key)
    if (found) {
      found.albums.push(a)
      found.songs += a.tracks.length
    } else byArtist.set(key, { key, name: a.artist, albums: [a], songs: a.tracks.length })
  }
  for (const ar of byArtist.values()) ar.albums.sort(newestFirst)
  const artists = [...byArtist.values()].sort((a, b) => compareText(a.name, b.name))

  const cats = new Map<string, number>()
  for (const a of albums) cats.set(a.category, (cats.get(a.category) ?? 0) + 1)
  const categories = [...cats.entries()].sort((a, b) => b[1] - a[1] || compareText(a[0], b[0]))

  return {
    tracks,
    albums,
    byDir: new Map(albums.map((a) => [a.dir, a])),
    artists,
    byArtist,
    categories,
    hiRes: albums.filter((a) => a.hiRes),
    trackText: tracks.map((t) => `${t.title} ${t.artists.join(' ')} ${t.album} ${t.album_artist}`.toLowerCase()),
    albumText: albums.map((a) => `${a.title} ${a.artist} ${a.year}`.toLowerCase()),
  }
}

export const newestFirst = (a: Album, b: Album) => yearOf(b) - yearOf(a) || compareText(a.title, b.title)

/** Songs where this artist sings or plays, on albums that are someone else's. */
export function appearsOn(lib: Library, key: string): Album['tracks'] {
  return lib.tracks.filter((t) => artistKey(t.album_artist) !== key && t.artists.some((n) => artistKey(n) === key))
}

/** An artist page for any name: their own albums, or none when they only appear on others'. */
export function artistFor(lib: Library, name: string): Artist {
  const key = artistKey(name)
  return lib.byArtist.get(key) ?? { key, name: name.trim(), albums: [], songs: 0 }
}

export interface Results {
  albums: Album[]
  artists: Artist[]
  tracks: CatalogTrack[]
}

/** Everything that holds every word typed, in any order. */
export function search(lib: Library, query: string): Results {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean)
  if (!words.length) return { albums: [], artists: [], tracks: [] }
  const has = (text: string) => words.every((w) => text.includes(w))
  return {
    albums: lib.albums.filter((_, i) => has(lib.albumText[i])),
    artists: lib.artists.filter((a) => has(`${a.key} ${a.name.toLowerCase()}`)),
    tracks: lib.tracks.filter((_, i) => has(lib.trackText[i])),
  }
}

/** The same album all day, a different one tomorrow; albums with covers first. */
export function featured(albums: Album[], withCover: (dir: string) => boolean, now = new Date()): Album | undefined {
  const pool = albums.filter((a) => withCover(a.dir))
  const list = pool.length ? pool : albums
  if (!list.length) return undefined
  const day = Math.floor((now.getTime() - now.getTimezoneOffset() * 60_000) / 86_400_000)
  const mixed = Math.imul(day ^ 0x5bd1e995, 0x9e3779b1) >>> 0
  return list[mixed % list.length]
}

export function shuffled<T>(xs: T[]): T[] {
  const out = xs.slice()
  for (let i = out.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1))
    ;[out[i], out[j]] = [out[j], out[i]]
  }
  return out
}

export const fmtNum = (n: number) => n.toLocaleString('en-IN')
export const plural = (n: number, one: string, many = `${one}s`) => `${fmtNum(n)} ${n === 1 ? one : many}`

/** "58 min", "1 h 12 min". */
export function minutes(ms: number): string {
  const m = Math.max(1, Math.round(ms / 60_000))
  return m >= 60 ? `${Math.floor(m / 60)} h${m % 60 ? ` ${m % 60} min` : ''}` : `${m} min`
}

export const spring = { type: 'spring', stiffness: 380, damping: 34 } as const

/** Scroll smoothly, unless motion is turned down. */
export const smooth = (): ScrollBehavior => (document.documentElement.dataset.motion === 'reduced' ? 'auto' : 'smooth')

/** The element that scrolls this one (the screen's own scroll area). */
export function scrollerOf(el: HTMLElement | null): HTMLElement | null {
  for (let p = el?.parentElement ?? null; p; p = p.parentElement) {
    const o = getComputedStyle(p).overflowY
    if (o === 'auto' || o === 'scroll') return p
  }
  return null
}

// --- Shared with every part of the screen -----------------------------------------------------

export interface Nav {
  root: string
  /** `origin` names where the cover was, so it can fly from there to the album page. */
  openAlbum: (dir: string, origin?: string) => void
  openArtist: (name: string) => void
  editDetails: (dir: string) => void
}
export const NavContext = createContext<Nav>({
  root: '',
  openAlbum: () => {},
  openArtist: () => {},
  editDetails: () => {},
})
export const useNav = () => useContext(NavContext)

/** What is playing: only what rows need, so position updates never re-render the lists. */
export interface Now {
  path: string
  dir: string
  playing: boolean
}
export const NowContext = createContext<Now>({ path: '', dir: '', playing: false })
export const useNowPlaying = () => useContext(NowContext)

/** A cover flies between places that share its layout id; one place per origin. */
export const coverId = (origin: string | undefined, dir: string) => (origin ? `listen-${origin}:${dir}` : undefined)
