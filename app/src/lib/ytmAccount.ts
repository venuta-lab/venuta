import { listen } from '@tauri-apps/api/event'
import { useCallback, useEffect, useState } from 'react'
import { inApp, ytmAccount, ytmRate, type YtmAccountInfo } from './engine'
import { listening, loadListening } from './listening'
import { notify } from './notify'
import type { QueueItem } from './player'

/** Who is signed in to YouTube Music (null: no one; undefined: still asking), kept current. */
export function useYtmAccount() {
  const [account, setAccount] = useState<YtmAccountInfo | null | undefined>(undefined)
  const reload = useCallback(() => {
    if (!inApp()) return setAccount(null)
    ytmAccount()
      .then(setAccount)
      .catch(() => setAccount(null))
  }, [])
  useEffect(() => {
    reload()
    if (!inApp()) return
    const stop = listen('ytm-account-changed', () => reload())
    return () => {
      void stop.then((f) => f())
    }
  }, [reload])
  return account
}


/** With "Like on YouTube Music too" on and someone signed in: an online song gets the same
 *  rating on the account as in Venuta. Says so only when YouTube Music doesn't take it. */
export async function rateOnAccount(q: QueueItem, rating: 'like' | 'dislike' | 'none') {
  if (!inApp() || q.online?.kind !== 'online' || q.online.provider !== 'remote_catalog') return
  await loadListening()
  if (!listening().sync_likes) return
  try {
    await ytmRate(q.online.track_id, rating)
  } catch (e) {
    notify(`YouTube Music didn't take it for “${q.title}”: ${String(e)}`)
  }
}
