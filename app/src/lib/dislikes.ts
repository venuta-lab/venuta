/*
  A thumbs down on an online song, as YouTube Music's radio does it: the song leaves what is
  coming up, and Undo puts back the rating it had before (a like stays a like). The rules only;
  lib/radio.ts `dislike` runs them. Pure, so tests/dislikes.test.ts reads it.
*/

/** The places after the one playing that hold this song, last first: taken out in this order,
 *  each place stays right. Songs already played stay where they are. */
export function upcomingCopies(keys: string[], index: number, key: string): number[] {
  const out: number[] = []
  for (let i = keys.length - 1; i > index; i--) if (keys[i] === key) out.push(i)
  return out
}

/** How a song stood before its thumbs down. */
export interface BeforeDislike {
  liked: boolean
  disliked: boolean
}

/** What Undo of a dislike puts back: take the dislike off (unless it was disliked already),
 *  like it again (when the dislike replaced a like), and the rating that goes to the account
 *  with Likes and dislikes on (null: none, nothing changed there). */
export function undoOfDislike(before: BeforeDislike): { undislike: boolean; relike: boolean; account: 'like' | 'none' | null } {
  if (before.disliked) return { undislike: false, relike: false, account: null }
  if (before.liked) return { undislike: true, relike: true, account: 'like' }
  return { undislike: true, relike: false, account: 'none' }
}
