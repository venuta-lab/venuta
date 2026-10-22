import { useEffect, useMemo } from 'react'
import { loadCovers, useCovers } from '../../lib/covers'
import type { Ground, LyricColour } from '../../lib/lyricLook'
import { isOnline } from '../../lib/player'
import { artUrl } from '../RemoteArt'
import { accentRoles } from '../../lib/themes'
import { useNowSong } from './NowLyrics'

// The song, its words and the clock come from the window's one provider (NowLyrics.tsx).
export { useNowLyrics, useNowSong } from './NowLyrics'

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
  const { song, root } = useNowSong()
  const localDir = song && !isOnline(song) ? song.dir : ''
  const local = useCovers(640)(localDir)
  useEffect(() => {
    if (root && localDir) void loadCovers(root, [localDir], 640)
  }, [root, localDir])
  const remote = song && isOnline(song) ? artUrl(song.artwork_url, 800) : undefined
  return remote ? { src: remote } : local
}
