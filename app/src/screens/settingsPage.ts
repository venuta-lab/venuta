// Which Settings section opens next, kept apart from the Settings screen so asking for one
// doesn't load the screen before it is opened.

export type SettingsPage = 'look' | 'playback' | 'lyrics' | 'library' | 'online' | 'system' | 'activity' | 'about'

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

/** Each page's name, as the Settings list and Jump to (Ctrl+K) say it. */
export const SETTINGS_PAGE_NAMES: Record<SettingsPage, string> = {
  look: 'Look and feel',
  playback: 'Playback',
  lyrics: 'Lyrics',
  library: 'Library',
  online: 'Online',
  system: 'System',
  activity: 'Activity',
  about: 'About and updates',
}

/**
 * What Jump to (Ctrl+K) finds in Settings: each page, and the settings people look for by name,
 * with the words they might type. Choosing one opens its page.
 */
export const SETTINGS_FOUND: [SettingsPage, string, string][] = [
  ['look', 'Look and feel', 'theme themes colour color accent dark light mode window surface size zoom motion app icon'],
  ['playback', 'Playback', 'sound audio output play through'],
  ['playback', 'Output device', 'speakers headphones bluetooth usb dac hdmi play through follow windows delay'],
  ['playback', 'Exclusive (bit-perfect)', 'exclusive bit perfect wasapi lossless hi-res'],
  ['playback', 'Pause when headphones or Bluetooth disconnect', 'unplug disconnect pause headphones bluetooth'],
  ['playback', 'Volume levelling', 'replaygain loudness normalise normalize'],
  ['playback', 'Crossfade', 'fade gapless transition'],
  ['playback', 'Autoplay', 'up next radio what plays'],
  ['playback', 'Stream quality', 'online quality bitrate'],
  ['lyrics', 'Lyrics', 'lyrics'],
  ['lyrics', 'Pronunciation under lyrics', 'english letters roman romanisation romanization transliteration hinglish'],
  ['lyrics', 'Translation of lyrics', 'translate translation language deepl microsoft google claude key'],
  ['lyrics', 'Lyrics sources', 'lrclib netease musixmatch sources order'],
  ['lyrics', 'Big screen and desktop lyrics', 'big screen desktop lyrics window tv'],
  ['library', 'Library', 'folders music folder where details come from musicbrainz backup rules'],
  ['online', 'Online', 'online catalog accounts'],
  ['online', 'Local only', 'offline local only no internet'],
  ['online', 'YouTube Music account', 'youtube music sign in account google'],
  ['online', 'Listen together', 'room rooms together name relay ip'],
  ['online', 'Region', 'country region catalog'],
  ['system', 'System', 'windows'],
  ['system', 'When you close the window', 'close tray quit minimise'],
  ['system', 'Start with Windows', 'startup autostart boot login'],
  ['system', 'Shortcuts from anywhere', 'global hotkeys keys media keys'],
  ['system', 'History and cache', 'history cache clear storage space'],
  ['activity', 'Activity', 'changes undo log'],
  ['about', 'About and updates', 'about version'],
  ['about', 'Check for updates', 'update updates upgrade new version install'],
  ['about', 'Pre-releases', 'beta pre-release early'],
  ['about', 'Diagnostics', 'diagnostics report bug log help'],
]
