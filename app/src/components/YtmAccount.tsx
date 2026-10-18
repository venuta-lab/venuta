import { listen } from '@tauri-apps/api/event'
import { useEffect, useState } from 'react'
import { inApp, ytmSignIn, ytmSignOut } from '../lib/engine'
import { useYtmAccount } from '../lib/ytmAccount'
import { confirmAsk } from './Confirm'
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
  const signOut = async () => {
    const ok = await confirmAsk({
      title: 'Sign out of YouTube Music?',
      text: 'Venuta forgets the sign-in on this computer. Online music keeps playing, for everyone rather than for you. Your playlists and likes in Venuta stay.',
      confirm: 'Sign out',
    })
    if (!ok) return
    await ytmSignOut().catch((e) => notify(String(e)))
  }

  if (account === undefined) return <div className="shimmer mt-3 h-16 rounded-2xl" aria-hidden />
  return (
    <div className="mt-3 flex flex-wrap items-center gap-4 rounded-2xl bg-ink/[0.04] px-4 py-3 shadow-[inset_0_0_0_1px_var(--edge)]">
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
  )
}
