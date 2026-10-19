import { listen } from '@tauri-apps/api/event'
import { useCallback, useEffect, useState } from 'react'
import { inApp, libraryHome, ytmAccount, ytmBringInLikes, ytmIdentities, ytmRate, ytmSavedAccounts, type YtmAccountInfo, type YtmIdentity, type YtmSavedAccount } from './engine'
import { loadLikes } from './likes'
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

/** Your YouTube Music likes into Venuta's Liked songs, once, when asked; what happened, in words. */
export async function bringInYtmLikes(): Promise<string> {
  try {
    const root = (await libraryHome()).library
    if (!root) return 'Choose your library first.'
    const r = await ytmBringInLikes(root)
    await loadLikes(root, true)
    window.dispatchEvent(new Event('venuta:playlists-changed'))
    const list = r.name ?? 'Liked songs'
    if (!r.found) return 'Your Liked Music on YouTube Music is empty.'
    return r.added
      ? `Added ${r.added.toLocaleString('en-IN')} of ${r.found.toLocaleString('en-IN')} songs to ${list}.`
      : `All ${r.found.toLocaleString('en-IN')} are in ${list} already.`
  } catch (e) {
    return `Couldn't bring in your likes: ${String(e)}`
  }
}

/** The sign-ins kept on this computer and the channels of the one in use (brand accounts),
 *  read again when the account changes. Channels are asked for only while signed in. */
export function useYtmAccounts() {
  const account = useYtmAccount()
  const [saved, setSaved] = useState<YtmSavedAccount[]>([])
  const [channels, setChannels] = useState<YtmIdentity[]>([])
  const signedIn = !!account
  useEffect(() => {
    if (!inApp()) return
    let live = true
    const read = () => {
      ytmSavedAccounts()
        .then((s) => live && setSaved(s))
        .catch(() => live && setSaved([]))
      if (signedIn)
        ytmIdentities()
          .then((c) => live && setChannels(c))
          .catch(() => live && setChannels([]))
    }
    read()
    const stop = listen('ytm-account-changed', read)
    return () => {
      live = false
      void stop.then((f) => f())
    }
  }, [signedIn])
  return { account, saved, channels: signedIn ? channels : [] }
}
