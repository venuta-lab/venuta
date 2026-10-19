import { useEffect, useSyncExternalStore } from 'react'
import { listening, listeningVersion, loadListening, subscribeListening, updateListening } from './listening'

/*
  Local only (Settings, Online; off until switched on): Venuta shows only your own music and
  goes nowhere online. The app itself refuses every online request then (app playback.rs, engine
  net.rs); these say it to the screens, so they hide what would only come back empty.
*/

/** Whether Local only is on (as last read; the defaults say off until read). */
export const isLocalOnly = () => listening().local_only

/** Whether Local only is on, kept current. */
export function useLocalOnly(): boolean {
  useSyncExternalStore(subscribeListening, listeningVersion)
  useEffect(() => {
    void loadListening()
  }, [])
  return listening().local_only
}

export const setLocalOnly = (on: boolean) => updateListening({ local_only: on })
