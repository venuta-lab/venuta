import { useEffect, useMemo } from 'react'
import { loadCovers, useCovers } from '../../lib/covers'
import { lineAt, lineProgress, useSmoothPosition, useSongLyrics } from '../../lib/lyrics'
import type { Ground, LyricColour } from '../../lib/lyricLook'
import { current, isOnline, usePlayerValue } from '../../lib/player'
import { artUrl } from '../RemoteArt'
import { accentRoles } from '../../lib/themes'
import { toLatin } from '../../lib/textsearch'

const INDIC = /[ऀ-ൿ]/

/** The song playing, its words, the line being sung and how far through it, smoothly. */
export function useNowLyrics() {
  const song = usePlayerValue(current)
  const root = usePlayerValue((s) => s.root ?? '')
  const playing = usePlayerValue((s) => s.playing)
  // Your file's words, else the lyrics services' (online songs too).
  const lyrics = useSongLyrics(root, song ?? undefined)
  const position = useSmoothPosition()
  const at = lineAt(lyrics, position)
  const progress = lineProgress(lyrics, at, position)
  return { song, root, playing, lyrics, position, at, progress }
}

/** A line in Roman letters, when it is written in an Indian script and that was asked for. */
export const latinOf = (line: string | undefined, want: boolean) =>
  want && line && INDIC.test(line) ? toLatin(line) : undefined

/** The words have any line in an Indian script (so the Roman letters switch means something). */
export const hasIndic = (lines: [number, string][] | undefined) => !!lines?.some(([, l]) => INDIC.test(l))

/**
 * The colour lyrics are drawn in: the playing cover's own colour (made to read on what's behind
 * the words), the theme's accent, or plain: white on dark, the theme's ink on light.
 */
export function useLyricColour(choice: LyricColour, root: string, dir: string | undefined, ground: Ground = 'dark'): string {
  const cover = useCovers(320)
  useEffect(() => {
    if (choice === 'cover' && root && dir) void loadCovers(root, [dir])
  }, [choice, root, dir])
  const seed = dir ? cover(dir)?.accent : undefined
  return useMemo(() => {
    if (choice === 'white') return ground === 'dark' ? '#ffffff' : 'var(--ink)'
    if (choice === 'accent') return 'var(--accent)'
    const roles = seed ? accentRoles(seed, ground === 'light', 'vivid') : null
    return roles?.accent ?? 'var(--accent)'
  }, [choice, seed, ground])
}

/** The playing song's cover, large: your file's, or the catalog's picture. */
export function useWindowCover(): { src: string } | undefined {
  const { song, root } = useNowLyrics()
  const localDir = song && !isOnline(song) ? song.dir : ''
  const local = useCovers(640)(localDir)
  useEffect(() => {
    if (root && localDir) void loadCovers(root, [localDir], 640)
  }, [root, localDir])
  const remote = song && isOnline(song) ? artUrl(song.artwork_url, 800) : undefined
  return remote ? { src: remote } : local
}
