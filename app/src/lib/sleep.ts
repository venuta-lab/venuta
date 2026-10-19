// The sleep timer at a glance: what the player bar's moon says, and the choices it offers. Pure,
// so tests run it as is; the player keeps the timer (player_sleep, player_stop_after).

/** The minutes it can be set to, the same everywhere (the bar, the tray, Sound). */
export const SLEEP_MINUTES = [15, 30, 45, 60, 90] as const

/** What the timer reads from the player's state. */
export interface SleepState {
  sleep_in_s: number | null
  stop_after_current: boolean
}

/** Which choice is on: none, the end of this song, or a timer. */
export function sleepChoice(s: SleepState): 'off' | 'song' | 'timer' {
  if (s.stop_after_current) return 'song'
  return s.sleep_in_s !== null ? 'timer' : 'off'
}

/** Beside the moon while it's on: "23m", "0:45" in the last minute, or "End of song". */
export function sleepBadge(s: SleepState): string | null {
  if (s.stop_after_current) return 'End of song'
  if (s.sleep_in_s === null) return null
  const left = Math.max(0, Math.round(s.sleep_in_s))
  if (left < 60) return `0:${String(left).padStart(2, '0')}`
  return `${Math.ceil(left / 60)}m`
}
