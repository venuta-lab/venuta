// "Add to playlist" on YouTube Music: only a playlist you can change takes songs, and a song that
// is there already is left out (YouTube refuses the whole add for one that is).

interface ThereLike {
  title: string
  /** The account made it. */
  owned: boolean
  collaborative: boolean
  tracks: { track_id: string }[]
}

/** A playlist you made, or one you collaborate on. */
export const canAddTo = (p: Pick<ThereLike, 'owned' | 'collaborative'>) => p.owned || p.collaborative

/** What to send to `there`, each song once and none already in it, and what to say after.
 *  `label` names a single song (“Tum Hi Ho”). */
export function addPlan(videoIds: string[], there: ThereLike, label?: string): { send: string[]; words: string } {
  const where = `“${there.title}” on YouTube Music`
  if (!canAddTo(there)) {
    return { send: [], words: `${where} is someone else’s playlist, so Venuta can’t add to it.` }
  }
  const have = new Set(there.tracks.map((t) => t.track_id))
  const wanted = [...new Set(videoIds)]
  const send = wanted.filter((v) => !have.has(v))
  const already = wanted.length - send.length
  if (!send.length) {
    return { send, words: wanted.length === 1 ? `It’s in ${where} already.` : `${wanted.length === 2 ? 'Both are' : `All ${wanted.length} are`} in ${where} already.` }
  }
  const what = send.length === 1 ? (wanted.length === 1 && label ? label : '1 song') : `${send.length} songs`
  const was = already ? ` ${already} ${already === 1 ? 'was' : 'were'} there already.` : ''
  return { send, words: `Added ${what} to ${where}.${was}` }
}

/** Whether a whole list (`source`: the playlist's or the album's own list) can go in as one copy
 *  on YouTube Music's side, rather than song by song: only when every song of it is an online one,
 *  each once, and none is in `there` already (a copy can't leave any out). */
export function copyWhole(source: string | null | undefined, videoIds: string[], plan: { send: string[] }, filesOfYours: number): boolean {
  if (!source?.trim() || filesOfYours > 0 || !videoIds.length) return false
  const once = new Set(videoIds)
  return once.size === videoIds.length && plan.send.length === videoIds.length
}
