// How lyrics look on the big screen and on the desktop: one choice, shared by every window (it
// is kept on this computer and every window hears when it changes).

import { useEffect, useState } from 'react'

export type LookId = 'clean' | 'karaoke' | 'glow' | 'outline' | 'serif' | 'theme'
export type LyricSize = 's' | 'm' | 'l' | 'xl'
/** The playing cover's colour, the theme's accent, or plain white. */
export type LyricColour = 'cover' | 'accent' | 'white'

export interface LyricLook {
  look: LookId
  size: LyricSize
  colour: LyricColour
  /** Lines in Indian scripts shown in Roman letters too. */
  latin: boolean
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

const DEFAULT: LyricLook = { look: 'karaoke', size: 'm', colour: 'cover', latin: false, lines: 2 }
const KEY = 'lyric-look'

function read(): LyricLook {
  try {
    const raw = localStorage.getItem(KEY)
    return raw ? { ...DEFAULT, ...(JSON.parse(raw) as Partial<LyricLook>) } : DEFAULT
  } catch {
    return DEFAULT
  }
}

let current = read()
const listeners = new Set<(l: LyricLook) => void>()

export function setLyricLook(change: Partial<LyricLook>) {
  current = { ...current, ...change }
  try {
    localStorage.setItem(KEY, JSON.stringify(current))
  } catch {
    // Not kept (private storage): it still applies until the window closes.
  }
  listeners.forEach((l) => l(current))
}

// Another window changed it.
if (typeof window !== 'undefined')
  window.addEventListener('storage', (e) => {
    if (e.key !== KEY) return
    current = read()
    listeners.forEach((l) => l(current))
  })

export function useLyricLook(): LyricLook {
  const [look, setLook] = useState(current)
  useEffect(() => {
    listeners.add(setLook)
    return () => {
      listeners.delete(setLook)
    }
  }, [])
  return look
}

export const scaleOf = (size: LyricSize) => SIZES.find((s) => s.id === size)?.scale ?? 1
