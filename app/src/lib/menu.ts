import { useSyncExternalStore } from 'react'

/*
  The window's one menu, as state: what is open, where, and where focus goes back to. Rows,
  cards and "…" buttons open it (menuFromEvent); components/ContextMenu.tsx draws it.
*/

export interface MenuItem {
  label: string
  run: () => void
  /** Drawn as a warning (Delete…). */
  danger?: boolean
  disabled?: boolean
  /** Asked for when the menu shows this item (an item that reads what it offers): it calls
   *  rebuildMenu() when there is more to show. */
  load?: () => void
}
/** Items in groups; a line is drawn between groups. */
export type MenuGroups = MenuItem[][]

export type Place = { x: number; y: number } | { anchor: HTMLElement }
export type Open = { groups: MenuGroups; place: Place; back: HTMLElement | null; id: number; build?: () => MenuGroups }

let open: Open | null = null
let serial = 0
const listeners = new Set<() => void>()
const changed = () => listeners.forEach((l) => l())

const tidy = (groups: MenuGroups) => groups.map((g) => g.filter(Boolean)).filter((g) => g.length)

/** Open the menu at the pointer, or under `anchor` (a "…" button). Focus returns to `back`.
 *  Given as a builder, it can be built again while open (rebuildMenu). */
export function openMenu(groups: MenuGroups | (() => MenuGroups), place: Place, back?: HTMLElement | null) {
  const build = typeof groups === 'function' ? groups : undefined
  const shown = tidy(build ? build() : (groups as MenuGroups))
  if (!shown.length) return
  open = { groups: shown, place, back: back ?? (document.activeElement as HTMLElement | null), id: ++serial, build }
  changed()
  for (const item of shown.flat()) item.load?.()
}

/** The open menu, built again (an item read what it offers). Nothing when it was closed. */
export function rebuildMenu() {
  if (!open?.build) return
  const shown = tidy(open.build())
  if (!shown.length) return closeMenu(false)
  open = { ...open, groups: shown }
  changed()
}

export function closeMenu(refocus = true) {
  if (!open) return
  const back = open.back
  open = null
  changed()
  if (refocus && back?.isConnected) back.focus({ preventScroll: true })
}

/**
 * Open `groups` for a right-click or a "…" press: at the pointer for a right-click, under the
 * element for a click or a key (Shift+F10, the menu key). The innermost handler wins.
 */
export function menuFromEvent(e: React.MouseEvent, groups: MenuGroups | (() => MenuGroups)) {
  if (e.defaultPrevented) return
  e.preventDefault()
  e.stopPropagation()
  const el = e.currentTarget as HTMLElement
  const fromKey = e.type === 'click' || (e.clientX === 0 && e.clientY === 0)
  openMenu(groups, fromKey ? { anchor: el } : { x: e.clientX, y: e.clientY }, fromKey ? el : (document.activeElement as HTMLElement))
}

/** The menu open now, or null; re-renders when that changes. */
export const useMenuOpen = () =>
  useSyncExternalStore(
    (l) => {
      listeners.add(l)
      return () => {
        listeners.delete(l)
      }
    },
    () => open,
  )
