import { useEffect, useSyncExternalStore } from 'react'
import { listening, listeningVersion, loadListening, subscribeListening, updateListening } from './listening'
import { notify } from './notify'
import { pinned, togglePin, type Shortcut } from './shortcutList'
import type { MenuItem } from './menu'

/*
  Your shortcuts on Home, kept with your settings (lib/shortcutList.ts says what they are).
*/

export type { Shortcut }

export const shortcuts = (): Shortcut[] => (listening().shortcuts as Shortcut[]) ?? []

/** Your shortcuts, kept current. */
export function useShortcuts(): Shortcut[] {
  useSyncExternalStore(subscribeListening, listeningVersion)
  useEffect(() => {
    void loadListening()
  }, [])
  return shortcuts()
}

/** Pin it to Home, or take it off. */
export async function toggleShortcut(s: Shortcut, label: string) {
  await loadListening()
  const was = pinned(shortcuts(), s)
  await updateListening({ shortcuts: togglePin(shortcuts(), s) })
  notify(was ? `Took ${label} off Home.` : `Added ${label} to the top of Home.`)
}

/** The menu line for it: Add to Home, or Remove from Home when pinned. */
export function shortcutWork(s: Shortcut, label: string): MenuItem[] {
  const on = pinned(shortcuts(), s)
  return [{ label: on ? 'Remove from Home' : 'Add to Home', run: () => void toggleShortcut(s, label) }]
}
