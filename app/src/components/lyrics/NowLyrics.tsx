import { createContext, useContext, useMemo, type ReactNode } from 'react'
import { lineAt, lineProgress, useSmoothPosition, useSongLyrics, type Lyrics } from '../../lib/lyrics'
import { current, usePlayerValue, type QueueItem } from '../../lib/player'
import { useSecondLines, type SecondLinesView } from '../../lib/useSecondLines'

/*
  One lyrics clock per window. The provider reads the playing song and its words once, works
  out their second lines once, and runs the one frame loop (the smooth position); every part of
  the window reads them from here. The song changes rarely and the clock every frame, so they are
  two contexts: the frame (background, bars) reads only the song and is not drawn again each
  frame; only the lines read the clock.
*/

export interface NowSong {
  song: QueueItem | null
  root: string
  playing: boolean
  /** Your file's words, else the lyrics services' (null while they're read). */
  lyrics: Lyrics | null
  seconds: SecondLinesView
}

export interface NowClock {
  position: number
  /** The line being sung (-1 before the first). */
  at: number
  /** How far through it, 0 to 1 (the karaoke fill). */
  progress: number
}

const SongContext = createContext<NowSong | null>(null)
const ClockContext = createContext<NowClock>({ position: 0, at: -1, progress: 0 })

export function NowLyricsProvider({ children }: { children: ReactNode }) {
  const song = usePlayerValue(current) ?? null
  const root = usePlayerValue((s) => s.root ?? '')
  const playing = usePlayerValue((s) => s.playing)
  const lyrics = useSongLyrics(root, song ?? undefined)
  const seconds = useSecondLines(song, lyrics)
  const position = useSmoothPosition()
  const at = lineAt(lyrics, position)
  const progress = lineProgress(lyrics, at, position)
  const now = useMemo(() => ({ song, root, playing, lyrics, seconds }), [song, root, playing, lyrics, seconds])
  const clock = useMemo(() => ({ position, at, progress }), [position, at, progress])
  return (
    <SongContext value={now}>
      <ClockContext value={clock}>{children}</ClockContext>
    </SongContext>
  )
}

/** The song playing, its words and their second lines (no clock: not drawn again each frame). */
export function useNowSong(): NowSong {
  const now = useContext(SongContext)
  if (!now) throw new Error('useNowSong outside NowLyricsProvider')
  return now
}

/** The song, its words, the line being sung and how far through it, smoothly (each frame). */
export function useNowLyrics(): NowSong & NowClock {
  return { ...useNowSong(), ...useContext(ClockContext) }
}
