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

let shown = ''
/**
 * Move the app's accent to an album's colour (CSS glides it over 600 ms). A fixed accent wins;
 * the retro themes keep their palette (Bounty Session snaps it to its colour blocks). A grey
 * cover falls back to your fixed accent, and a change too small to notice is skipped.
 */
export function setAccent(hex: string | null | undefined) {
  const p = getPrefs()
  const root = document.documentElement.style
  if (p.style === 'bounty') {
    const block = nearestBlock(hex ?? '')
    root.setProperty('--block', block)
    root.setProperty('--accent', block)
    return
  }
  if (!hex || p.style !== 'venuta' || p.accentMode === 'fixed') return
  const next = tuneAccent(hex, resolvedTheme(p) === 'light') ?? fixedAccent(p)
  if (shown && root.getPropertyValue('--accent').trim() === shown && colourDistance(shown, next) < 0.06) return
  shown = next
  root.setProperty('--accent', next)
}

/** Cached cover for one album, if loaded. */
export const coverFor = (dir: string, size = 320) => cache.get(`${size}:${dir}`)
