import { useEffect, useState } from 'react'
import { inApp, saveAppearance } from './engine'
import { ACCENTS, accentByName, colourDistance, styleInfo, type Style } from './themes'

/*
  Look and feel, kept on this computer. Applied as attributes on <html> (see index.css), so
  switching is instant and every screen follows without code of its own.
*/
export type Theme = 'dark' | 'light' | 'system'
export type Surface = 'glass' | 'frosted' | 'solid' | 'amoled'
export type AccentMode = 'album' | 'fixed'
export type Motion = 'full' | 'reduced' | 'system'

export interface Prefs {
  /** The theme: Venuta (glass, follows the album) or one of the retro looks. */
  style: Style
  theme: Theme
  surface: Surface
  accentMode: AccentMode
  /** A fixed accent's name (see ACCENTS in lib/themes.ts); also the fallback for grey covers. */
  accent: string
  backdrop: boolean
  motion: Motion
}

const DEFAULTS: Prefs = {
  style: 'venuta',
  theme: 'dark',
  surface: 'glass',
  accentMode: 'album',
  accent: 'Azure',
  backdrop: true,
  motion: 'system',
}

const KEY = 'appearance'
let current: Prefs = load()
const listeners = new Set<(p: Prefs) => void>()

function load(): Prefs {
  try {
    const raw = localStorage.getItem(KEY)
    return raw ? upgrade({ ...DEFAULTS, ...(JSON.parse(raw) as Partial<Prefs>) }) : DEFAULTS
  } catch {
    return DEFAULTS
  }
}

/** Older saves kept the accent as a colour; the accents are named now: take the nearest. */
function upgrade(p: Prefs): Prefs {
  if (!p.accent.startsWith('#')) return p
  const nearest = ACCENTS.reduce((a, b) => (colourDistance(p.accent, b.dark) < colourDistance(p.accent, a.dark) ? b : a))
  return { ...p, accent: nearest.name }
}

export const getPrefs = () => current

/**
 * Change the look. It is kept with your library (so it travels with it) and also on this
 * computer, so the very first frame on start already has it.
 */
export function setPrefs(change: Partial<Prefs>, opts: { save?: boolean } = {}) {
  current = { ...current, ...change }
  try {
    localStorage.setItem(KEY, JSON.stringify(current))
  } catch {
    // Not remembered on this computer this time; the library copy still is.
  }
  if (opts.save !== false) void saveAppearance(current).catch(() => {})
  applyPrefs()
  listeners.forEach((l) => l(current))
}

/** Take the look saved with the library (on start), without saving it straight back. */
export function adoptPrefs(saved: Partial<Prefs> | null | undefined) {
  if (saved) setPrefs(upgrade({ ...DEFAULTS, ...saved }), { save: false })
}

/** The theme actually showing ("system" resolved). */
export function resolvedTheme(p = current): 'dark' | 'light' {
  // The retro looks are one palette each (Bounty Session has a day and a night).
  if (!styleInfo(p.style).modes) return 'dark'
  if (p.theme !== 'system') return p.theme
  return window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark'
}

export function applyPrefs() {
  const p = current
  const el = document.documentElement
  const theme = resolvedTheme(p)
  el.dataset.style = p.style
  el.dataset.theme = theme
  el.dataset.surface = p.surface
  el.dataset.backdrop = p.backdrop ? 'on' : 'off'
  el.dataset.motion = p.motion === 'reduced' ? 'reduced' : 'full'
  if (p.style !== 'venuta') {
    // The retro themes bring their own accent (Bounty's follows the album, in its blocks).
    el.style.removeProperty('--accent')
    if (p.style !== 'bounty') el.style.removeProperty('--block')
  } else {
    // Fixed, or the fallback until an album's colour arrives.
    el.style.setProperty('--accent', fixedAccent(p))
  }
  // The window's own glass (acrylic) follows the theme too.
  if (inApp()) {
    import('@tauri-apps/api/window')
      .then(({ getCurrentWindow }) => getCurrentWindow().setTheme(theme))
      .catch(() => {})
  }
}

// Follow Windows when the theme is "Match Windows".
try {
  window.matchMedia('(prefers-color-scheme: light)').addEventListener('change', () => {
    if (current.theme === 'system') {
      applyPrefs()
      listeners.forEach((l) => l(current))
    }
  })
} catch {
  // Older engines: the theme updates on the next change instead.
}

export function usePrefs(): Prefs {
  const [p, set] = useState(current)
  useEffect(() => {
    listeners.add(set)
    return () => {
      listeners.delete(set)
    }
  }, [])
  return p
}

/** Motion's reduced-motion setting for <MotionConfig>. */
export const motionSetting = (p: Prefs) =>
  p.motion === 'reduced' ? 'always' : p.motion === 'full' ? 'never' : 'user'

/** The chosen fixed accent, in the version for the theme showing. */
export function fixedAccent(p = current): string {
  const a = accentByName(p.accent)
  return resolvedTheme(p) === 'light' ? a.light : a.dark
}

// Development builds only: lets the screenshot scripts try a look without saving it.
if (import.meta.env.DEV) {
  ;(window as unknown as { __venutaPrefs: unknown }).__venutaPrefs = {
    get: () => current,
    try: (change: Partial<Prefs>) => {
      current = { ...current, ...change }
      applyPrefs()
      listeners.forEach((l) => l(current))
    },
  }
}
