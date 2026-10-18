/*
  Picking songs in a list to act on them together ("Select" on an album or a playlist): a click
  picks one more or lets it go, Shift picks everything from the last one picked, and what is
  acted on keeps the list's order. Pure (no app imports), so tests/selection.test.ts reads it.
*/

/** What is picked, by each song's key, and the last one clicked (where Shift ranges from). */
export interface Picks {
  keys: string[]
  anchor: string | null
}

export const NO_PICKS: Picks = { keys: [], anchor: null }

/** Click `key` in `list`: toggles it, or with `range` picks from the anchor to it. */
export function pick(p: Picks, list: string[], key: string, opts: { range?: boolean } = {}): Picks {
  if (opts.range && p.anchor !== null) {
    const a = list.indexOf(p.anchor)
    const b = list.indexOf(key)
    if (a >= 0 && b >= 0) {
      const [lo, hi] = a <= b ? [a, b] : [b, a]
      const set = new Set([...p.keys, ...list.slice(lo, hi + 1)])
      // In the list's order; earlier picks no longer in it stay at the end.
      const keys = [...list.filter((k) => set.has(k)), ...p.keys.filter((k) => !list.includes(k))]
      return { keys, anchor: key }
    }
  }
  const has = p.keys.includes(key)
  return { keys: has ? p.keys.filter((k) => k !== key) : [...p.keys, key], anchor: key }
}

/** Pick the whole list, or let all go when it is all picked already. */
export function pickAll(p: Picks, list: string[]): Picks {
  const all = list.length > 0 && list.every((k) => p.keys.includes(k))
  return all ? NO_PICKS : { keys: [...list], anchor: p.anchor }
}

/** The picked keys still in the list, in its order. */
export function picked(p: Picks, list: string[]): string[] {
  const set = new Set(p.keys)
  return list.filter((k) => set.has(k))
}
