// One ranking brain for online catalog search: every search bar (Music, Ctrl+K, Add songs,
// the online picker, "more by") orders catalog rows here, so the song people mean (the
// artist's own recording, on its album, played by millions) comes before covers, karaoke,
// slowed-and-reverb uploads and hour-long loops, unless the words typed ask for those.
//
// Plain TypeScript (no React, no aliases): node --test imports it as it is.

import type { OnlineAlbum, OnlineArtist, OnlineTrack } from './engine'
import { EXACTLY, NEARLY, matcher, searchKey } from './textsearch.ts'

// --- What kind of version a row is ---------------------------------------------------------

export type VersionClass = 'official' | 'video' | 'alternate' | 'remix' | 'cover'

/** How much each kind counts for an ordinary search (the kind a query asks for counts 1). */
const CLASS_WEIGHT: Record<VersionClass, number> = { official: 1, video: 0.85, alternate: 0.75, remix: 0.6, cover: 0.25 }

/** Words as typed, for spotting version words: lower case, no accents, single spaces. (Not
 *  `searchKey`, whose loose spelling turns "cover" into "kover".) */
const plain = (s: string | null | undefined) =>
  ` ${(s ?? '')
    .normalize('NFD')
    .replace(/\p{M}/gu, '')
    .toLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, ' ')
    .trim()} `

const words = (list: string[]) => new RegExp(` (?:${list.join('|')}) `)

// Someone else's take on the song, or the song made into something else.
const COVER = words([
  'karaoke',
  'covers?',
  'covered by',
  'tribute',
  'in the style of',
  'originally performed by',
  'made famous by',
  'as made famous',
  'nightcore',
  'slowed',
  'slow reverb',
  'reverb',
  'sped up',
  'speed up',
  '8d',
  '8d audio',
  'lofi',
  'lo fi',
  'ringtone',
  '\\d+ hours?',
  'one hour',
  'loop(?:ed)?',
  'bass boosted',
  'jhankar',
  'whatsapp status',
  'status video',
  'lullaby',
  'lullabies',
  'music box',
  'plays the music of',
  'cover sessions?',
  'female version',
  'male version',
  '(?:epic|trailer|epic trailer|cinematic|piano|guitar|violin|cello|flute|sax|saxophone|string|strings|orchestral|8 ?bit|chiptune|music box) (?:version|cover|rendition)',
])
const REMIX = words(['remix(?:es|ed)?', 'rmx', 'dance mix', 'club mix', 'vip mix', 'mashup', 'mash up', 'bootleg'])
const ALTERNATE = words([
  'live',
  'unplugged',
  'acoustic',
  'reprise',
  'remaster(?:ed)?',
  '\\d{4} remaster(?:ed)?',
  'extended(?: mix| version)?',
  'radio edit',
  'instrumental',
  'demo',
  'orchestral',
  'symphonic',
])
const LYRIC_UPLOAD = words(['lyrics?', 'lyric video', 'lyrical'])
const OFFICIAL_VIDEO = words(['official video', 'official music video', 'music video'])
const OFFICIAL_AUDIO = words(['official audio', 'audio'])

/** What the words typed ask for: a cover-kind, a remix, another version, or nothing special. */
export function searchIntent(query: string): VersionClass | null {
  const q = plain(query)
  if (COVER.test(q)) return 'cover'
  if (REMIX.test(q)) return 'remix'
  if (ALTERNATE.test(q)) return 'alternate'
  return null
}

/** Long listening asked for (a mix, a jukebox, hours): long rows aren't odd then. */
const wantsLong = (query: string) => words(['mix', 'jukebox', 'nonstop', 'non stop', 'mashup', '\\d+ hours?', 'hour', 'playlist', 'medley']).test(plain(query))

/** The kind of version a row is: the artist's own recording, an official video, another
 *  version (live, acoustic…), a remix, or a cover-kind (karaoke, slowed, loops, uploads…). */
export function versionClass(t: OnlineTrack, query = ''): VersionClass {
  const title = plain(t.title)
  const all = plain(`${t.title} ${t.album ?? ''} ${t.artists.join(' ')}`)
  const type = (t.video_type ?? '').toUpperCase()
  const hasAlbum = !!(t.album || t.album_id)
  if (COVER.test(all)) return 'cover'
  // Someone's lyric upload is a copy of the song, not the song.
  if (type === 'UGC' && LYRIC_UPLOAD.test(title)) return 'cover'
  if (REMIX.test(all)) return 'remix'
  if (ALTERNATE.test(title) || (hasAlbum && ALTERNATE.test(plain(t.album)))) return 'alternate'
  if (type === 'ATV') return 'official'
  if (type === 'OMV') return 'video'
  if (type === 'UGC') {
    // Someone's upload, no album: taken as a copy, unless the query asks for that kind.
    const intent = searchIntent(query)
    return hasAlbum ? 'video' : intent === 'alternate' ? 'alternate' : 'cover'
  }
  if (type === 'PODCAST_EPISODE') return 'cover'
  // Kind unknown (an older row, another catalog): the album and the title decide.
  if (hasAlbum && (OFFICIAL_AUDIO.test(title) || !OFFICIAL_VIDEO.test(title))) return 'official'
  return 'video'
}

/** A cover-kind row the query didn't ask for (for "Best" vs "Covers & other"). */
export const isCoverClass = (t: OnlineTrack, query: string) => versionClass(t, query) === 'cover' && searchIntent(query) !== 'cover'

// --- How well a row matches the words -------------------------------------------------------

/** A title without the notes that don't make it another song: (Official Video), (Lyrics),
 *  (Audio), (From "…"), feat. … */
export function cleanTitle(title: string): string {
  return title
    .replace(/[([](?:[^)\]]*\b(?:official|lyrics?|lyrical|audio|video|visuali[sz]er|hd|hq|4k|from|feat|ft)\b[^)\]]*)[)\]]/gi, ' ')
    .replace(/\s+-\s+(?:official\b.*|lyrics?\b.*|from\b.*)$/i, ' ')
    .replace(/\s+(?:feat\.?|ft\.?|featuring)\s.*$/i, ' ')
    .replace(/\s+/g, ' ')
    .trim()
}

/** The song a title names, without any version notes at all ("Wonderwall (Unplugged)" and
 *  "Oasis - Wonderwall (slowed)" are both "wonderwall"): who recorded the song is judged on it. */
export function songKey(title: string): string {
  const bare = cleanTitle(title)
    .replace(/[([][^)\]]*[)\]]/g, ' ')
    .split(/\s+-\s+/)
  // "Artist - Title": the title is the last part.
  return searchKey(bare[bare.length - 1] ?? '')
}

/** 0–1: the title is the query, starts with it, holds all its words, or the words are found
 *  across title, artists and album. */
function textScore(t: OnlineTrack, q: string): number {
  if (!q) return 0.5
  const title = searchKey(cleanTitle(t.title))
  const artists = searchKey(t.artists.join(' '))
  if (title === q) return 1
  // "tum hi ho arijit": the title, then (some of) its artist.
  if (q.startsWith(`${title} `) && title) {
    const rest = matcher(q.slice(title.length + 1))
    if (rest(artists) === EXACTLY) return 0.97
  }
  if (title.startsWith(`${q} `)) return 0.85
  const has = matcher(q)
  const inTitle = has(title)
  if (inTitle === EXACTLY) return 0.7
  const inAll = has(`${title} ${artists} ${searchKey(t.album ?? '')}`)
  if (inAll === EXACTLY) return 0.6
  if (inTitle === NEARLY || inAll === NEARLY) return 0.4
  return 0.1
}

// --- Scoring online songs -------------------------------------------------------------------

/** How much each signal counts. Text and kind lead; plays, album and catalog type follow;
 *  the catalog's own order and top result only nudge (the top card is sometimes a cover). */
export const WEIGHTS = {
  text: 3,
  version: 2.5,
  album: 0.4,
  type: { ATV: 0.5, OMV: 0.3, UGC: 0, PODCAST_EPISODE: -1, unknown: 0.2 } as Record<string, number>,
  plays: 1.5,
  /** log10(plays) at which the plays signal is full (≈3 billion). */
  playsFull: 9.5,
  /** Plays unknown: neither a boost nor a penalty to speak of. */
  playsUnknown: 0.3,
  shelf: 0.3,
  top: 0.25,
  lengthGood: 0.15,
  lengthOdd: -1,
  artist: 0.4,
  /** Same song (title without its version notes) by another artist, with under 1% of the
   *  most-played recording's plays: almost always someone's version of it. */
  notTheOriginal: -1.5,
  wantedLength: { within3s: 1.5, within15s: 0.7, far: -0.6 },
}

export interface RankOptions {
  /** The length of the song wanted (a playlist's missing song): closer lengths go first. */
  wantDurationMs?: number | null
}

const primaryArtist = (t: OnlineTrack) => searchKey(t.artists[0] ?? '')
const artistKeys = (t: OnlineTrack) => new Set(t.artists.map((a) => searchKey(a)).filter(Boolean))
const shareArtist = (a: OnlineTrack, b: OnlineTrack) => {
  const bs = artistKeys(b)
  return [...artistKeys(a)].some((x) => bs.has(x))
}

/** A row's score for a query (higher is better), and the parts it is made of (for tests). */
export function scoreTrack(t: OnlineTrack, query: string, opts: RankOptions = {}, context?: Map<string, OnlineTrack>) {
  const q = searchKey(query)
  const intent = searchIntent(query)
  const cls = versionClass(t, query)
  const text = textScore(t, q)
  const version = intent ? (cls === intent ? 1 : CLASS_WEIGHT[cls] * 0.7) : CLASS_WEIGHT[cls]
  const type = WEIGHTS.type[(t.video_type ?? '').toUpperCase()] ?? WEIGHTS.type.unknown
  const plays = t.plays && t.plays > 0 ? Math.min(1.1, Math.max(0, Math.log10(t.plays) / WEIGHTS.playsFull)) : WEIGHTS.playsUnknown
  const shelf = t.shelf_rank != null && t.shelf_rank < 10 ? 1 - t.shelf_rank / 10 : 0
  const top = t.is_top && (cls === 'official' || cls === 'video') ? 1 : 0
  let length = 0
  const ms = t.duration_ms ?? 0
  if (ms > 0) {
    if ((ms < 45_000 || ms > 20 * 60_000) && !wantsLong(query)) length = WEIGHTS.lengthOdd
    else if (ms >= 90_000 && ms <= 8 * 60_000) length = WEIGHTS.lengthGood
  }
  // Two or more words typed, and one of them names the row's artist.
  const tokens = q.split(' ').filter((w) => w.length >= 3)
  const artistWords = new Set(searchKey(t.artists.join(' ')).split(' '))
  const artist = tokens.length >= 2 && tokens.some((w) => artistWords.has(w)) ? WEIGHTS.artist : 0
  // The most-played recording of this title is someone else's, by far: likely a cover.
  let original = 0
  const best = context?.get(songKey(t.title))
  if (best && best !== t && intent !== 'cover' && (best.plays ?? 0) >= 1_000_000 && !shareArtist(best, t) && (t.plays ?? 0) < (best.plays ?? 0) / 100)
    original = WEIGHTS.notTheOriginal
  let wanted = 0
  if (opts.wantDurationMs && ms > 0) {
    const d = Math.abs(ms - opts.wantDurationMs)
    wanted = d <= 3_000 ? WEIGHTS.wantedLength.within3s : d <= 15_000 ? WEIGHTS.wantedLength.within15s : d > 60_000 ? WEIGHTS.wantedLength.far : 0
  }
  const parts = {
    text: text * WEIGHTS.text,
    version: version * WEIGHTS.version,
    album: t.album || t.album_id ? WEIGHTS.album : 0,
    type,
    plays: plays * WEIGHTS.plays,
    shelf: shelf * WEIGHTS.shelf,
    top: top * WEIGHTS.top,
    length,
    artist,
    original,
    wanted,
  }
  const score = Object.values(parts).reduce((a, b) => a + b, 0)
  return { score, cls, parts }
}

/** The most-played row of each song (by `songKey`). */
function mostPlayed(tracks: OnlineTrack[]): Map<string, OnlineTrack> {
  const out = new Map<string, OnlineTrack>()
  for (const t of tracks) {
    const k = songKey(t.title)
    const b = out.get(k)
    if (!b || (t.plays ?? 0) > (b.plays ?? 0)) out.set(k, t)
  }
  return out
}

/** The same recording twice (same title, a shared artist, lengths within 3 s when both known). */
function sameRecording(a: OnlineTrack, b: OnlineTrack): boolean {
  if (a.track_id === b.track_id && a.provider === b.provider) return true
  if (searchKey(cleanTitle(a.title)) !== searchKey(cleanTitle(b.title))) return false
  // Rows that name no artist are never taken for one another.
  const pa = primaryArtist(a)
  const artistsMatch = (!!pa && pa === primaryArtist(b)) || shareArtist(a, b)
  if (!artistsMatch) return false
  if (a.duration_ms && b.duration_ms) return Math.abs(a.duration_ms - b.duration_ms) <= 3_000
  return true
}

/**
 * Online songs in the order to show them for a query: best first, the same recording once
 * (the copy with the album, the catalog's own audio, the most plays), and cover-kinds after
 * everything else unless the words typed ask for them.
 */
export function rankOnlineTracks(query: string, tracks: OnlineTrack[], opts: RankOptions = {}): OnlineTrack[] {
  if (tracks.length < 2 && !opts.wantDurationMs) return tracks
  const context = mostPlayed(tracks)
  const coverWanted = searchIntent(query) === 'cover'
  const scored = tracks
    .map((t, i) => ({ t, i, ...scoreTrack(t, query, opts, context) }))
    .sort((a, b) => {
      const ca = !coverWanted && a.cls === 'cover' ? 1 : 0
      const cb = !coverWanted && b.cls === 'cover' ? 1 : 0
      return ca - cb || b.score - a.score || a.i - b.i
    })
  const kept: OnlineTrack[] = []
  for (const s of scored) if (!kept.some((k) => sameRecording(k, s.t))) kept.push(s.t)
  return kept
}

/** The ranked songs split into the ones to show by default and the cover-kinds. */
export function partitionBestAndCovers(tracks: OnlineTrack[], query: string): { best: OnlineTrack[]; covers: OnlineTrack[] } {
  const ranked = rankOnlineTracks(query, tracks)
  const best: OnlineTrack[] = []
  const covers: OnlineTrack[] = []
  for (const t of ranked) (isCoverClass(t, query) ? covers : best).push(t)
  return { best, covers }
}

// --- Albums and artists ----------------------------------------------------------------------

/** 0–1 for a name against the query: the same, starting with it, holding its words. */
function nameScore(name: string, q: string, extra = ''): number {
  if (!q) return 0.5
  const k = searchKey(name)
  if (k === q) return 1
  if (k.startsWith(`${q} `) || q.startsWith(`${k} `)) return 0.8
  const has = matcher(q)
  if (has(k) === EXACTLY) return 0.6
  if (extra && has(`${k} ${searchKey(extra)}`) === EXACTLY) return 0.5
  if (has(k) === NEARLY) return 0.35
  return 0.1
}

const sortBy = <T,>(items: T[], score: (x: T) => number) =>
  items
    .map((x, i) => ({ x, i, s: score(x) }))
    .sort((a, b) => b.s - a.s || a.i - b.i)
    .map((e) => e.x)

/** Online albums by how well they match: the name, then a real album over karaoke and
 *  tribute sets, the artist named in the query, and albums over singles a little. */
export function rankOnlineAlbums(query: string, albums: OnlineAlbum[]): OnlineAlbum[] {
  const q = searchKey(query)
  const intent = searchIntent(query)
  return sortBy(albums, (a) => {
    const text = nameScore(a.title, q, a.artists.join(' ')) * 3
    const copy = intent !== 'cover' && COVER.test(plain(`${a.title} ${a.artists.join(' ')}`)) ? -1.5 : 0
    const artist = q.split(' ').some((w) => w.length >= 3 && searchKey(a.artists.join(' ')).split(' ').includes(w)) ? 0.4 : 0
    const kind = a.kind === 'Album' ? 0.2 : a.kind === 'Single' || a.kind === 'EP' ? 0.1 : 0
    return text + copy + artist + kind
  })
}

/**
 * Online artists by how well the name matches and how many listen to them. Both count: typing a
 * first name ("arijit") means the artist millions play whose name starts with it, not a small
 * account called exactly that, while a name typed in full still leads. The audience adds up to
 * 2 on a log scale (1,000 listeners about 0.7, 600 million about 2).
 */
export function rankOnlineArtists(query: string, artists: OnlineArtist[]): OnlineArtist[] {
  const q = searchKey(query)
  return sortBy(artists, (a) => {
    const audience = a.audience && a.audience > 0 ? Math.min(2, (2 * Math.log10(a.audience)) / 9) : 0.6
    return nameScore(a.name, q) * 3 + audience
  })
}

/** Whether the words begin this artist's name and many listen to them (a Top result). */
const widelyHeard = (a: OnlineArtist, q: string) => searchKey(a.name).startsWith(`${q} `) && (a.audience ?? 0) >= 1_000_000

/** Songs, albums and artists of one search answer, each in ranked order. */
export function rankOnlineResults<R extends { tracks: OnlineTrack[]; albums: OnlineAlbum[]; artists: OnlineArtist[] }>(query: string, r: R): R {
  return { ...r, tracks: rankOnlineTracks(query, r.tracks), albums: rankOnlineAlbums(query, r.albums), artists: rankOnlineArtists(query, r.artists) }
}

// --- The top result ------------------------------------------------------------------------

/** What the Top result is: an artist, a song or an album, from the library or the online
 *  catalog, by its place in the list it came from. */
export type TopPick = { kind: 'artist' | 'song' | 'album'; where: 'library' | 'online'; index: number }

/** The library side, as far as choosing needs it. */
export interface TopLibrary {
  tracks: { title: string; artists: string[] }[]
  albums: { title: string }[]
  artists: { name: string }[]
}
/** The online side (songs: the Best list, covers set aside), or null while it isn't known. */
export interface TopOnline {
  songs: OnlineTrack[]
  albums: OnlineAlbum[]
  artists: OnlineArtist[]
}

/** How well a song matches the query, 0–1 (the same measure the ranker uses). */
export const songMatch = (t: { title: string; artists: string[] }, query: string) =>
  textScore({ ...(t as OnlineTrack), album: null }, searchKey(query))

/**
 * The one thing the words most likely mean, in this order:
 * 1. an artist whose name is the query: in the library, or online when no song is called that;
 * 2. a song called that: your own file first, else the online catalog's best;
 * 3. an album or film called that: yours first, then online;
 * 4. otherwise the best match overall, the library winning ties.
 */
export function pickTop(query: string, lib: TopLibrary, online: TopOnline | null): TopPick | null {
  const q = searchKey(query)
  if (!q) return null
  const same = (s: string) => searchKey(s) === q
  const isSong = (t: { title: string; artists: string[] }) => songMatch(t, query) >= 0.97
  const libArtist = lib.artists.findIndex((a) => same(a.name))
  if (libArtist >= 0) return { kind: 'artist', where: 'library', index: libArtist }
  const libSong = lib.tracks.findIndex(isSong)
  const onSongs = online?.songs.slice(0, 3) ?? []
  const onSong = onSongs.findIndex(isSong)
  // The first of the catalog's top two that is named this, or whose name the words begin and
  // that millions listen to ("arijit" for Arijit Singh).
  const onArtist = (online?.artists.slice(0, 2) ?? []).findIndex((a) => same(a.name) || widelyHeard(a, q))
  // A song title only outweighs an artist's name when it is yours, or widely played.
  const strongSong = libSong >= 0 || (onSong >= 0 && (onSongs[onSong].plays ?? 0) >= 1_000_000)
  if (onArtist >= 0 && !strongSong) return { kind: 'artist', where: 'online', index: onArtist }
  if (libSong >= 0) return { kind: 'song', where: 'library', index: libSong }
  if (onSong >= 0) return { kind: 'song', where: 'online', index: onSong }
  const libAlbum = lib.albums.findIndex((a) => same(a.title))
  if (libAlbum >= 0) return { kind: 'album', where: 'library', index: libAlbum }
  const onAlbum = (online?.albums.slice(0, 3) ?? []).findIndex((a) => same(a.title))
  if (onAlbum >= 0) return { kind: 'album', where: 'online', index: onAlbum }
  if (onArtist >= 0) return { kind: 'artist', where: 'online', index: onArtist }
  // Nothing named exactly that: the closest song, yours by a hair.
  let bestLib = -1
  let libScore = -1
  lib.tracks.slice(0, 50).forEach((t, i) => {
    const s = songMatch(t, query)
    if (s > libScore) [bestLib, libScore] = [i, s]
  })
  const onScore = online?.songs.length ? songMatch(online.songs[0], query) : -1
  if (bestLib >= 0 && libScore + 0.05 >= onScore) return { kind: 'song', where: 'library', index: bestLib }
  if (online?.songs.length) return { kind: 'song', where: 'online', index: 0 }
  if (lib.albums.length) return { kind: 'album', where: 'library', index: 0 }
  if (online?.albums.length) return { kind: 'album', where: 'online', index: 0 }
  if (lib.artists.length) return { kind: 'artist', where: 'library', index: 0 }
  if (online?.artists.length) return { kind: 'artist', where: 'online', index: 0 }
  return null
}
