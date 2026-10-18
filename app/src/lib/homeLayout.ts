/*
  Home's sections in your order, each on or off (Edit Home). Kept in this window's storage: how
  you like Home to look on this computer. Pure functions, so tests/homeLayout.test.ts reads it.
*/

export interface HomeLayout {
  /** Section ids in your order (sections not named keep their own place). */
  order: string[]
  /** Sections switched off. */
  hidden: string[]
}

/** The known sections in your order, each on or off. A section added since you arranged Home
 *  goes after the section it follows in its own order. */
export function arrange(l: HomeLayout, known: string[]): { id: string; on: boolean }[] {
  const out = l.order.filter((id) => known.includes(id))
  known.forEach((id, i) => {
    if (out.includes(id)) return
    const before = known.slice(0, i).reverse().find((b) => out.includes(b))
    out.splice(before ? out.indexOf(before) + 1 : 0, 0, id)
  })
  return out.map((id) => ({ id, on: !l.hidden.includes(id) }))
}

/** Move a section up (-1) or down (1). */
export function move(l: HomeLayout, known: string[], id: string, by: -1 | 1): HomeLayout {
  const order = arrange(l, known).map((s) => s.id)
  const i = order.indexOf(id)
  const j = i + by
  if (i < 0 || j < 0 || j >= order.length) return { ...l, order }
  ;[order[i], order[j]] = [order[j], order[i]]
  return { ...l, order }
}

/** Switch a section on or off. */
export function toggle(l: HomeLayout, id: string): HomeLayout {
  return { ...l, hidden: l.hidden.includes(id) ? l.hidden.filter((h) => h !== id) : [...l.hidden, id] }
}

const KEY = 'home-layout'

export function readLayout(): HomeLayout {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? '') as HomeLayout
    if (Array.isArray(v.order) && Array.isArray(v.hidden)) return v
  } catch {
    // Nothing kept (or a private window): the usual Home.
  }
  return { order: [], hidden: [] }
}

export function saveLayout(l: HomeLayout) {
  try {
    localStorage.setItem(KEY, JSON.stringify(l))
  } catch {
    // Kept for this visit only.
  }
}
