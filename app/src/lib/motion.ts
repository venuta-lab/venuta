import { useSyncExternalStore } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { inApp } from './engine'

/*
  The motion budget: whether this window is on screen at all. Minimised, hidden (in the tray,
  closed to the tray) or covered by nothing else of ours, it is not, and then nothing in it
  moves: every CSS animation pauses (one rule, on :root[data-awake="no"] in index.css), and
  the loops that draw each frame (the lyrics clock) ask `useAwake()` before they run. Each
  window (main, mini, big screen, desktop lyrics) answers for itself.
*/

let awake = true
const listeners = new Set<() => void>()

function set(v: boolean) {
  if (v === awake) return
  awake = v
  document.documentElement.dataset.awake = v ? 'yes' : 'no'
  listeners.forEach((l) => l())
}

let watching = false

/** Start following this window's visibility (once, at start). */
export function watchAwake() {
  if (watching) return
  watching = true
  document.documentElement.dataset.awake = 'yes'
  const win = inApp() ? getCurrentWindow() : null
  let asking = 0
  const check = async () => {
    const ask = ++asking
    const hidden = document.visibilityState === 'hidden'
    let shown = true
    if (win && !hidden) {
      // WebView2 keeps a minimised window "visible" to the page, so ask the window itself.
      const [min, vis] = await Promise.all([win.isMinimized().catch(() => false), win.isVisible().catch(() => true)])
      shown = !min && vis
    }
    // Only the newest answer counts (a restore right after a minimise).
    if (ask === asking) set(!hidden && shown)
  }
  document.addEventListener('visibilitychange', () => void check())
  window.addEventListener('focus', () => void check())
  window.addEventListener('blur', () => void check())
  // Minimising and restoring resize the window on Windows; the tray's show and hide focus it.
  void win?.onResized(() => void check())
  void check()
}

/** Whether this window is on screen now. */
export const isAwake = () => awake

/** Called whenever this window comes on screen or goes off it; returns how to stop. */
export function onAwake(l: () => void): () => void {
  listeners.add(l)
  return () => {
    listeners.delete(l)
  }
}

/** Whether this window is on screen; re-renders when that changes. */
export function useAwake(): boolean {
  return useSyncExternalStore(onAwake, () => awake)
}
