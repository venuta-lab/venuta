// The places of the app and the rail that reaches them.
//
// The rail is four groups: what you listen to (Home, Search, Library), your library at a glance
// (Stats, Health), managing it (Organize, Devices), and Settings at the bottom. Activity is
// reached from the title bar and Ctrl+H; Metadata became Health's Fix details.

export type Place = 'home' | 'search' | 'library' | 'stats' | 'health' | 'organize' | 'devices' | 'activity' | 'settings'

/** The music screen's own views: the three music places, and Listening shown as Stats. */
export type MusicTop = 'home' | 'search' | 'library' | 'listening'

export const RAIL_GROUPS: Place[][] = [
  ['home', 'search', 'library'],
  ['stats', 'health'],
  ['organize', 'devices'],
]
export const RAIL_BOTTOM: Place[] = ['settings']

/** The numbered places, Ctrl+1 to Ctrl+7, in the rail's order. */
const NUMBERED: Place[] = RAIL_GROUPS.flat()

/** Every place in the order pages slide between them. */
export const PLACE_ORDER: Place[] = [...NUMBERED, 'activity', 'settings']

export const PLACE_LABELS: Record<Place, string> = {
  home: 'Home',
  search: 'Search',
  library: 'Library',
  stats: 'Stats',
  health: 'Health',
  organize: 'Organize',
  devices: 'Devices',
  activity: 'Activity',
  settings: 'Settings',
}

export const PLACE_KEYS: Record<Place, string> = {
  ...(Object.fromEntries(NUMBERED.map((p, i) => [p, `Ctrl+${i + 1}`])) as Record<Place, string>),
  activity: 'Ctrl+H',
  settings: 'Ctrl+,',
}

/** The place Ctrl and this digit goes to, if any. */
export function placeForKey(n: number): Place | undefined {
  return n >= 1 ? NUMBERED[n - 1] : undefined
}

export function isMusicPlace(p: Place): p is 'home' | 'search' | 'library' | 'stats' {
  return p === 'home' || p === 'search' || p === 'library' || p === 'stats'
}

export function musicTopOf(p: 'home' | 'search' | 'library' | 'stats'): MusicTop {
  return p === 'stats' ? 'listening' : p
}

export function placeOfMusicTop(t: MusicTop): Place {
  return t === 'listening' ? 'stats' : t
}

/** The four music places are one screen: moving between them keeps what it has open. */
export function screenKey(p: Place): string {
  return isMusicPlace(p) ? 'music' : p
}

/** The next place along the rail for a sideways swipe, or none at its ends. */
export function swipeFrom(p: Place, dir: 1 | -1): Place | undefined {
  const i = NUMBERED.indexOf(p)
  return i < 0 ? undefined : NUMBERED[i + dir]
}
