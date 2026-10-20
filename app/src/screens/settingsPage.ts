// Which Settings section opens next, kept apart from the Settings screen so asking for one
// doesn't load the screen before it is opened.

export type SettingsPage = 'look' | 'playback' | 'lyrics' | 'library' | 'online' | 'system' | 'activity'

let asked: SettingsPage | null = null

const listeners = new Set<() => void>()

/** Open Settings on this section next time it shows, or now when it is showing. */
export function settingsSection(p: SettingsPage) {
  asked = p
  listeners.forEach((l) => l())
}

/** Settings, while open: told when a section is asked for (Ctrl+H on Settings, say). */
export function onSettingsSection(l: () => void): () => void {
  listeners.add(l)
  return () => {
    listeners.delete(l)
  }
}

/** The section asked for, without using it up (a screen may render more than once before it shows). */
export function peekSettingsSection(): SettingsPage | null {
  return asked
}

/** The section asked for, once. */
export function takeSettingsSection(): SettingsPage | null {
  const p = asked
  asked = null
  return p
}
