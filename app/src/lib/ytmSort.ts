// Sorting your YouTube Music library lists as its own app does: as they were added (the order
// YouTube Music gives), or by name either way.
//
// Pure (no app imports), so tests/ytmSort.test.ts reads it as it is.

export type YtmSort = 'added' | 'az' | 'za'

export const SORTS: [YtmSort, string][] = [
  ['added', 'Recently added'],
  ['az', 'A to Z'],
  ['za', 'Z to A'],
]

const byName = new Intl.Collator(undefined, { sensitivity: 'base', numeric: true })

/** A sorted copy of `items` (the list given is left as it is). */
export function sortedBy<T>(items: readonly T[], sort: YtmSort, nameOf: (t: T) => string): T[] {
  if (sort === 'added') return [...items]
  const out = [...items].sort((a, b) => byName.compare(nameOf(a), nameOf(b)))
  return sort === 'za' ? out.reverse() : out
}
