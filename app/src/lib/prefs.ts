import { useEffect, useState } from 'react'
import { inApp } from './engine'

/*
  Look and feel, kept on this computer. Applied as attributes on <html> (see index.css), so
  switching is instant and every screen follows without code of its own.
*/
export type Theme = 'dark' | 'light' | 'system'
export type Surface = 'glass' | 'frosted' | 'solid' | 'amoled'
export type AccentMode = 'album' | 'fixed'
export type Motion = 'full' | 'reduced' | 'system'

export interface Prefs {
  theme: Theme
  surface: Surface
  accentMode: AccentMode
  accent: string
  backdrop: boolean
  motion: Motion
}

/** Fixed accents, each readable on both themes (light darkens it for text). */
export const ACCENTS: { name: string; hex: string }[] = [
  { name: 'Amber', hex: '#f0a85a' },
  { name: 'Rose', hex: '#ec6a95' },
  { name: 'Violet', hex: '#9c88ff' },
  { name: 'Periwinkle', hex: '#a6b8ff' },
  { name: 'Sky', hex: '#5aaef0' },
  { name: 'Teal', hex: '#3fc2a8' },
  { name: 'Lime', hex: '#a3d65a' },
  { name: 'Coral', hex: '#ff8a65' },
]

const DEFAULTS: Prefs = {
  theme: 'dark',
  surface: 'glass',
  accentMode: 'album',
  accent: '#a6b8ff',
  backdrop: true,
  motion: 'system',
}

const KEY = 'appearance'
let current: Prefs = load()
const listeners = new Set<(p: Prefs) => void>()

function load(): Prefs {
  try {
    const raw = localStorage.getItem(KEY)
    return raw ? { ...DEFAULTS, ...(JSON.parse(raw) as Partial<Prefs>) } : DEFAULTS
  } catch {
    return DEFAULTS
  }
}

export const getPrefs = () => current

export function setPrefs(change: Partial<Prefs>) {
  current = { ...current, ...change }
  try {
    localStorage.setItem(KEY, JSON.stringify(current))
  } catch {
    // Not remembered this time; the change still applies now.
  }
  applyPrefs()
  listeners.forEach((l) => l(current))
}

/** The theme actually showing ("system" resolved). */
export function resolvedTheme(p = current): 'dark' | 'light' {
  if (p.theme !== 'system') return p.theme
  return window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark'
}

export function applyPrefs() {
  const p = current
  const el = document.documentElement
  const theme = resolvedTheme(p)
  el.dataset.theme = theme
  el.dataset.surface = p.surface
  el.dataset.backdrop = p.backdrop ? 'on' : 'off'
  el.dataset.motion = p.motion === 'reduced' ? 'reduced' : 'full'
  if (p.accentMode === 'fixed') el.style.setProperty('--accent', p.accent)
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

/*
  Album colours vary wildly; keep the accent inside a band that reads on the current theme.
*/
export function tuneAccent(hex: string): string {
  const m = /^#?([0-9a-f]{6})$/i.exec(hex.trim())
  if (!m) return hex
  const n = parseInt(m[1], 16)
  const [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((v) => v / 255)
  const max = Math.max(r, g, b)
  const min = Math.min(r, g, b)
  let h = 0
  const l = (max + min) / 2
  const d = max - min
  let s = d === 0 ? 0 : d / (1 - Math.abs(2 * l - 1))
  if (d !== 0) {
    if (max === r) h = ((g - b) / d) % 6
    else if (max === g) h = (b - r) / d + 2
    else h = (r - g) / d + 4
    h *= 60
    if (h < 0) h += 360
  }
  const light = resolvedTheme() === 'light'
  const L = Math.min(Math.max(l, light ? 0.5 : 0.64), light ? 0.62 : 0.8)
  s = Math.max(s, 0.45)
  const c = (1 - Math.abs(2 * L - 1)) * s
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1))
  const mm = L - c / 2
  const [r1, g1, b1] =
    h < 60 ? [c, x, 0] : h < 120 ? [x, c, 0] : h < 180 ? [0, c, x] : h < 240 ? [0, x, c] : h < 300 ? [x, 0, c] : [c, 0, x]
  const to = (v: number) =>
    Math.round((v + mm) * 255)
      .toString(16)
      .padStart(2, '0')
  return `#${to(r1)}${to(g1)}${to(b1)}`
}
