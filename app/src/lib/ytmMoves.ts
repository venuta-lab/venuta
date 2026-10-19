// A YouTube Music playlist reordered in Venuta: the fewest "move this item before that one" edits
// that turn the order it had into the new one. The songs that keep their order among themselves
// (the longest run already in order) stay put; each other song moves, last first, before the song
// that follows it in the new order, which by then is in its place.

export interface Move {
  set_video_id: string
  /** The item it goes just before; null: to the end. */
  before: string | null
}

/** The items of `next` that keep their order from `prev`: a longest increasing run. */
function staying(prev: string[], next: string[]): Set<string> {
  const at = new Map(prev.map((id, i) => [id, i]))
  const seq = next.map((id) => at.get(id) ?? -1)
  // Patience sort for the longest increasing subsequence, with the way back.
  const tails: number[] = []
  const tailAt: number[] = []
  const back: number[] = new Array(seq.length).fill(-1)
  seq.forEach((v, i) => {
    if (v < 0) return
    let lo = 0
    let hi = tails.length
    while (lo < hi) {
      const mid = (lo + hi) >> 1
      if (tails[mid] < v) lo = mid + 1
      else hi = mid
    }
    tails[lo] = v
    tailAt[lo] = i
    back[i] = lo > 0 ? tailAt[lo - 1] : -1
  })
  const keep = new Set<string>()
  let i = tails.length ? tailAt[tails.length - 1] : -1
  while (i >= 0) {
    keep.add(next[i])
    i = back[i]
  }
  return keep
}

/** The moves, in the order to send them, that make `prev` into `next` (the same items). */
export function movesBetween(prev: string[], next: string[]): Move[] {
  const keep = staying(prev, next)
  const moves: Move[] = []
  for (let i = next.length - 1; i >= 0; i--) {
    if (keep.has(next[i])) continue
    moves.push({ set_video_id: next[i], before: next[i + 1] ?? null })
  }
  return moves
}

/** The order after these moves (what YouTube does with them; for checking). */
export function applyMoves(order: string[], moves: Move[]): string[] {
  const out = order.slice()
  for (const m of moves) {
    const from = out.indexOf(m.set_video_id)
    if (from < 0) continue
    out.splice(from, 1)
    const to = m.before === null ? out.length : out.indexOf(m.before)
    out.splice(to < 0 ? out.length : to, 0, m.set_video_id)
  }
  return out
}
