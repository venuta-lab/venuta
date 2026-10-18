/*
  Shortcuts from anywhere (app hotkeys.rs): what each one does, the keys a key press makes
  (written the way Windows registers them: "Ctrl+Alt+KeyP"), and how they read ("Ctrl Alt P").
  Pure, so tests/hotkeys.test.ts reads it.
*/

export const HOTKEY_ACTIONS: { id: string; label: string }[] = [
  { id: 'play_pause', label: 'Play or pause' },
  { id: 'next', label: 'Next song' },
  { id: 'previous', label: 'Previous song' },
  { id: 'like', label: 'Like the song playing' },
  { id: 'volume_up', label: 'Volume up' },
  { id: 'volume_down', label: 'Volume down' },
  { id: 'show', label: 'Show or hide Venuta' },
  { id: 'desktop_lyrics', label: 'Desktop lyrics' },
]

interface KeyPress {
  code: string
  ctrlKey: boolean
  altKey: boolean
  shiftKey: boolean
  metaKey: boolean
}

const MODIFIER_CODES = /^(Control|Shift|Alt|Meta|OS)(Left|Right)?$/

/** The keys a press makes, or null while only modifiers are down (the press isn't done yet). */
export function keysOf(e: KeyPress): string | null {
  if (!e.code || MODIFIER_CODES.test(e.code)) return null
  const mods = [e.ctrlKey && 'Ctrl', e.altKey && 'Alt', e.shiftKey && 'Shift', e.metaKey && 'Super'].filter(Boolean)
  return [...mods, e.code].join('+')
}

/** How keys read in Settings: "Ctrl+Alt+KeyP" as "Ctrl Alt P". */
export function prettyKeys(keys: string): string {
  if (!keys) return 'Off'
  return keys
    .split('+')
    .map((k) =>
      k === 'Super'
        ? 'Win'
        : k
            .replace(/^Key([A-Z])$/, '$1')
            .replace(/^Digit(\d)$/, '$1')
            .replace(/^Numpad(\d)$/, 'Num $1')
            .replace(/^Arrow(Up|Down|Left|Right)$/, (_, d: string) => ({ Up: '↑', Down: '↓', Left: '←', Right: '→' })[d] ?? d)
            .replace(/^Media(PlayPause|TrackNext|TrackPrevious|Stop)$/, (_, m: string) => ({ PlayPause: 'Play/Pause', TrackNext: 'Next track', TrackPrevious: 'Previous track', Stop: 'Stop' })[m] ?? m),
    )
    .join(' ')
}
