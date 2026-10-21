import { listen } from '@tauri-apps/api/event'
import { useEffect, useState, useSyncExternalStore } from 'react'
import { inApp, ytmSignIn } from '../lib/engine'
import { listening, listeningVersion, loadListening, subscribeListening, updateListening } from '../lib/listening'
import { useLocalOnly } from '../lib/localOnly'
import { accountLine, playChoicesOf, SAFE } from '../lib/ytmChoiceWords'
import { openTasteBuilder, saveYtmChoices, useYtmChoices } from '../lib/ytmChoices'
import { useYtmAccount } from '../lib/ytmAccount'
import { askSignOut } from './askSignOut'
import { YtmAccountsSection } from './YtmProfile'
import { notify } from '../lib/notify'
import { RemoteArt } from './RemoteArt'
import { YtmChoiceRows } from './YtmChoices'

/*
  Your YouTube Music account: signed in, the online catalog answers as you for what you chose
  (your Home and radios, your playlists and Liked Music in Library; searches only if you say so).
  You sign in in Google's own window; Venuta never sees your password, and keeps the sign-in in
  Windows' credential store. With Local only on, none of it is used, and sign-in isn't offered.
*/

export function YtmAccount() {
  const localOnly = useLocalOnly()
  const account = useYtmAccount()
  const mine = useYtmChoices()
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

  // Local only: YouTube Music is off, and Google's sign-in isn't opened.
  if (localOnly)
    return (
      <div className="mt-3 rounded-2xl bg-ink/[0.04] px-4 py-3 shadow-[inset_0_0_0_1px_var(--edge)]">
        <div className="flex items-center gap-4">
          <span className="grid h-10 w-10 shrink-0 place-items-center rounded-full bg-ink/[0.08] text-[16px] font-semibold text-ink/60">YT</span>
          <div className="min-w-[200px] flex-1">
            <div className="text-[14px] font-medium">YouTube Music</div>
            <div className="mt-0.5 text-[12.5px] leading-relaxed text-ink/55">
              YouTube Music is off while Local only is on. A sign-in kept here waits for when you turn Local only off; nothing is read, synced or counted.
            </div>
          </div>
        </div>
      </div>
    )
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
            ? `${account.email ? `${account.email}. ` : ''}${accountLine(mine?.choices ?? SAFE)}`
            : 'Sign in to have your Home and radios, and your playlists and Liked Music in Library. You choose what Venuta tells your account. You sign in in Google’s own window; Venuta never sees your password.'}
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
    {account && <YtmChoicesSection />}
    </div>
  )
}

/** Your choices for the account and channel in use: the same switches as after sign-in, each
 *  kept the moment it changes. */
function YtmChoicesSection() {
  const mine = useYtmChoices()
  useSyncExternalStore(subscribeListening, listeningVersion)
  useEffect(() => {
    void loadListening()
  }, [])
  if (!mine) return null
  return (
    <div className="mt-3 border-t border-edge pt-3">
      <div className="flex flex-wrap items-center gap-3">
        <div className="min-w-[200px] flex-1">
          <div className="text-[13.5px] font-medium">Your choices</div>
          <div className="mt-0.5 text-[12.5px] leading-relaxed text-ink/55">For the account and channel in use; each keeps its own. Signing out keeps them for when you come back.</div>
        </div>
        <button onClick={openTasteBuilder} className="btn btn-sm" title="Tell YouTube Music which artists you like, for your recommendations">
          Choose artists you like…
        </button>
      </div>
      <div className="-mx-3 mt-3">
        <YtmChoiceRows
          choices={mine.choices}
          plays={playChoicesOf(listening())}
          onChoice={(c) => void saveYtmChoices(c).catch((e) => notify(`Couldn't keep it: ${String(e)}`))}
          onPlay={(c) => void updateListening(c).catch((e) => notify(String(e)))}
        />
      </div>
    </div>
  )
}
