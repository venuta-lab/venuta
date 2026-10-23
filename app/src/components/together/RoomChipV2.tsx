import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useRef, useState } from 'react'
import { useEscape } from '../../lib/layers'
import { useOnlineLyrics, type Lyrics } from '../../lib/lyrics'
import type { QueueItem } from '../../lib/player'
import { dropInvite, liveActions, pendingInvite, TOGETHER, viewAge } from '../../lib/together'
import type { TogetherViewV2 } from '../../lib/togetherView'
import { Initial } from './Initial'
import { chipTone, hostWords, progress, screenOf, type RoomActions } from './model'
import { RoomPanel, type PanelEnv } from './RoomPanel'

/*
  The title bar's chip and the room panel v2 (design, WP12 notes), for a View v2. Drawn from its
  props only, so the preview page (preview/) draws it for every fixture state; LiveRoomChipV2
  binds it to the app.
*/

const RING = {
  accent: 'shadow-[0_0_0_1.5px_var(--accent)]',
  warning: 'shadow-[0_0_0_1.5px_color-mix(in_oklab,var(--problem)_75%,transparent)]',
  faded: 'shadow-[0_0_0_1.5px_color-mix(in_oklab,var(--ink)_35%,transparent)]',
}

export function RoomChipV2(props: {
  v: TogetherViewV2
  act: RoomActions
  env: PanelEnv
  /** An invite link Venuta was opened with: opens the panel, ready to join. */
  offered?: string | null
  dropOffered?: () => void
  /** Open from the start (the preview). */
  startOpen?: boolean
}) {
  const { v, act, env, offered = null, dropOffered, startOpen = false } = props
  const [shut, setOpen] = useState(startOpen)
  const open = shut || offered !== null
  const box = useRef<HTMLDivElement>(null)
  const closeIt = () => {
    setOpen(false)
    dropOffered?.()
  }
  useEscape(open, closeIt)
  useEffect(() => {
    if (!open || startOpen) return
    const close = (e: PointerEvent) => {
      if (box.current?.contains(e.target as Node)) return
      setOpen(false)
      dropOffered?.()
    }
    window.addEventListener('pointerdown', close)
    return () => window.removeEventListener('pointerdown', close)
  }, [open, startOpen, dropOffered])
  // Someone knocking opens the host's panel (so the request is seen at once); so does a request.
  const asks = v.host ? (v.room?.waiting.length ?? 0) + (v.room?.requests.length ?? 0) : 0
  const lastAsks = useRef(0)
  useEffect(() => {
    if (asks > lastAsks.current) setOpen(true)
    lastAsks.current = asks
  }, [asks])

  const screen = screenOf(v)
  const inRoom = screen === 'room' || screen === 'door'
  const people = v.room?.members ?? []
  const knocks = v.host ? (v.room?.waiting.length ?? 0) : 0
  return (
    <div ref={box} className="relative mr-1 flex items-center">
      {inRoom ? (
        <button
          onClick={() => setOpen((o) => !o)}
          aria-expanded={open}
          aria-label={`${TOGETHER}: ${people.length} ${people.length === 1 ? 'person' : 'people'}`}
          title={TOGETHER}
          className={`flex items-center gap-1 rounded-full bg-ink/[0.06] py-[3px] pl-1 pr-2.5 text-[12px] text-ink/80 hover:bg-ink/[0.1] ${RING[chipTone(v)]}`}
        >
          <span className="flex">
            {people.slice(0, 3).map((m, i) => (
              <span key={m.id} style={{ marginLeft: i ? -7 : 0 }} className="rounded-full ring-2 ring-[var(--app-bg,transparent)]">
                <Initial m={m} size={20} />
              </span>
            ))}
          </span>
          {people.length > 3 && <span>+{people.length - 3}</span>}
          {knocks > 0 && <span className="ml-0.5 h-1.5 w-1.5 rounded-full bg-accent" aria-label={`${knocks} waiting`} />}
          {screen === 'door' && <span className="ml-1">Waiting...</span>}
        </button>
      ) : (
        <button
          onClick={() => (open ? closeIt() : setOpen(true))}
          aria-expanded={open}
          aria-label={TOGETHER}
          title={TOGETHER}
          className={`grid h-8 w-8 place-items-center rounded-full text-ink/65 transition-colors hover:bg-ink/10 hover:text-ink ${screen === 'joining' ? RING.accent : ''}`}
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
            <circle cx="9" cy="8" r="3.2" />
            <path d="M3.5 19c.6-3.2 2.8-5 5.5-5s4.9 1.8 5.5 5" />
            <circle cx="16.5" cy="9" r="2.6" />
            <path d="M15.5 14.2c2.4-.3 4.4 1.2 5 4.3" />
          </svg>
        </button>
      )}
      <AnimatePresence>
        {open && (
          <motion.div
            initial={startOpen ? false : { opacity: 0, y: -6 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -6 }}
            transition={{ duration: 0.16 }}
            className="popover absolute right-0 top-10 z-50 w-[380px] max-w-[calc(100vw-16px)] overflow-hidden rounded-2xl"
            role="dialog"
            aria-label={TOGETHER}
          >
            <div className="thin-scroll max-h-[calc(100vh-64px)] overflow-y-auto p-3.5">
              <RoomPanel v={v} act={act} env={env} offered={offered} />
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  )
}

/**
 * The words of the room's song for a remote: a song only the host has, as the host sent them;
 * a catalog song's, from the lyrics services (as a queue row).
 */
function useRoomLyrics(v: TogetherViewV2): Lyrics | null {
  const remote = v.you?.mode === 'one_speaker'
  const song = remote ? progress(v, 0)?.entry.song : undefined
  const fromHost: Lyrics | null = hostWords(v)
  // A new object with each view is fine: the lyrics are looked up by the song's key and words.
  const item: QueueItem | undefined = song?.online_id
    ? {
        path: '',
        dir: '',
        title: song.title,
        artists: song.artists,
        album: song.album ?? '',
        album_artist: '',
        duration_ms: song.duration_ms ?? 0,
        format: '',
        bit_depth: 0,
        sample_rate: 0,
        lyrics: false,
        stars: 0,
        rg: { track_db: null, album_db: null, track_peak: null, album_peak: null },
        online: { kind: 'online', provider: 'remote_catalog', track_id: song.online_id },
      }
    : undefined
  const online = useOnlineLyrics(item, !!item && !fromHost)
  return fromHost ?? online
}

/** The chip and panel v2, bound to the app. */
export function LiveRoomChipV2({ v }: { v: TogetherViewV2 }) {
  const lyrics = useRoomLyrics(v)
  return <RoomChipV2 v={v} act={liveActions} env={{ since: viewAge, lyrics }} offered={pendingInvite()} dropOffered={dropInvite} />
}
