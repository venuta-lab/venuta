// Choose artists you like: YouTube Music's taste builder. Every artist it showed is told as seen,
// the chosen ones as chosen, and only when Done is pressed.
//
// Pure (no app imports), so tests/ytmTaste.test.ts reads it as it is.

interface ArtistLike {
  selection_value: string
  impression_value: string
  selected: boolean
}

/** The ones YouTube Music says are chosen already, by their selection value. */
export const firstChosen = (artists: ArtistLike[]): Set<string> => new Set(artists.filter((a) => a.selected).map((a) => a.selection_value))

/** Chosen, or taken back. */
export function toggleChoice(chosen: ReadonlySet<string>, value: string): Set<string> {
  const next = new Set(chosen)
  if (next.has(value)) next.delete(value)
  else next.add(value)
  return next
}

/** What YouTube Music is told: every artist shown, and the chosen ones, in the order shown. */
export function tasteAnswer(artists: ArtistLike[], chosen: ReadonlySet<string>): { impressions: string[]; selected: string[] } {
  return {
    impressions: artists.map((a) => a.impression_value),
    selected: artists.filter((a) => chosen.has(a.selection_value)).map((a) => a.selection_value),
  }
}
