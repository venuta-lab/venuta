// YouTube Music history in Stats: a play taken out goes from the list at once, by its key, and
// comes back by its key alone if YouTube Music turns it down, whatever else was taken out meanwhile.

interface DayLike<T> {
  title: string
  tracks: T[]
}

/** One play's key: its day and its place there, as the history came. */
export const playKey = (day: number, i: number) => `${day}:${i}`

/** The days as shown: each play with its key, the ones taken out left out, and a day left with
 *  none left out too. */
export function visibleDays<T>(days: DayLike<T>[], gone: ReadonlySet<string>) {
  return days.flatMap((d, di) => {
    const plays = d.tracks.flatMap((track, i) => {
      const key = playKey(di, i)
      return gone.has(key) ? [] : [{ key, track }]
    })
    return plays.length ? [{ title: d.title, plays }] : []
  })
}
