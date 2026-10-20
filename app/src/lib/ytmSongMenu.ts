// An online song's own YouTube Music menu: Save to library, Pin to Listen again, Not interested.
// Each is an opaque token YouTube minted with the row where the song was listed, one per way, so
// a press needs no reading again: it sends the way the song does not stand now, and the song then
// stands the other way wherever it shows (a list, a card, the queue) until it is listed afresh.
//
// Pure (no app imports), so tests/ytmSongMenu.test.ts reads it as it is.

/** A two-way press: how it stands, and the token for each way (absent: that way can't be sent). */
export interface MenuToggle {
  on: boolean
  on_token?: string | null
  off_token?: string | null
}
/** A one-way press ("Not interested"), in YouTube Music's own words. */
export interface MenuFeedback {
  icon: string
  label: string
  token: string
}
/** What a song's own menu offers the account (crates/media-access `TrackMenu`). */
export interface TrackMenu {
  library?: MenuToggle | null
  pin?: MenuToggle | null
  feedback?: MenuFeedback[]
  /** When it was listed (ms since 1970, the app's own clock). */
  listed_at?: number
}

export type Press = 'library' | 'pin' | 'feedback'

/** One thing the menu offers: its words, the token it sends, and how the song stands after. */
export interface SongMenuItem {
  press: Press
  label: string
  token: string
  next: boolean
}

/** What pressing a toggle sends, and how it leaves the song. Null: that way wasn't listed. */
export function togglePress(t: MenuToggle): { token: string; next: boolean } | null {
  const token = t.on ? t.off_token : t.on_token
  return token ? { token, next: !t.on } : null
}

const WORDS: Record<'library' | 'pin', [string, string]> = {
  library: ['Save to YouTube Music library', 'Remove from YouTube Music library'],
  pin: ['Pin to Listen again', 'Unpin from Listen again'],
}

/** The presses a song's menu offers now, in the order YouTube Music's own menu has them. */
export function songMenuItems(menu: TrackMenu | null | undefined): SongMenuItem[] {
  if (!menu) return []
  const items: SongMenuItem[] = []
  for (const press of ['library', 'pin'] as const) {
    const t = menu[press]
    const sent = t ? togglePress(t) : null
    if (t && sent) items.push({ press, label: WORDS[press][t.on ? 1 : 0], token: sent.token, next: sent.next })
  }
  for (const f of menu.feedback ?? []) if (f.token && f.label.trim()) items.push({ press: 'feedback', label: f.label.trim(), token: f.token, next: true })
  return items
}

const same = (a: TrackMenu, b: TrackMenu) => JSON.stringify(a) === JSON.stringify(b)

/**
 * The menus of the songs listed this session, by song key, with the presses made on them. The
 * newest listing is kept (an older page shown again never takes its place; YouTube Music mints
 * new tokens with every listing, and either does the same). A press stands over a listing made
 * before it; a listing made after a press is the truth, so a change on the phone shows here once
 * the song is listed again. The oldest are let go past `limit`.
 */
export class SongMenus {
  private menus = new Map<string, TrackMenu>()
  private presses = new Map<string, { library?: { on: boolean; at: number }; pin?: { on: boolean; at: number }; sent?: Set<string> }>()
  private readonly limit: number

  constructor(limit = 3000) {
    this.limit = limit
  }

  /** A song as it was listed, with its menu. */
  remember(key: string, menu: TrackMenu | null | undefined) {
    if (!menu) return
    const kept = this.menus.get(key)
    if (kept && (same(kept, menu) || (kept.listed_at ?? 0) > (menu.listed_at ?? 0))) return
    this.menus.delete(key)
    this.menus.set(key, menu)
    while (this.menus.size > this.limit) {
      const oldest = this.menus.keys().next().value as string
      this.menus.delete(oldest)
      this.presses.delete(oldest)
    }
  }

  /** The song's menu as it stands now: as last listed (`came`, when given, is that listing),
   *  with the presses made on it. Null: never listed with one. */
  menuOf(key: string, came?: TrackMenu | null): TrackMenu | null {
    if (came) this.remember(key, came)
    const menu = this.menus.get(key)
    if (!menu) return null
    const p = this.presses.get(key)
    if (!p) return menu
    const listed = menu.listed_at ?? 0
    const stands = <T extends MenuToggle>(t: T | null | undefined, press?: { on: boolean; at: number }) =>
      t && press && press.at > listed ? { ...t, on: press.on } : t
    return {
      ...menu,
      library: stands(menu.library, p.library),
      pin: stands(menu.pin, p.pin),
      feedback: (menu.feedback ?? []).filter((f) => !p.sent?.has(f.token)),
    }
  }

  /** A toggle was pressed and YouTube Music took it (`at`): the song stands `on` now. */
  pressed(key: string, which: 'library' | 'pin', on: boolean, at = Date.now()) {
    this.presses.set(key, { ...this.presses.get(key), [which]: { on, at } })
  }

  /** A one-way press was sent: it isn't offered again for this listing. */
  sent(key: string, token: string) {
    const p = this.presses.get(key) ?? {}
    this.presses.set(key, { ...p, sent: new Set([...(p.sent ?? []), token]) })
  }
}

/** The app's one store of them. */
export const songMenus = new SongMenus()
