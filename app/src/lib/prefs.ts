import { useEffect, useState } from 'react'
import { inApp, saveAppearance } from './engine'
import { accentByName, accentRoles, neutralRoles, styleInfo, type AccentRoles, type Intensity, type Style, type StyleInfo } from './themes'
import { draftBase, onPackChange, packInfoFor, packModes, showPack } from './packs'
import { noteTheme, themeKey } from './recentThemes'

/*
  Look and feel, kept on this computer. Applied as attributes on <html> (see index.css), so
  switching is instant and every screen follows without code of its own.
*/
export type Theme = 'dark' | 'light' | 'system'
export type Surface = 'glass' | 'frosted' | 'solid' | 'amoled'
/** From the music playing (or the album open), one colour you keep, or none at all. */
export type AccentMode = 'album' | 'fixed' | 'neutral'
export type Motion = 'full' | 'reduced' | 'system'
/** The player bar's seek bar: the song's waveform in the theme's colours, or the theme's own. */
export type SeekChoice = 'wave' | 'theme'

export interface Prefs {
  /** The theme: Venuta (glass, follows the album) or one of the retro looks. With a theme of
   *  yours, the built-in theme it extends. */
  style: Style
  /** A theme of yours (its id in the library's .venuta/themes), or '' for the built-in one. */
  pack: string
  theme: Theme
  surface: Surface
  accentMode: AccentMode
  /** A swatch's name (see ACCENTS in lib/themes.ts) or your own colour as #rrggbb; also the
   *  colour for grey covers. */
  accent: string
  /** How colourful the accent may be. */
  intensity: Intensity
  backdrop: boolean
  motion: Motion
  seekBar: SeekChoice
}

const DEFAULTS: Prefs = {
  style: 'venuta',
  pack: '',
  theme: 'dark',
  surface: 'glass',
  accentMode: 'album',
  accent: 'Iris',
  intensity: 'balanced',
  backdrop: true,
  motion: 'system',
  seekBar: 'wave',
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

/** Older saves: a swatch that was renamed finds its nearest (a colour kept as #rrggbb is yours). */
function upgrade(p: Prefs): Prefs {
  const style = styleInfo(p.style).id
  if (style !== p.style) p = { ...p, style }
  if (typeof p.pack !== 'string') p = { ...p, pack: '' }
  if (p.seekBar !== 'wave' && p.seekBar !== 'theme') p = { ...p, seekBar: 'wave' }
  if (p.accent.startsWith('#') || accentByName(p.accent).name === p.accent) return p
  return { ...p, accent: accentByName(p.accent).name }
}

export const getPrefs = () => current

/**
 * Change the look. It is kept with your library (so it travels with it) and also on this
 * computer, so the very first frame on start already has it.
 */
export function setPrefs(change: Partial<Prefs>, opts: { save?: boolean } = {}) {
  const was = themeKey(current.style, current.pack)
  current = { ...current, ...change }
  // A theme chosen: it and the one before it lead the recent row (Settings).
  const now = themeKey(current.style, current.pack)
  if (opts.save !== false && now !== was) {
    noteTheme(was)
    noteTheme(now)
  }
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

/**
 * What the theme on screen does: a theme of yours (once loaded), or the built-in one. Code asks
 * this, never a theme's name.
 */
export const themeInfo = (p = current): StyleInfo => packInfoFor(p.pack) ?? styleInfo(p.style)

/** What the theme on screen does, kept up to date in a component. */
export function useThemeInfo(): StyleInfo {
  return themeInfo(usePrefs())
}

/** The theme actually showing ("system" resolved). */
export function resolvedTheme(p = current): 'dark' | 'light' {
  // A theme of yours made for one mode shows in it.
  const own = packModes(p.pack)
  if (own?.length === 1) return own[0]
  // The retro looks are one palette each (Bounty Session has a day and a night).
  if (!themeInfo(p).modes) return 'dark'
  if (p.theme !== 'system') return p.theme
  return window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark'
}

/** Whether to hold things still: chosen in Venuta, or Windows' setting when it matches Windows. */
export const reducedMotion = (p = current) =>
  p.motion === 'reduced' || (p.motion === 'system' && window.matchMedia('(prefers-reduced-motion: reduce)').matches)

/** Windows has "Transparency effects" turned off. */
export const transparencyOff = () => window.matchMedia('(prefers-reduced-transparency: reduce)').matches

/** The surface on screen: see-through ones turn Solid when Windows turns transparency off. */
export const surfaceShown = (p = current): Surface =>
  transparencyOff() && (p.surface === 'glass' || p.surface === 'frosted') ? 'solid' : p.surface

export function applyPrefs() {
  const p = current
  const el = document.documentElement
  // A theme of yours first: what it does decides the rest.
  showPack(p.pack)
  const info = themeInfo(p)
  const theme = resolvedTheme(p)
  el.dataset.style = styleInfo(draftBase() ?? p.style).id
  el.dataset.theme = theme
  // A theme of yours brings its own surfaces; the choice of glass, frosted… is Lumen's.
  el.dataset.surface = info.surfaces ? surfaceShown(p) : 'glass'
  // Neutral is a material in Lumen (clear glass), not a white colour: CSS asks this.
  el.dataset.accent = p.accentMode
  el.dataset.backdrop = p.backdrop ? 'on' : 'off'
  // "Match Windows" follows Windows' animation setting, so CSS only has to ask this attribute.
  el.dataset.motion = reducedMotion(p) ? 'reduced' : 'full'
  const accent = info.accent
  if (accent !== 'yours') {
    // The other themes bring their own accent (Bounty's follows the album, in its blocks;
    // Glasshouse's the cover, set by lib/covers.ts).
    setAccentVars(null)
    if (accent !== 'blocks') el.style.removeProperty('--block')
  } else {
    // The colour you chose, or the fallback until an album's colour arrives.
    setAccentVars(chosenRoles(p))
  }
  // The window's own glass (acrylic) follows the theme too.
  if (inApp()) {
    import('@tauri-apps/api/window')
      .then(({ getCurrentWindow }) => getCurrentWindow().setTheme(theme))
      .catch(() => {})
  }
}

// A theme of yours loaded, changed on disk, or couldn't load: apply the look again.
onPackChange(() => {
  applyPrefs()
  listeners.forEach((l) => l(current))
})

// Another window (the mini player, or the main one) changed the look: follow it here too.
try {
  window.addEventListener('storage', (e) => {
    if (e.key !== KEY) return
    current = load()
    applyPrefs()
    listeners.forEach((l) => l(current))
  })
} catch {
  // No storage events: this window catches up when it next opens.
}

// Follow Windows when the theme or motion is "Match Windows".
try {
  window.matchMedia('(prefers-color-scheme: light)').addEventListener('change', () => {
    if (current.theme === 'system') {
      applyPrefs()
      listeners.forEach((l) => l(current))
    }
  })
  window.matchMedia('(prefers-reduced-transparency: reduce)').addEventListener('change', () => {
    applyPrefs()
    listeners.forEach((l) => l(current))
  })
  window.matchMedia('(prefers-reduced-motion: reduce)').addEventListener('change', () => {
    if (current.motion === 'system') {
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

/** The colour you chose (a swatch or your own), as it shows on the theme: also the colour for grey covers. */
export function chosenRoles(p = current): AccentRoles {
  const light = resolvedTheme(p) === 'light'
  if (p.accentMode === 'neutral') return neutralRoles(light)
  const seed = p.accent.startsWith('#') ? p.accent : accentByName(p.accent).seed
  return accentRoles(seed, light, p.intensity) ?? neutralRoles(light)
}

/** Put an accent's colours on the page (null: the theme's own, from its CSS). */
export function setAccentVars(r: AccentRoles | null) {
  const st = document.documentElement.style
  if (!r) {
    for (const v of ['--accent', '--on-accent', '--accent-ink']) st.removeProperty(v)
    return
  }
  st.setProperty('--accent', r.accent)
  st.setProperty('--on-accent', r.onAccent)
  st.setProperty('--accent-ink', r.ink)
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
