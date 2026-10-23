// Sharing a song of yours by its YouTube Music link: which online song is yours, the same
// recording, and not another version of it. Stricter than telling an online song you already
// have (lib/owned.ts, which a live take may stand in for): a shared link must play the song you
// meant. Chosen only when sure; otherwise the closest few are offered to choose from.
//
// The same song: the same recording code when both have one (certain either way), else the same
// title once credits and features are left out, the same version words (live, remix, acoustic…),
// at least one artist in common, and both lengths known and within 4 s.

import { sameName, titleCore } from './owned.ts'
import { searchKey } from './textsearch.ts'

/** Lengths further apart than this are different versions (a radio edit, a film cut). */
const SAME_LENGTH_MS = 4_000

/** Words that make a recording another version of the song, as they are written in titles. */
const VERSIONS: [RegExp, string][] = [
  [/\blive\b/, 'live'],
  [/\bremix(ed)?\b/, 'remix'],
  [/\bacoustic\b/, 'acoustic'],
  [/\bunplugged\b/, 'unplugged'],
  [/\bslowed\b/, 'slowed'],
  [/\breverb\b/, 'reverb'],
  [/\bsped[ -]?up\b/, 'sped up'],
  [/\breprise\b/, 'reprise'],
  [/\blo-?fi\b/, 'lofi'],
  [/\binstrumental\b/, 'instrumental'],
  [/\bkaraoke\b/, 'karaoke'],
  [/\bcover\b/, 'cover'],
  [/\bmashup\b/, 'mashup'],
  [/\bextended\b/, 'extended'],
]

/** A title's version words, from what follows its name (in brackets or after a dash): "Live
 *  Is Life" is a song's name, "Song (Live)" a version. Sorted, each once. */
export function versionWords(title: string): string[] {
  const lower = title.toLowerCase()
  const at = Math.min(...['(', '[', ' - ', ' – '].map((c) => lower.indexOf(c)).filter((i) => i > 0), lower.length)
  const tail = lower.slice(at)
  return [...new Set(VERSIONS.filter(([re]) => re.test(tail)).map(([, w]) => w))].sort()
}

/** A song as it is matched: yours, or one online. */
export interface MatchSong {
  title: string
  artists: string[]
  duration_ms?: number | null
  isrc?: string | null
}

/** What sharing a song of yours can use: the online song that is it, or none and the closest
 *  few (the same title) to choose from. */
export interface ShareMatch<T> {
  pick?: T
  offered: T[]
}

/** How many to offer when none is sure. */
const OFFERED = 3

export function shareMatch<T extends MatchSong>(mine: MatchSong, results: T[]): ShareMatch<T> {
  const core = titleCore(mine.title)
  const versions = versionWords(mine.title).join(',')
  const artists = mine.artists.map(searchKey).filter(Boolean)
  const code = mine.isrc?.trim().toUpperCase() || null
  const same = (t: T): boolean => {
    const theirs = t.isrc?.trim().toUpperCase() || null
    if (code && theirs) return code === theirs
    if (!core || titleCore(t.title) !== core) return false
    if (versionWords(t.title).join(',') !== versions) return false
    const shared = t.artists.map(searchKey).some((b) => !!b && artists.some((a) => sameName(a, b)))
    if (!shared) return false
    const a = mine.duration_ms ?? 0
    const b = t.duration_ms ?? 0
    return a > 0 && b > 0 && Math.abs(a - b) <= SAME_LENGTH_MS
  }
  const pick = results.find(same)
  if (pick) return { pick, offered: [] }
  return { offered: results.filter((t) => !!core && titleCore(t.title) === core).slice(0, OFFERED) }
}
