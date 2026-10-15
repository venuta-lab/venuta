// Searches you meant (Enter, or playing / opening something you found), newest first, kept
// only on this computer (localStorage) and never sent anywhere. Typing alone records nothing.
//
// Plain TypeScript (no React): node --test imports it as it is, with its own store.

export interface RecentSearch {
  /** The words, as typed (spaces tidied). */
  q: string
  /** When last searched (ms since 1970). */
  at: number
}

/** Where they're kept, and how many. */
export const RECENT_KEY = 'search-recent'
export const RECENT_KEEP = 20

/** The little a store needs (localStorage, or a stand-in in tests). */
export interface KeyValueStore {
  getItem(key: string): string | null
  setItem(key: string, value: string): void
  removeItem(key: string): void
}

const tidy = (q: string) => q.replace(/\s+/g, ' ').trim()
/** Two searches are the same when they differ only in case and spacing. */
const same = (a: string, b: string) => tidy(a).toLowerCase() === tidy(b).toLowerCase()

/** Recent searches over a store: list, remember, forget, clear, and hear changes. */
export function recentSearchStore(store: () => KeyValueStore | null, now: () => number = Date.now) {
  const listeners = new Set<() => void>()
  let cache: RecentSearch[] | null = null

  const read = (): RecentSearch[] => {
    if (cache) return cache
    try {
      const v = JSON.parse(store()?.getItem(RECENT_KEY) ?? '[]') as unknown
      cache = Array.isArray(v)
        ? v
            .filter((x): x is RecentSearch => !!x && typeof (x as RecentSearch).q === 'string')
            .map((x) => ({ q: tidy(x.q), at: Number(x.at) || 0 }))
            .filter((x) => x.q)
            .slice(0, RECENT_KEEP)
        : []
    } catch {
      cache = []
    }
    return cache
  }
  const write = (list: RecentSearch[]) => {
    cache = list
    try {
      const s = store()
      if (list.length) s?.setItem(RECENT_KEY, JSON.stringify(list))
      else s?.removeItem(RECENT_KEY)
    } catch {
      // Not kept this time (private storage): still shown until the window closes.
    }
    listeners.forEach((l) => l())
  }

  return {
    /** Newest first. The same array until something changes (safe for React stores). */
    list: read,
    /** A search was meant: to the front (once), oldest dropped past the limit. One letter is no search. */
    push(q: string) {
      const t = tidy(q)
      if (t.length < 2) return
      write([{ q: t, at: now() }, ...read().filter((r) => !same(r.q, t))].slice(0, RECENT_KEEP))
    },
    remove(q: string) {
      const list = read()
      if (list.some((r) => same(r.q, q))) write(list.filter((r) => !same(r.q, q)))
    },
    clear() {
      if (read().length) write([])
    },
    subscribe(l: () => void) {
      listeners.add(l)
      return () => {
        listeners.delete(l)
      }
    },
  }
}

/** This computer's recent searches (the window's localStorage). */
export const recentSearches = recentSearchStore(() => (typeof localStorage === 'undefined' ? null : localStorage))
