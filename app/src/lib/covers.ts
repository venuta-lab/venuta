import { useEffect, useState } from 'react'
import { albumCovers, assetUrl, type CoverThumb } from './engine'
import { chosenRoles, getPrefs, resolvedTheme, setAccentVars } from './prefs'
import { accentRoles, artTone, colourDistance, nearestBlock, neutralRoles, styleInfo, type AccentRoles } from './themes'

export interface Cover {
  src: string
  accent: string | null
}

const cache = new Map<string, Cover>()
const listeners = new Set<() => void>()

/**
 * Fetch covers for album folders in batches; results are cached for the session.
 * `onProgress` counts albums handled (with or without a cover found).
 */
export async function loadCovers(
  root: string,
  dirs: string[],
  size = 320,
  onProgress?: (done: number, total: number) => void,
) {
  const missing = dirs.filter((d) => !cache.has(`${size}:${d}`))
  onProgress?.(0, missing.length)
  for (let i = 0; i < missing.length; i += 48) {
    const batch = missing.slice(i, i + 48)
    let thumbs: CoverThumb[] = []
    try {
      thumbs = await albumCovers(root, batch, size)
    } catch {
      thumbs = []
    }
    for (const t of thumbs) cache.set(`${size}:${t.dir}`, { src: assetUrl(t.path), accent: t.accent })
    listeners.forEach((l) => l())
    onProgress?.(Math.min(missing.length, i + batch.length), missing.length)
  }
}

/** Re-render when new covers arrive; returns a lookup for one size. */
export function useCovers(size = 320) {
  const [, tick] = useState(0)
  useEffect(() => {
    const l = () => tick((n) => n + 1)
    listeners.add(l)
    return () => {
      listeners.delete(l)
    }
  }, [])
  return (dir: string) => cache.get(`${size}:${dir}`)
}

/*
  The accent follows one thing at a time: the album page you have open (its glow is that cover),
  else the song that's playing, else your fixed accent. Never what the pointer happens to cross:
  that made the whole app flicker colour while you browsed.
*/
let playing: string | null = null
let page: string | null = null
let shown = ''

function apply() {
  const p = getPrefs()
  const root = document.documentElement.style
  const hex = page ?? playing
  // The cover's colour as a tint, for the themes that colour a page with it (every theme gets it).
  if (hex) root.setProperty('--art', artTone(hex))
  else root.removeProperty('--art')
  const accent = styleInfo(p.style).accent
  if (accent === 'blocks') {
    const block = nearestBlock(hex ?? '')
    root.setProperty('--block', block)
    root.setProperty('--accent', block)
    return
  }
  if (accent !== 'yours' && accent !== 'cover') return
  // A cover theme always follows the cover (its own colour when there is none); Lumen as you chose.
  const lumen = accent === 'yours'
  const light = resolvedTheme(p) === 'light'
  let next: AccentRoles | null
  if (lumen && p.accentMode === 'neutral') next = neutralRoles(light)
  else if (lumen && p.accentMode === 'fixed') next = chosenRoles(p)
  else next = (hex ? accentRoles(hex, light, lumen ? p.intensity : 'balanced') : null) ?? (lumen ? chosenRoles(p) : null)
  if (!next) {
    shown = ''
    setAccentVars(null)
    return
  }
  // Similar covers (one album's songs) keep one accent instead of shimmering between near-twins.
  if (shown && root.getPropertyValue('--accent').trim() === shown && colourDistance(shown, next.accent) < 0.04) return
  shown = next.accent
  setAccentVars(next)
}

/** The playing song's cover colour (set by the app when the song changes). */
export function setPlayingAccent(hex: string | null | undefined) {
  playing = hex ?? null
  apply()
}

/** While an album page is open, its colour leads; cleared when it closes. */
export function usePageAccent(hex: string | null | undefined) {
  useEffect(() => {
    page = hex ?? null
    apply()
    return () => {
      page = null
      apply()
    }
  }, [hex])
}

/** Again after the look changes (theme, accent mode), so the right variant shows. */
export const refreshAccent = () => {
  shown = ''
  apply()
}

/** Cached cover for one album, if loaded. */
export const coverFor = (dir: string, size = 320) => cache.get(`${size}:${dir}`)
