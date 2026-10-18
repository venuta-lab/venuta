import { useEffect, useMemo } from 'react'
import { loadCovers, useCovers } from '../../lib/covers'
import { lineAt, lineProgress, useSmoothPosition, useSongLyrics } from '../../lib/lyrics'
import type { LyricColour } from '../../lib/lyricLook'
import { current, usePlayerValue } from '../../lib/player'
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
 * The colour lyrics are drawn in: the playing cover's own colour (made bright enough to read on
 * dark), the theme's accent, or white.
 */
export function useLyricColour(choice: LyricColour, root: string, dir: string | undefined): string {
  const cover = useCovers(320)
  useEffect(() => {
    if (choice === 'cover' && root && dir) void loadCovers(root, [dir])
  }, [choice, root, dir])
  const seed = dir ? cover(dir)?.accent : undefined
  return useMemo(() => {
    if (choice === 'white') return '#ffffff'
    if (choice === 'accent') return 'var(--accent)'
    const roles = seed ? accentRoles(seed, false, 'vivid') : null
    return roles?.accent ?? 'var(--accent)'
  }, [choice, seed])
}
