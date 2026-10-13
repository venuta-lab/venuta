import { useSyncExternalStore } from 'react'

/*
  One notice at a time at the bottom of the window, from anywhere (an add to a playlist, a
  removal with Undo). The app shows it with <Toast>.
*/
export interface Notice {
  text: string
  /** One action beside it ("Undo", "Open"). */
  action?: { label: string; run: () => void }
  /** Changes each time, so the same words twice still show again. */
  id: number
}

let current: Notice | null = null
let seq = 0
const listeners = new Set<() => void>()
const emit = () => listeners.forEach((l) => l())

export function notify(text: string, action?: Notice['action']) {
  current = { text, action, id: ++seq }
  emit()
}

export function dismissNotice() {
  current = null
  emit()
}

export function useNotice(): Notice | null {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l)
      return () => {
        listeners.delete(l)
      }
    },
    () => current,
  )
}
