import { useEffect, useState } from 'react'
import { albumCovers, assetUrl, type CoverThumb } from './engine'
import { fixedAccent, getPrefs, resolvedTheme } from './prefs'
import { colourDistance, nearestBlock, tuneAccent } from './themes'

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
  if (p.style === 'bounty') {
    const block = nearestBlock(hex ?? '')
    root.setProperty('--block', block)
    root.setProperty('--accent', block)
    return
  }
  if (p.style !== 'venuta') return
  const next = p.accentMode === 'fixed' || !hex ? fixedAccent(p) : (tuneAccent(hex, resolvedTheme(p) === 'light') ?? fixedAccent(p))
  // Similar covers (one album's songs) keep one accent instead of shimmering between near-twins.
  if (shown && root.getPropertyValue('--accent').trim() === shown && colourDistance(shown, next) < 0.06) return
  shown = next
  root.setProperty('--accent', next)
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
