/*
  The themes you've used lately, newest first, kept on this computer: Settings shows them as a
  row to switch back in one click, however many themes there are.
*/

const KEY = 'theme-recent'
const KEEP = 10

/** A theme as one key: a built-in one ("style:neo-tokyo") or one of yours ("pack:dusk"). */
export const themeKey = (style: string, pack: string) => (pack ? `pack:${pack}` : `style:${style}`)

export function recentThemes(): string[] {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? '[]') as unknown
    return Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : []
  } catch {
    return []
  }
}

/** The theme was used: to the front of the row. */
export function noteTheme(key: string) {
  try {
    localStorage.setItem(KEY, JSON.stringify([key, ...recentThemes().filter((k) => k !== key)].slice(0, KEEP)))
  } catch {
    // Not remembered this time.
  }
}
