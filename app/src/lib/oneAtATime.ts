// One press at a time: while what a press asked for is still being done, pressing again is the
// same press (its promise), not another. Pure, so tests run it as is.

export function oneAtATime<K>() {
  const going = new Map<K, Promise<unknown>>()
  return <T>(key: K, run: () => Promise<T>): Promise<T> => {
    const now = going.get(key)
    if (now) return now as Promise<T>
    const p = run().finally(() => going.delete(key))
    going.set(key, p)
    return p
  }
}
