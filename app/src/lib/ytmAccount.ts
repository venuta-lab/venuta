import { listen } from '@tauri-apps/api/event'
import { useCallback, useEffect, useState } from 'react'
import { inApp, libraryHome, ytmAccount, ytmBringInLikes, ytmIdentities, ytmNewLikesIn, ytmRate, ytmSavedAccounts, type YtmAccountInfo, type YtmIdentity, type YtmSavedAccount } from './engine'
import { loadLikes } from './likes'
import { notify } from './notify'
import { loadYtmChoices } from './ytmChoices'
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


/** With "Likes and dislikes" on for the account in use: an online song gets the same rating
 *  there as in Venuta (a like, an unlike, a thumbs down). Says so only when YouTube Music
 *  doesn't take it. */
export async function rateOnAccount(q: QueueItem, rating: 'like' | 'dislike' | 'none') {
  if (!inApp() || q.online?.kind !== 'online' || q.online.provider !== 'remote_catalog') return
  const mine = await loadYtmChoices()
  if (!mine?.choices.likes) return
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

const likesLooked = new Set<string>()

/** "New YouTube likes come in": the songs liked on the account since Venuta last looked join
 *  Liked songs, quietly; at most once per start for each account (and the first look only notes
 *  what is there). Says something only when songs came in. */
export async function newLikesIn(account: string) {
  if (!inApp() || likesLooked.has(account)) return
  likesLooked.add(account)
  try {
    const root = (await libraryHome()).library
    if (!root) return
    const r = await ytmNewLikesIn(root)
    if (!r.added) return
    await loadLikes(root, true)
    window.dispatchEvent(new Event('venuta:playlists-changed'))
    const n = r.added.toLocaleString('en-IN')
    notify(`${n} ${r.added === 1 ? 'song' : 'songs'} you liked on YouTube Music joined ${r.name ?? 'Liked songs'}.`)
  } catch {
    // Quiet: it looks again at the next start.
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
