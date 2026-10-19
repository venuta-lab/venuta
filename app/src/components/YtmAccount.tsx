import { listen } from '@tauri-apps/api/event'
import { useEffect, useState, useSyncExternalStore } from 'react'
import { inApp, ytmSignIn } from '../lib/engine'
import { listening, listeningVersion, loadListening, subscribeListening, updateListening } from '../lib/listening'
import { Switch } from './ui'
import { bringInYtmLikes, useYtmAccount } from '../lib/ytmAccount'
import { askSignOut } from './askSignOut'
import { YtmAccountsSection } from './YtmProfile'
import { notify } from '../lib/notify'
import { RemoteArt } from './RemoteArt'

/*
  Your YouTube Music account: signed in, the online catalog answers for you (your Home, your
  search and radios, your playlists and Liked Music in Library). You sign in in Google's own
  window; Venuta never sees your password, and keeps the sign-in in Windows' credential store.
*/

export function YtmAccount() {
  const account = useYtmAccount()
  const [waiting, setWaiting] = useState(false)
  useEffect(() => {
    if (!inApp()) return
    const stop = listen<string>('ytm-account-error', (e) => {
      setWaiting(false)
      notify(e.payload)
    })
    return () => {
      void stop.then((f) => f())
    }
  }, [])
  useEffect(() => {
    if (account) setWaiting(false)
  }, [account])

  const signIn = async () => {
    setWaiting(true)
    try {
      await ytmSignIn()
    } catch (e) {
      setWaiting(false)
      notify(String(e))
    }
  }
  const signOut = askSignOut

  if (account === undefined) return <div className="shimmer mt-3 h-16 rounded-2xl" aria-hidden />
  return (
    <div className="mt-3 rounded-2xl bg-ink/[0.04] px-4 py-3 shadow-[inset_0_0_0_1px_var(--edge)]">
    <div className="flex flex-wrap items-center gap-4">
      <span className="grid h-10 w-10 shrink-0 place-items-center overflow-hidden rounded-full bg-ink/[0.08] text-[16px] font-semibold text-ink/60">
        {account?.picture ? <RemoteArt url={account.picture} size={40} fallback={(account.name ?? 'Y').slice(0, 1)} imgClassName="h-full w-full object-cover" /> : account ? (account.name ?? 'Y').slice(0, 1) : 'YT'}
      </span>
      <div className="min-w-[200px] flex-1">
        <div className="text-[14px] font-medium">{account ? `YouTube Music: ${account.name ?? account.handle ?? 'signed in'}` : 'YouTube Music'}</div>
        <div className="mt-0.5 text-[12.5px] leading-relaxed text-ink/55">
          {account
            ? `${account.email ? `${account.email}. ` : ''}Home, search and radios are yours; your playlists are in Library.`
            : 'Sign in to have your Home, search and radios, and your playlists and Liked Music in Library. You sign in in Google’s own window; Venuta never sees your password.'}
        </div>
      </div>
      {account ? (
        <button onClick={() => void signOut()} className="btn-ghost btn-sm">
          Sign out
        </button>
      ) : (
        <button onClick={() => void signIn()} disabled={waiting} className="btn-primary btn-sm">
          {waiting ? 'Waiting for Google…' : 'Sign in with Google'}
        </button>
      )}
    </div>
    <YtmAccountsSection />
    {account && <YtmLikes />}
    </div>
  )
}

/** Likes between Venuta and the account: the same likes there from now on (when switched on),
 *  and the ones already there brought in once, when asked. */
function YtmLikes() {
  useSyncExternalStore(subscribeListening, listeningVersion)
  useEffect(() => {
    void loadListening()
  }, [])
  const l = listening()
  const [bringing, setBringing] = useState(false)
  const bringIn = async () => {
    setBringing(true)
    try {
      notify(await bringInYtmLikes())
    } finally {
      setBringing(false)
    }
  }
  return (
    <div className="mt-3 border-t border-edge pt-3">
      <div className="flex items-center gap-4">
        <div className="min-w-[200px] flex-1">
          <div className="text-[13.5px] font-medium">Like on YouTube Music too</div>
          <div className="mt-0.5 text-[12.5px] leading-relaxed text-ink/55">Online songs you like, unlike or dislike in Venuta get the same on your account. Your own files stay in Venuta.</div>
        </div>
        <Switch on={l.sync_likes} onChange={(on) => void updateListening({ sync_likes: on }).catch((e) => notify(String(e)))} label="Like on YouTube Music too" />
      </div>
      <div className="mt-3 flex items-center gap-4">
        <div className="min-w-[200px] flex-1">
          <div className="text-[13.5px] font-medium">Count plays on YouTube Music</div>
          <div className="mt-0.5 text-[12.5px] leading-relaxed text-ink/55">
            After 30 seconds of an online song, your account is told it was played, as YouTube Music&rsquo;s own app does. Your history and recommendations then learn from Venuta too. Skips and your own files are never sent.
          </div>
        </div>
        <Switch on={l.report_plays} onChange={(on) => void updateListening({ report_plays: on }).catch((e) => notify(String(e)))} label="Count plays on YouTube Music" />
      </div>
      <div className="mt-3 flex items-center gap-4">
        <div className="min-w-[200px] flex-1">
          <div className="text-[13.5px] font-medium">Your YouTube Music likes</div>
          <div className="mt-0.5 text-[12.5px] leading-relaxed text-ink/55">Adds the songs in your Liked Music to Liked songs in Venuta. Songs liked already stay where they are.</div>
        </div>
        <button onClick={() => void bringIn()} disabled={bringing} className="btn btn-sm">
          {bringing ? 'Bringing them in…' : 'Bring them in'}
        </button>
      </div>
    </div>
  )
}
