// While an online song waits for its stream: what the player says, and whether a seek can go.
// Pure, so tests run it as is; the engine says when it waits (buffering, and stalled partway).

/** What the waiting reads from the player's state. */
export interface StreamWait {
  /** Waiting for the stream: still opening, or partway through. */
  buffering: boolean
  /** Partway through, all that arrived played: it plays on when the bytes come. */
  stalled?: boolean
}

/** Beside the song while it waits: "Opening…" before it starts, "Buffering…" partway. */
export function waitingWords(s: StreamWait): string | null {
  if (!s.buffering) return null
  return s.stalled ? 'Buffering…' : 'Opening…'
}

/** A song still opening has nothing to seek in yet; partway through, a seek goes. */
export function canSeekNow(s: StreamWait): boolean {
  return !s.buffering || !!s.stalled
}
