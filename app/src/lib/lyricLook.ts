// How lyrics look on the big screen and on the desktop: each window its own (kept on this
// computer; every window hears when either changes). The look from when they shared one is where
// both begin.

import { useEffect, useState } from 'react'

export type LookId = 'clean' | 'karaoke' | 'glow' | 'outline' | 'serif' | 'theme'
export type LyricSize = 's' | 'm' | 'l' | 'xl'
/** The playing cover's colour, the theme's accent, or plain white. */
export type LyricColour = 'cover' | 'accent' | 'white'

export interface LyricLook {
  look: LookId
  size: LyricSize
  colour: LyricColour
  /** Desktop lyrics: one line, or the next one under it. */
  lines: 1 | 2
}

export const LOOKS: { id: LookId; name: string; line: string }[] = [
  { id: 'clean', name: 'Clean', line: 'Bold and plain, with a soft shadow.' },
  { id: 'karaoke', name: 'Karaoke', line: 'Each line fills in as it is sung.' },
  { id: 'glow', name: 'Glow', line: 'Lit in the colour, for dark rooms.' },
  { id: 'outline', name: 'Outline', line: 'Readable over any picture or window.' },
  { id: 'serif', name: 'Serif', line: 'Quiet and bookish, for slow songs.' },
  { id: 'theme', name: 'Theme', line: "In the theme's own type." },
]

export const SIZES: { id: LyricSize; name: string; scale: number }[] = [
  { id: 's', name: 'Small', scale: 0.75 },
  { id: 'm', name: 'Medium', scale: 1 },
  { id: 'l', name: 'Large', scale: 1.3 },
  { id: 'xl', name: 'Huge', scale: 1.7 },
]

/** What's behind the words: dark (the desktop, the cover's light, a dark theme) or a light theme. */
export type Ground = 'dark' | 'light'

/** The two lyrics windows: the big screen (`#stage`) and the desktop lyrics (`#overlay`). */
export type LyricWindow = 'stage' | 'overlay'
export const LYRIC_WINDOWS: LyricWindow[] = ['stage', 'overlay']

// (Its "Roman letters too" switch became Pronunciation, one choice for every view: lib/secondChoice.ts.)
const DEFAULT: LyricLook = { look: 'karaoke', size: 'm', colour: 'cover', lines: 2 }
/** The look when both windows shared one: where each begins. */
const SHARED = 'lyric-look'
const keyOf = (w: LyricWindow) => `lyric-look-${w}`

/** A window's look from what's kept (`get` reads a key): its own, else the shared one from
 *  before, else the default. Anything unreadable falls back the same way. */
export function readLook(w: LyricWindow, get: (key: string) => string | null): LyricLook {
  for (const key of [keyOf(w), SHARED]) {
    try {
      const raw = get(key)
      if (raw) return { ...DEFAULT, ...(JSON.parse(raw) as Partial<LyricLook>) }
    } catch {
      // On to the next.
    }
  }
  return DEFAULT
}

const stored = (key: string) => {
  try {
    return localStorage.getItem(key)
  } catch {
    return null
  }
}

const current: Record<LyricWindow, LyricLook> = { stage: readLook('stage', stored), overlay: readLook('overlay', stored) }
const listeners = new Set<() => void>()

export function setLyricLook(w: LyricWindow, change: Partial<LyricLook>) {
  current[w] = { ...current[w], ...change }
  try {
    localStorage.setItem(keyOf(w), JSON.stringify(current[w]))
  } catch {
    // Not kept (private storage): it still applies until the window closes.
  }
  listeners.forEach((l) => l())
}

// Another window changed one.
if (typeof window !== 'undefined')
  window.addEventListener('storage', (e) => {
    const w = LYRIC_WINDOWS.find((x) => e.key === keyOf(x))
    if (!w) return
    current[w] = readLook(w, stored)
    listeners.forEach((l) => l())
  })

export function useLyricLook(w: LyricWindow): LyricLook {
  const [, tick] = useState(0)
  useEffect(() => {
    const l = () => tick((n) => n + 1)
    listeners.add(l)
    return () => {
      listeners.delete(l)
    }
  }, [])
  return current[w]
}

export const scaleOf = (size: LyricSize) => SIZES.find((s) => s.id === size)?.scale ?? 1
