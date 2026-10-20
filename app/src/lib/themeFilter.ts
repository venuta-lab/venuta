/*
  The filters over the theme gallery in Look and feel: where a theme comes from (built in, made
  or added by you, used lately) and which modes it has. Pure, so the order and the rules can be
  tested without a screen.
*/

export type Source = 'all' | 'built' | 'created' | 'recent'
export type ModeFilter = 'all' | 'dark' | 'light'
type Mode = 'dark' | 'light'

export const SOURCES: { id: Source; label: string }[] = [
  { id: 'all', label: 'All' },
  { id: 'built', label: 'Built-in' },
  { id: 'created', label: 'Created' },
  { id: 'recent', label: 'Recent' },
]

export const MODE_FILTERS: { id: ModeFilter; label: string }[] = [
  { id: 'all', label: 'All' },
  { id: 'dark', label: 'Dark' },
  { id: 'light', label: 'Light' },
]

/** What the filters need to know of a theme. */
export interface Filterable {
  /** "style:venuta" or "pack:dusk" (lib/recentThemes.ts). */
  key: string
  kind: 'built' | 'yours'
  modes: readonly Mode[]
  /** False for a theme of yours that can't load: listed under Created, never under Recent. */
  usable: boolean
}

/**
 * The themes to show. Every source keeps the order given (built in first), except Recent:
 * newest first, as `recent` (keys, newest first) lists them.
 */
export function filterThemes<T extends Filterable>(list: T[], f: { source: Source; mode: ModeFilter }, recent: readonly string[]): T[] {
  let from: T[]
  if (f.source === 'recent') {
    const byKey = new Map(list.map((e) => [e.key, e]))
    const seen = new Set<string>()
    from = []
    for (const k of recent) {
      const e = byKey.get(k)
      if (!e || !e.usable || seen.has(k)) continue
      seen.add(k)
      from.push(e)
    }
  } else if (f.source === 'built') from = list.filter((e) => e.kind === 'built')
  else if (f.source === 'created') from = list.filter((e) => e.kind === 'yours')
  else from = list
  return f.mode === 'all' ? from : from.filter((e) => e.modes.includes(f.mode as Mode))
}

/** A theme's modes in words: "light and dark", "dark" or "light". */
export const modesWord = (modes: readonly Mode[]) => (modes.includes('dark') && modes.includes('light') ? 'light and dark' : modes.includes('light') ? 'light' : 'dark')

/** A card's hover line: what the theme is, then where it comes from and its modes. */
export function themeHint(t: { line: string; kind: 'built' | 'yours' | 'venuta'; base?: string; modes: readonly Mode[] }): string {
  const from = t.kind === 'built' ? 'Built in' : `${t.kind === 'yours' ? 'Yours' : 'From Venuta'}, built on ${t.base}`
  const about = `${from}, ${modesWord(t.modes)}`
  return t.line ? `${t.line}\n${about}` : about
}
