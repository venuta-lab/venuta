import { AnimatePresence, motion } from 'motion/react'
import { useCallback, useEffect, useState } from 'react'
import {
  setSettings,
  spotifyCancel,
  spotifyConnect,
  spotifyDisconnect,
  spotifyStatus,
  type AccountStatus,
} from '../lib/engine'
import { confirmAsk } from './Confirm'
import { notify } from '../lib/notify'

/*
  Your Spotify account: the one-time setup (your own free Spotify app, whose Client ID Venuta
  signs in with), then Sign in, Connected, Disconnect. Signing in happens in your browser;
  Venuta never sees your password. Used in Settings and in Import playlists.
*/
const REDIRECT = 'http://127.0.0.1/callback'

export function useSpotifyStatus() {
  const [status, setStatus] = useState<AccountStatus | null>(null)
  const reload = useCallback(() => {
    spotifyStatus()
      .then(setStatus)
      .catch(() => setStatus({ ready: false, connected: false, name: null }))
  }, [])
  useEffect(() => {
    reload()
    const on = () => reload()
    window.addEventListener('venuta:spotify-changed', on)
    return () => window.removeEventListener('venuta:spotify-changed', on)
  }, [reload])
  return [status, reload] as const
}

const changed = () => window.dispatchEvent(new CustomEvent('venuta:spotify-changed'))

export function SpotifyAccount(props: { compact?: boolean; onConnected?: () => void }) {
  const [status] = useSpotifyStatus()
  const [waiting, setWaiting] = useState(false)
  const [problem, setProblem] = useState('')
  const [id, setId] = useState('')
  const [setup, setSetup] = useState(false)

  async function connect() {
    setProblem('')
    setWaiting(true)
    try {
      const name = await spotifyConnect()
      notify(`Connected to Spotify as ${name}.`)
      changed()
      props.onConnected?.()
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      if (!/cancelled/.test(msg)) setProblem(msg)
    } finally {
      setWaiting(false)
    }
  }

  async function disconnect() {
    const ok = await confirmAsk({
      title: 'Disconnect Spotify?',
      text: 'Venuta forgets the sign-in on this computer. Playlists you brought over stay. To remove Venuta from your Spotify account too, open spotify.com/account/apps.',
      confirm: 'Disconnect',
    })
    if (!ok) return
    await spotifyDisconnect()
    changed()
  }

  async function saveId() {
    const v = id.trim()
    if (!/^[0-9a-f]{32}$/i.test(v)) {
      setProblem('A Client ID is 32 letters and digits, from your app’s Settings page on Spotify for Developers.')
      return
    }
    setProblem('')
    await setSettings({ spotify_client_id: v })
    setId('')
    setSetup(false)
    changed()
  }

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text)
      notify('Copied.')
    } catch {
      notify("Couldn't reach the clipboard.")
    }
  }

  if (!status) return <div className="shimmer h-16 rounded-2xl" aria-hidden />

  const steps = (
    <ol className="mt-3 grid gap-2.5 text-[13px] leading-relaxed text-ink/75">
      <li className="flex gap-3">
        <span className="mt-[2px] grid h-5 w-5 shrink-0 place-items-center rounded-full bg-ink/[0.08] text-[11px] font-semibold text-ink/60">1</span>
        <span>
          Open <b className="select-text">developer.spotify.com/dashboard</b> and log in with your Spotify account. Spotify asks that the account
          making the app has Premium.
        </span>
      </li>
      <li className="flex gap-3">
        <span className="mt-[2px] grid h-5 w-5 shrink-0 place-items-center rounded-full bg-ink/[0.08] text-[11px] font-semibold text-ink/60">2</span>
        <span>
          <b>Create app</b>. Any name and description (“Venuta” is fine). As <b>Redirect URI</b>, add exactly{' '}
          <button onClick={() => void copy(REDIRECT)} className="select-text rounded-md bg-ink/[0.08] px-1.5 py-0.5 font-mono text-[12px] text-ink hover:bg-ink/[0.14]" title="Copy">
            {REDIRECT}
          </button>
          . Under the APIs, tick <b>Web API</b>. Save.
        </span>
      </li>
      <li className="flex gap-3">
        <span className="mt-[2px] grid h-5 w-5 shrink-0 place-items-center rounded-full bg-ink/[0.08] text-[11px] font-semibold text-ink/60">3</span>
        <span>
          Open the app’s <b>Settings</b>, copy its <b>Client ID</b> and paste it below. It isn’t a secret; your password never reaches Venuta.
        </span>
      </li>
    </ol>
  )

  return (
    <div className={props.compact ? '' : 'border-b border-edge py-4 last:border-b-0'}>
      <div className="flex flex-wrap items-center gap-3">
        <div className="min-w-[220px] flex-1">
          <div className="text-[14px] font-medium">Spotify</div>
          <div className="mt-0.5 text-[12.5px] leading-relaxed text-ink/55">
            {status.connected
              ? `Connected as ${status.name}. Venuta can read your playlists and Liked Songs; it can’t change anything.`
              : status.ready
                ? 'Sign in in your browser to bring your playlists and Liked Songs over directly.'
                : 'Bring your playlists over by signing in. It needs a free Spotify app of your own, set up once.'}
          </div>
        </div>
        {status.connected ? (
          <button onClick={() => void disconnect()} className="btn-ghost btn-sm">
            Disconnect
          </button>
        ) : status.ready ? (
          waiting ? (
            <button onClick={() => void spotifyCancel()} className="btn btn-sm">
              Cancel
            </button>
          ) : (
            <button onClick={() => void connect()} className="btn-primary">
              Sign in with Spotify
            </button>
          )
        ) : (
          !setup &&
          props.compact && (
            <button onClick={() => setSetup(true)} className="btn-primary">
              Set up
            </button>
          )
        )}
      </div>
      {waiting && (
        <p className="mt-3 flex items-center gap-2 text-[12.5px] text-ink/65" role="status">
          <span className="spin h-3.5 w-3.5 rounded-full border-2 border-ink/20 border-t-accent" aria-hidden />
          Waiting for Spotify in your browser. Agree there, then come back here.
        </p>
      )}
      <AnimatePresence initial={false}>
        {(setup || (!status.ready && !props.compact)) && !status.connected && (
          <motion.div initial={{ opacity: 0, height: 0 }} animate={{ opacity: 1, height: 'auto' }} exit={{ opacity: 0, height: 0 }} className="overflow-hidden">
            {steps}
            <div className="mt-3 flex flex-wrap gap-2">
              <input
                value={id}
                onChange={(e) => setId(e.target.value)}
                onKeyDown={(e) => e.key === 'Enter' && void saveId()}
                spellCheck={false}
                autoComplete="off"
                placeholder="Client ID"
                aria-label="Spotify Client ID"
                className="field min-w-0 flex-1 font-mono"
              />
              <button onClick={() => void saveId()} disabled={!id.trim()} className="btn-primary">
                Save
              </button>
            </div>
          </motion.div>
        )}
      </AnimatePresence>
      {status.ready && !status.connected && !waiting && (
        <button onClick={() => setSetup((o) => !o)} className="btn-ghost btn-sm mt-2">
          {setup ? 'Hide the setup' : 'Use another Client ID'}
        </button>
      )}
      {problem && <p className="mt-2 text-[12.5px] text-problem">{problem}</p>}
    </div>
  )
}
