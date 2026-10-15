import { useEffect, useId, useRef, useSyncExternalStore } from 'react'
import type { QueueItem } from './player'

/*
  Dragging music to where it should go: songs, an album or a playlist onto a playlist, the
  Playlists tab, an open playlist, the queue or the player bar. Built on pointer events (HTML5
  drag is unreliable in WebView2 while dropping files on the window is on).

  A press becomes a drag only after the pointer has moved a few pixels, so a click is still a
  click (and the click that ends a drag is swallowed). What is carried follows the pointer
  (components/DragLayer); the target under it lights up and says what will happen; holding over
  a target that can open (the Playlists tab) opens it; lists scroll at their edges; Escape
  cancels. Every drop has a keyboard way too: the same actions are in each thing's menu.
*/

/** What is being carried. */
export interface Carried {
  /** In words: "Tum Hi Ho", "Aashiqui 2". */
  label: string
  /** How many songs, when known (a playlist's are read on drop). */
  count?: number
  /** A picture for it while it is carried. */
  cover?: string
  /** The songs, read when they are dropped. */
  songs: () => Promise<QueueItem[]>
  /** Where it came from ("playlist:Road trip"), so it is never dropped onto itself. */
  from?: string
}

export interface Target {
  /** What a drop here does, said while over it: "Add to Road trip", "Play next". */
  label: string
  accepts?: (c: Carried) => boolean
  drop: (c: Carried) => void | Promise<void>
  /** Held over for a moment: open it (the Playlists tab shows the playlists). */
  dwell?: () => void
}

type State = { carried: Carried; x: number; y: number; over: string | null } | null

let state: State = null
const listeners = new Set<() => void>()
const tell = () => listeners.forEach((l) => l())
const targets = new Map<string, Target>()

const START = 6
const DWELL_MS = 650
const EDGE = 64

const subscribe = (l: () => void) => {
  listeners.add(l)
  return () => {
    listeners.delete(l)
  }
}

/** The drag now (what, where, over which target), or null; re-renders when it changes. */
export const useDrag = () => useSyncExternalStore(subscribe, () => state)

export const targetLabel = (id: string | null) => (id ? targets.get(id)?.label : undefined)

/**
 * Make something draggable: spread the result on it (`onPointerDown`). `carry` says what it
 * carries, asked when the drag starts; null means nothing to carry.
 */
export function dragSource(carry: () => Carried | null) {
  return {
    onPointerDown: (e: React.PointerEvent) => {
      if (e.button !== 0 || e.pointerType === 'touch' || state) return
      // A press on a control inside (a button, a field) stays that control's.
      const inner = (e.target as HTMLElement).closest('input, textarea, select, [data-no-drag]')
      if (inner) return
      begin(e.clientX, e.clientY, carry)
    },
  }
}

function begin(x0: number, y0: number, carry: () => Carried | null) {
  let carried: Carried | null = null
  let dwellAt = 0
  let dwellOn: string | null = null
  let dwelt = false
  let raf = 0
  let px = x0
  let py = y0

  const overAt = (x: number, y: number) => {
    const el = document.elementFromPoint(x, y)?.closest<HTMLElement>('[data-drop]')
    const id = el?.dataset.drop ?? null
    const t = id ? targets.get(id) : undefined
    return t && carried && (t.accepts?.(carried) ?? true) ? id : null
  }

  const move = (e: PointerEvent) => {
    px = e.clientX
    py = e.clientY
    if (!carried) {
      if (Math.abs(px - x0) < START && Math.abs(py - y0) < START) return
      carried = carry()
      if (!carried) return stop()
      document.documentElement.dataset.dragging = 'yes'
      window.getSelection()?.removeAllRanges()
      raf = requestAnimationFrame(edges)
    }
    const over = overAt(px, py)
    if (over !== dwellOn) {
      dwellOn = over
      dwellAt = performance.now()
      dwelt = false
    }
    state = { carried, x: px, y: py, over }
    tell()
  }

  // Near the top or bottom of whatever scrolls under the pointer: scroll it, faster the closer.
  // Held over a target that opens: open it, once.
  const edges = () => {
    const under = document.elementFromPoint(px, py) as HTMLElement | null
    let sc: HTMLElement | null = under
    while (sc && !(sc.scrollHeight > sc.clientHeight && /auto|scroll/.test(getComputedStyle(sc).overflowY))) sc = sc.parentElement
    if (sc) {
      const r = sc.getBoundingClientRect()
      const up = py - r.top
      const down = r.bottom - py
      const speed = up < EDGE ? -((EDGE - up) / EDGE) * 16 : down < EDGE ? ((EDGE - down) / EDGE) * 16 : 0
      if (speed) sc.scrollTop += speed
    }
    if (dwellOn && !dwelt && performance.now() - dwellAt > DWELL_MS) {
      dwelt = true
      targets.get(dwellOn)?.dwell?.()
    }
    raf = requestAnimationFrame(edges)
  }

  const up = () => {
    const done = carried && state?.over ? { t: targets.get(state.over), c: carried } : null
    const dragged = !!carried
    stop()
    if (dragged) swallowClick()
    if (done?.t) void Promise.resolve(done.t.drop(done.c))
  }

  const key = (e: KeyboardEvent) => {
    if (e.key !== 'Escape' || !carried) return
    e.preventDefault()
    e.stopPropagation()
    stop()
    swallowClick()
  }

  function stop() {
    cancelAnimationFrame(raf)
    window.removeEventListener('pointermove', move)
    window.removeEventListener('pointerup', up)
    window.removeEventListener('pointercancel', stop)
    window.removeEventListener('keydown', key, true)
    delete document.documentElement.dataset.dragging
    if (state) {
      state = null
      tell()
    }
  }

  window.addEventListener('pointermove', move)
  window.addEventListener('pointerup', up)
  window.addEventListener('pointercancel', stop)
  window.addEventListener('keydown', key, true)
}

/** The click that ends a drag is not a click on what was under it. */
function swallowClick() {
  const eat = (e: MouseEvent) => {
    e.stopPropagation()
    e.preventDefault()
  }
  window.addEventListener('click', eat, { capture: true, once: true })
  window.setTimeout(() => window.removeEventListener('click', eat, { capture: true }), 300)
}

/**
 * Make something a place to drop music: spread `props` on it; `over` while something it takes
 * is held over it, `ready` while something it would take is being carried anywhere.
 */
export function useDropTarget(target: Target | null) {
  const id = useId()
  const latest = useRef(target)
  useEffect(() => {
    latest.current = target
  })
  const on = !!target
  useEffect(() => {
    if (!on) return
    targets.set(id, {
      get label() {
        return latest.current?.label ?? ''
      },
      accepts: (c) => latest.current?.accepts?.(c) ?? true,
      drop: (c) => latest.current?.drop(c),
      dwell: () => latest.current?.dwell?.(),
    })
    return () => {
      targets.delete(id)
    }
  }, [id, on])
  // Only what this target shows (held over it, something it takes being carried), so a page of
  // targets isn't drawn again on every move of the pointer.
  const flags = useSyncExternalStore(subscribe, () => {
    if (!state || !on) return 0
    const takes = latest.current?.accepts?.(state.carried) ?? true
    return (takes ? 1 : 0) | (state.over === id ? 2 : 0)
  })
  const over = (flags & 2) !== 0
  const ready = (flags & 1) !== 0
  // The same look for every target (index.css): a dashed outline while it would take what is
  // carried, solid while it is held over.
  return {
    props: on ? { 'data-drop': id, 'data-drop-ready': ready || undefined, 'data-drop-over': over || undefined } : {},
    over,
    ready,
  }
}
