// Votes on a collaborative YouTube Music playlist. Each button is two taps YouTube minted with the
// row: its vote, and (pressed again) taking it back. Which one a press sends, and the count to
// show after, come from what the row came with, so a press needs no reading again.

export type Vote = 'up' | 'down' | 'neutral'

interface VotesLike {
  /** How you voted when the list came. */
  mine: Vote
  /** The net count when the list came. */
  votes: number
  /** YouTube's words for the count with no vote of yours, with yours up, with yours down. */
  count?: string | null
  count_up?: string | null
  count_down?: string | null
  up: { token: string; undo_token?: string | null }
  down: { token: string; undo_token?: string | null }
}

/** What pressing `which` sends while your vote is `mine`, and the vote it leaves. Null: there is
 *  nothing to send (the vote can't be taken back from here). */
export function votePress(v: VotesLike, mine: Vote, which: 'up' | 'down'): { token: string; next: Vote } | null {
  const button = which === 'up' ? v.up : v.down
  if (mine === which) return button.undo_token ? { token: button.undo_token, next: 'neutral' } : null
  return { token: button.token, next: which }
}

const weight = (v: Vote) => (v === 'up' ? 1 : v === 'down' ? -1 : 0)

/** The count to show with your vote at `mine`. */
export function voteCount(v: VotesLike, mine: Vote): string {
  const said = mine === 'up' ? v.count_up : mine === 'down' ? v.count_down : v.count
  if (said != null) return said
  return String(v.votes - weight(v.mine) + weight(mine))
}
