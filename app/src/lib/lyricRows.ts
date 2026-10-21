// The desktop strip's rows, and how the strip grows to fit them. Its words size to its height
// (DesktopLyrics.tsx): the size is a share of the height over the rows. The rows are kept for
// the song (lib/secondLines.ts), so the words never change size line by line; when a row is
// switched on (or a song comes that has one), the strip grows by as much as keeps the words
// their size, and shrinks back when it goes. Pure, so tests run it as is.

import type { SecondSwitches } from './secondLines.ts'

/** Each row's height as a share of the line's font size (LyricText: line height 1.18; the next
 *  line 0.7 of the size with a little gap; pronunciation 0.52 with a 0.22 gap; translation 0.46
 *  with a 0.16 gap). */
export const ROW = { line: 1.18, next: 0.83 + 0.06, roman: 0.52 * 1.18 + 0.22, translation: 0.46 * 1.18 + 0.16 }

/** The rows the strip's words are a share of: the line, the next under it, and the second rows. */
export function stripRows(lines: 1 | 2, rows: SecondSwitches): number {
  return ROW.line + (lines === 2 ? ROW.next : 0) + (rows.roman ? ROW.roman : 0) + (rows.translation ? ROW.translation : 0)
}

/** The strip's place (physical pixels), and the screen it is on (its work area). */
export interface Place {
  x: number
  y: number
  w: number
  h: number
}

/** The words' padding (p-3, top and bottom), not room for words. */
const PAD = 24

/**
 * Where the strip goes when its rows change from `from` to `to`: taller (or shorter) by as much
 * as keeps the words their size, its bottom edge kept when it sits in the lower half of the
 * screen (it grows upward), its top edge otherwise, and never past the screen.
 */
export function grownStrip(p: Place, band: number, from: number, to: number, screen: Place): Place {
  if (from <= 0 || to <= 0 || Math.abs(from - to) < 1e-6) return p
  const room = Math.max(1, p.h - band - PAD)
  const h = Math.round(Math.min(screen.h, Math.max(band + PAD + 1, band + PAD + (room * to) / from)))
  const lower = p.y + p.h / 2 > screen.y + screen.h / 2
  let y = lower ? p.y + p.h - h : p.y
  y = Math.max(screen.y, Math.min(y, screen.y + screen.h - h))
  return { x: p.x, y, w: p.w, h }
}
