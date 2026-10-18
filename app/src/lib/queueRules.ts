/*
  What adding songs to the queue does, as a music app does it: a song already coming up is not
  added again; "Play next" on a song that is coming up moves it to play next instead. Songs
  already played can come round again. Pure (keys only), so tests/queueRules.test.ts reads it.
*/

export interface QueuePlan {
  /** The keys to add (in order), the ones coming up already left out. */
  add: string[]
  /** One song coming up, moved to play next (Play next on a single song). */
  move: { from: number; to: number } | null
  /** Songs left out because they are coming up already. */
  already: number
}

export function planEnqueue(queue: string[], index: number, keys: string[], nextUp: boolean): QueuePlan {
  const upcoming = new Map<string, number>()
  queue.forEach((k, i) => {
    if (i > index && !upcoming.has(k)) upcoming.set(k, i)
  })
  const seen = new Set<string>()
  const add: string[] = []
  let already = 0
  for (const k of keys) {
    if (seen.has(k)) continue
    seen.add(k)
    if (upcoming.has(k)) already++
    else add.push(k)
  }
  // One song asked to play next, coming up later: it moves up.
  const only = keys.length === 1 ? upcoming.get(keys[0]) : undefined
  const move = nextUp && only !== undefined && only > index + 1 ? { from: only, to: index + 1 } : null
  return { add, move, already: move ? 0 : already }
}
