// How much smaller a lyrics window's lines must be to fit it: pure, so tests run it as is.
// The lines wrap where they can; a run of letters with nowhere to wrap (a long word, words joined
// by no-break spaces, a wide theme font) is as wide as it is, so it is made smaller instead, and
// the whole of what shows must fit the room it has.

/** The smallest share of the chosen size a line is made: past it, it breaks inside its words. */
export const MIN_FIT = 0.35

export interface Measured {
  /** Each row of words: how wide its words are (scrollWidth), and how wide it is (clientWidth). */
  rows: { scroll: number; client: number }[]
  /** How tall the lines are together. */
  height: number
  /** How tall they may be. */
  room: number
}

/** The share of their size the lines need to be to fit (1: they fit as they are). */
export function shrinkFor(m: Measured): number {
  let need = 1
  // A pixel over is rounding, not overflow.
  for (const r of m.rows) if (r.client > 0 && r.scroll > r.client + 1) need = Math.min(need, r.client / r.scroll)
  if (m.room > 0 && m.height > m.room + 1) need = Math.min(need, m.room / m.height)
  return need
}

/** The next size to try (a share of the chosen size): a little under what is needed, so it lands
 *  inside, and never under the smallest. */
export function nextFit(fit: number, need: number): number {
  if (need >= 1) return fit
  return Math.max(MIN_FIT, fit * need * 0.98)
}

/** The big screen's room for its lines (px): `top` and `bottom`, where the lines' area starts and
 *  how far from the bottom it ends, and `room`, how tall the lines may be. The lines are centred in
 *  that area, so lines no taller than `room` never reach past it. With the song bar (its top
 *  `barTop` px down the screen, as measured: it grows with the lines under the song), the area
 *  ends a gap above the bar and keeps a margin under the window's buttons at the top; without it
 *  (Clear, nothing playing), the whole screen with a margin top and bottom. */
export function stageRoom(height: number, barTop: number | null): { top: number; bottom: number; room: number } {
  if (barTop === null) {
    const margin = height * 0.05
    return { top: margin, bottom: margin, room: height - 2 * margin }
  }
  const top = height * 0.08
  const bottom = height - barTop + height * 0.03
  return { top, bottom, room: Math.max(0, height - top - bottom) }
}
