// Which Settings section opens next, kept apart from the Settings screen so asking for one
// doesn't load the screen before it is opened.

export type SettingsPage = 'look' | 'playback' | 'lyrics' | 'library' | 'online' | 'system'

let asked: SettingsPage | null = null

/** Open Settings on this section next time it shows. */
export function settingsSection(p: SettingsPage) {
  asked = p
}

/** The section asked for, once. */
export function takeSettingsSection(): SettingsPage | null {
  const p = asked
  asked = null
  return p
}
