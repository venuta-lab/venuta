import { getSettings, inApp, setSettings, type Listening } from './engine'

/*
  Your listening choices (autoplay, blocked artists, disliked songs, history…), kept with your
  settings (crates/engine settings.rs `Listening`). Read once, then kept here; a change is saved
  whole and the online catalog is told at once (who it never offers, whether videos show).
*/

const DEFAULTS: Listening = {
  autoplay: true,
  open_player_on_play: false,
  blocked_artists: [],
  disliked: [],
  hide_videos: false,
  history: true,
  stream_quality: 'auto',
  online_levelling: false,
}

let now: Listening = DEFAULTS
let read: Promise<Listening> | null = null
let version = 0
const listeners = new Set<() => void>()

/** Your listening choices as they are now (the defaults until they are read). */
export const listening = () => now

/** Read them (once; again with `fresh`). */
export function loadListening(fresh = false): Promise<Listening> {
  if (!inApp()) return Promise.resolve(now)
  if (!read || fresh)
    read = getSettings()
      .then((s) => {
        now = { ...DEFAULTS, ...(s.listening ?? {}) }
        version++
        listeners.forEach((l) => l())
        return now
      })
      .catch(() => now)
  return read
}

/** Change some of them; saved at once. */
export async function updateListening(change: Partial<Listening>) {
  await loadListening()
  now = { ...now, ...change }
  version++
  listeners.forEach((l) => l())
  if (inApp()) await setSettings({ listening: now })
}

export function subscribeListening(l: () => void) {
  listeners.add(l)
  return () => {
    listeners.delete(l)
  }
}
export const listeningVersion = () => version
