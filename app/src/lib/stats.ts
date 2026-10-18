/*
  Listening stats, from the plays the app keeps (app/src-tauri listening.rs): how much, how
  many, which days, which hours, and who and what most. Your files and online songs count
  alike; a file says what it is through `info` (the library), an online play carries it.
  Pure (no app imports), so tests/stats.test.ts reads it.
*/

/** One play as the app keeps it. */
export interface Play {
  at: number
  heard_ms: number
  /** A library path, or `online:<provider>:<id>`. */
  file: string
  title: string | null
  artists: string[]
  album: string | null
  art: string | null
  ctx: string | null
}

/** What a library file is (from the library). */
export interface SongInfo {
  title: string
  artists: string[]
  album: string
  dir: string
}

export interface Ranked {
  /** What it is called: an artist's name, a song's or an album's title. */
  name: string
  ms: number
  plays: number
  /** Of all the time listened, 0–1. */
  share: number
}
export interface SongRank extends Ranked {
  file: string
  title: string
  artists: string[]
  /** The album folder (a library song), for its cover. */
  dir: string | null
  /** The cover address (an online song). */
  art: string | null
}
export interface AlbumRank extends Ranked {
  album: string
  artist: string
  dir: string | null
  art: string | null
}

export interface Stats {
  ms: number
  plays: number
  songs: number
  artists: number
  /** Days in a row, up to today (or yesterday), with at least one play. */
  streak: number
  daily: { day: number; ms: number }[]
  busiest: { day: number; ms: number } | null
  topArtists: Ranked[]
  topSongs: SongRank[]
  topAlbums: AlbumRank[]
  /** Time heard in each hour of the day (0–23). */
  hours: number[]
}

const DAY = 86_400_000
const dayStart = (t: number) => {
  const d = new Date(t)
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime()
}
const nextDay = (t: number) => {
  const d = new Date(t)
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() + 1).getTime()
}

/** A file's name without its folder and kind ("01 - Song" for "A/01 - Song.flac"). */
const fileName = (f: string) => (f.split('/').pop() ?? f).replace(/\.[a-z0-9]+$/i, '')

function rank<T extends { ms: number; plays: number }>(m: Map<string, T>, total: number, n: number) {
  return [...m.values()]
    .sort((a, b) => b.ms - a.ms || b.plays - a.plays)
    .slice(0, n)
    .map((x) => ({ ...x, share: total ? x.ms / total : 0 }))
}

export function statsOf(all: Play[], info: (file: string) => SongInfo | null, range: { from: number; to: number; now: number }, top = 10): Stats {
  const plays = all.filter((p) => p.at >= range.from && p.at <= range.to)
  const ms = plays.reduce((n, p) => n + p.heard_ms, 0)
  const artists = new Map<string, Omit<Ranked, 'share'>>()
  const songs = new Map<string, Omit<SongRank, 'share'>>()
  const albums = new Map<string, Omit<AlbumRank, 'share'>>()
  const hours = Array.from({ length: 24 }, () => 0)
  const byDay = new Map<number, number>()
  for (const p of plays) {
    const lib = p.file.startsWith('online:') ? null : info(p.file)
    const title = lib?.title || p.title || fileName(p.file)
    const credits = lib?.artists.length ? lib.artists : p.artists
    const album = lib?.album || p.album || ''
    const lead = credits[0] ?? ''
    if (lead) {
      const a = artists.get(lead.toLowerCase()) ?? { name: lead, ms: 0, plays: 0 }
      a.ms += p.heard_ms
      a.plays++
      artists.set(lead.toLowerCase(), a)
    }
    const s = songs.get(p.file) ?? { name: title, title, artists: credits, file: p.file, dir: lib?.dir ?? null, art: p.art, ms: 0, plays: 0 }
    s.ms += p.heard_ms
    s.plays++
    songs.set(p.file, s)
    if (album) {
      const key = `${album.toLowerCase()}|${lead.toLowerCase()}`
      const al = albums.get(key) ?? { name: album, album, artist: lead, dir: lib?.dir ?? null, art: p.art, ms: 0, plays: 0 }
      al.ms += p.heard_ms
      al.plays++
      albums.set(key, al)
    }
    hours[new Date(p.at).getHours()] += p.heard_ms
    const d = dayStart(p.at)
    byDay.set(d, (byDay.get(d) ?? 0) + p.heard_ms)
  }
  const daily: { day: number; ms: number }[] = []
  for (let d = dayStart(range.from); d <= range.to; d = nextDay(d)) daily.push({ day: d, ms: byDay.get(d) ?? 0 })
  const busiest = daily.reduce<{ day: number; ms: number } | null>((b, d) => (d.ms > (b?.ms ?? 0) ? d : b), null)
  // Days in a row with a play, counted back from today (a streak isn't broken by today yet).
  const played = new Set(all.map((p) => dayStart(p.at)))
  let streak = 0
  let d = dayStart(range.now)
  if (!played.has(d)) d -= DAY
  while (played.has(dayStart(d))) {
    streak++
    d = dayStart(d - DAY / 2)
  }
  return {
    ms,
    plays: plays.length,
    songs: songs.size,
    artists: artists.size,
    streak,
    daily,
    busiest,
    topArtists: rank(artists, ms, top),
    topSongs: rank(songs, ms, top),
    topAlbums: rank(albums, ms, top),
    hours,
  }
}
