import { invoke } from '@tauri-apps/api/core'
import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useRef, useState, type ReactNode } from 'react'
import { askAddSongs } from '../AddToPlaylist'
import { useEscape } from '../../lib/layers'
import type { Lyrics } from '../../lib/lyrics'
import { monitors, openBigScreen, toggleDesktopLyrics, useDesktopLyricsShown, type Monitor } from '../../lib/lyricWindows'
import { notify } from '../../lib/notify'
import { onlineSearch } from '../../lib/engine'
import { onlineProblem } from '../../lib/online'
import { isOnline, playItems, songItemOf, toRemoteItem, type QueueItem } from '../../lib/player'
import { useContextMenu } from '../../screens/listen/parts'
import { IconAlbum, IconBigScreen, IconDesktopLyrics, IconLyricStyle, IconMore, IconPlaylistAdd, IconTiming } from '../Icons'
import { LookPanel } from '../lyrics/LookPanel'
import { hasIndic, useLyricColour } from '../lyrics/useNowLyrics'

const spring = { type: 'spring', stiffness: 420, damping: 32 } as const

/**
 * A round icon button with its name (and key) shown under it on hover or focus, in the app's
 * glass. `on`: lit like a chosen tab (a view that's showing).
 */
function IconAction(props: {
  label: string
  keys?: string
  on?: boolean
  dot?: boolean
  onClick: (e: React.MouseEvent) => void
  children: ReactNode
  size?: 'sm' | 'md'
  /** Where the name sits: centred under it, or lined up with its right edge (at a window edge). */
  tip?: 'center' | 'end'
}) {
  const md = props.size === 'md'
  return (
    <span className="group/tip relative">
      <button
        aria-label={props.keys ? `${props.label} (${props.keys})` : props.label}
        aria-pressed={props.on}
        onClick={props.onClick}
        className={`relative grid place-items-center rounded-full transition-[color,background-color,transform] duration-150 active:scale-90 ${
          md ? 'h-9 w-9' : 'h-8 w-9'
        } ${props.on ? 'text-ink' : 'text-ink/65 hover:bg-ink/[0.08] hover:text-ink'}`}
      >
        {props.on && (
          <motion.span
            layoutId={undefined}
            initial={{ opacity: 0, scale: 0.8 }}
            animate={{ opacity: 1, scale: 1 }}
            transition={spring}
            className="absolute inset-0 rounded-full bg-ink/[0.13] shadow-[inset_0_1px_0_var(--highlight),0_1px_6px_-2px_rgb(0_0_0/0.3)]"
          />
        )}
        <span className="relative">{props.children}</span>
        {props.dot && <span className="absolute bottom-[3px] h-1 w-1 rounded-full bg-accent" aria-hidden />}
      </button>
      {/* The name, after a moment's hover (at once for the keyboard). */}
      <span
        role="tooltip"
        className={`popover pointer-events-none absolute top-full z-30 mt-2 flex translate-y-1 ${props.tip === 'end' ? 'right-0' : 'left-1/2 -translate-x-1/2'} items-center gap-2 whitespace-nowrap rounded-xl px-2.5 py-1.5 text-[12px] font-medium opacity-0 transition-[opacity,translate] duration-150 group-hover/tip:translate-y-0 group-hover/tip:opacity-100 group-hover/tip:delay-300 group-has-[:focus-visible]/tip:translate-y-0 group-has-[:focus-visible]/tip:opacity-100`}
      >
        {props.label}
        {props.keys && <kbd>{props.keys}</kbd>}
      </span>
    </span>
  )
}

/** A floating panel under its button: closes on a click outside or Esc. */
function Pop(props: { onClose: () => void; className?: string; children: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null)
  const { onClose } = props
  useEscape(true, onClose)
  useEffect(() => {
    const away = (e: PointerEvent) => {
      // A click on the button that opened it toggles it instead.
      const t = e.target as Node
      if (!ref.current?.contains(t) && !ref.current?.parentElement?.contains(t)) onClose()
    }
    document.addEventListener('pointerdown', away, true)
    return () => document.removeEventListener('pointerdown', away, true)
  }, [onClose])
  return (
    <motion.div
      ref={ref}
      initial={{ opacity: 0, y: -6, scale: 0.97 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      exit={{ opacity: 0, y: -6, scale: 0.97, transition: { duration: 0.12 } }}
      transition={spring}
      // The popover's tint twice over: nearly solid, so the lyrics moving under it never show
      // through (the blur alone doesn't reach them here).
      style={{ transformOrigin: 'top right', backgroundImage: 'linear-gradient(var(--popover), var(--popover))' }}
      className={`popover absolute right-0 top-full z-40 mt-2 rounded-[22px] ${props.className ?? ''}`}
    >
      {props.children}
    </motion.div>
  )
}

/**
 * What can be done with the lyrics, beside the tabs: their style, their timing, a whole
 * screen, the desktop. In the tabs' own material, so it reads as part of the same bar.
 */
export function LyricsDock(props: { root: string; song: QueueItem; lyrics: Lyrics | null; timing: boolean; onTiming: () => void }) {
  const [open, setOpen] = useState<'style' | 'screens' | null>(null)
  const [screens, setScreens] = useState<Monitor[]>([])
  const desktop = useDesktopLyricsShown()
  const colour = useLyricColour('cover', props.root, props.song.dir)
  const timed = !!props.lyrics?.lines.length
  const plain = !timed && !!props.lyrics?.text
  if (!timed && !plain) return null

  async function big() {
    const all = await monitors()
    if (all.length > 1) {
      setScreens(all)
      setOpen(open === 'screens' ? null : 'screens')
    } else void openBigScreen()
  }

  return (
    <motion.div
      initial={{ opacity: 0, x: 8 }}
      animate={{ opacity: 1, x: 0 }}
      exit={{ opacity: 0, x: 8 }}
      transition={spring}
      className="relative ml-auto"
    >
      <div role="toolbar" aria-label="Lyrics" className="seg inline-flex items-center rounded-full bg-ink/[0.07] p-1 shadow-[inset_0_0_0_1px_var(--edge)]">
        {timed && (
          <IconAction label="Lyric style" on={open === 'style'} onClick={() => setOpen(open === 'style' ? null : 'style')}>
            <IconLyricStyle width={19} height={19} />
          </IconAction>
        )}
        <IconAction label={timed ? 'Fix the timing' : 'Time the lyrics'} on={props.timing} onClick={props.onTiming}>
          <IconTiming width={19} height={19} />
        </IconAction>
        {timed && (
          <>
            <span className="mx-0.5 h-4 w-px bg-ink/15" aria-hidden />
            <IconAction label="Big screen" keys="F11" on={open === 'screens'} onClick={() => void big()}>
              <IconBigScreen width={19} height={19} />
            </IconAction>
            <IconAction label={desktop ? 'Hide desktop lyrics' : 'Desktop lyrics'} keys="Ctrl+Shift+D" tip="end" on={desktop} dot={desktop} onClick={() => void toggleDesktopLyrics()}>
              <IconDesktopLyrics width={19} height={19} />
            </IconAction>
          </>
        )}
      </div>

      <AnimatePresence>
        {open === 'style' && (
          <Pop key="style" onClose={() => setOpen(null)} className="w-[356px] p-4">
            <div className="mb-3">
              <div className="text-[14px] font-semibold">Lyric style</div>
              <div className="mt-0.5 text-[12px] text-ink/55">How the words look on the big screen and the desktop.</div>
            </div>
            <LookPanel tone="app" coverColour={colour} indic={hasIndic(props.lyrics?.lines)} />
          </Pop>
        )}
        {open === 'screens' && (
          <Pop key="screens" onClose={() => setOpen(null)} className="grid w-[280px] gap-1 p-2">
            <div className="px-2.5 pb-1 pt-1.5 text-[12px] text-ink/55">Show the lyrics on</div>
            {screens.map((m) => (
              <button
                key={m.name}
                onClick={() => {
                  setOpen(null)
                  void openBigScreen(m.name)
                }}
                className="flex items-center gap-3 rounded-[14px] px-2.5 py-2 text-left hover:bg-ink/[0.07]"
              >
                {/* The screen's own shape, small. */}
                <span
                  className="grid shrink-0 place-items-center rounded-[5px] bg-ink/[0.1] shadow-[inset_0_0_0_1.5px_color-mix(in_oklab,var(--ink)_35%,transparent)]"
                  style={{ width: 38, height: Math.round((38 * m.height) / m.width) }}
                >
                  {m.current && <span className="h-1 w-1 rounded-full bg-accent" />}
                </span>
                <span className="min-w-0 flex-1">
                  <span className="block text-[13.5px] font-medium">{m.current ? 'This screen' : m.primary ? 'The main screen' : 'The other screen'}</span>
                  <span className="block text-[12px] tabular-nums text-ink/50">
                    {m.width} × {m.height}
                  </span>
                </span>
              </button>
            ))}
          </Pop>
        )}
      </AnimatePresence>
    </motion.div>
  )
}

/**
 * Who and what the song is, each a way in: every artist and the album open their pages; the
 * buttons under add it to a playlist, open the album, and the rest is under More.
 */
export function SongActions(props: {
  root: string
  song: QueueItem
  lyrics: Lyrics | null
  onAlbum: (dir: string) => void
  onArtist: (name: string) => void
  onTiming: () => void
  stars: ReactNode
  /** "Playing from …", under the album. */
  from: ReactNode
}) {
  const { song } = props
  const remote = isOnline(song)
  const artists = song.artists.length ? song.artists : song.album_artist ? [song.album_artist] : []
  const moreBy = async (name: string) => {
    try {
      const found = await onlineSearch(name)
      if (!found.tracks.length) return notify(`No online songs for ${name}.`)
      void playItems(props.root, found.tracks.map(toRemoteItem), 0, { kind: 'search', ref: name, label: `Online: ${name}` })
    } catch (e) {
      notify(onlineProblem(e))
    }
  }
  const more = useContextMenu(() => {
    const rows: [string, () => void][] = [
      ...artists.slice(0, 3).map((a) => [`Go to ${a}`, () => props.onArtist(a)] as [string, () => void]),
      ...(song.album_artist && !artists.includes(song.album_artist)
        ? ([[`Go to ${song.album_artist}`, () => props.onArtist(song.album_artist)]] as [string, () => void][])
        : []),
      ['Add to playlist…', () => askAddSongs([songItemOf(song)], song.title)],
    ]
    if (!remote) {
      rows.unshift(['Go to album', () => props.onAlbum(song.dir)])
      if (props.lyrics && (props.lyrics.lines.length || props.lyrics.text)) {
        rows.push([props.lyrics.lines.length ? 'Fix the lyrics timing' : 'Time the lyrics', props.onTiming])
      } else {
        rows.push(['Add lyrics', props.onTiming])
      }
      rows.push([
        'Show in folder',
        () => void invoke('show_in_folder', { root: props.root, path: song.path }).catch((e) => notify(String(e))),
      ])
    } else if (artists[0]) {
      rows.push([`More online by ${artists[0]}`, () => void moreBy(artists[0])])
    }
    rows.push(['Search the web', () => void invoke('search_web', { words: `${song.title} ${artists[0] ?? ''}` })])
    return rows
  })
  const link = 'rounded-md transition-colors hover:text-ink hover:underline decoration-ink/35 underline-offset-[5px]'
  return (
    <>
      <p className="mt-1 line-clamp-2 text-[15px] text-ink/65">
        {artists.length
          ? artists.map((a, i) => (
              <span key={a}>
                {i > 0 && ', '}
                <button onClick={() => props.onArtist(a)} className={link} title={`Go to ${a}`}>
                  {a}
                </button>
              </span>
            ))
          : 'Unknown artist'}
      </p>
      {remote ? (
        song.album ? (
          <p className="mt-1 max-w-full truncate text-[13px] text-ink/50">{song.album}</p>
        ) : null
      ) : (
        <button onClick={() => props.onAlbum(song.dir)} className={`mt-1 max-w-full truncate text-[13px] text-ink/50 ${link}`} title="Go to the album">
          {song.album}
        </button>
      )}
      {props.from}
      <div className="mt-4 flex items-center justify-center gap-3">
        {!remote && props.stars}
        <div className="seg inline-flex items-center rounded-full bg-ink/[0.07] p-1 shadow-[inset_0_0_0_1px_var(--edge)]">
          <IconAction label="Add to playlist" onClick={() => askAddSongs([songItemOf(song)], song.title)}>
            <IconPlaylistAdd width={18} height={18} />
          </IconAction>
          {!remote && (
            <IconAction label="Go to album" onClick={() => props.onAlbum(song.dir)}>
              <IconAlbum width={18} height={18} />
            </IconAction>
          )}
          <IconAction label="More" onClick={more.onContextMenu}>
            <IconMore width={18} height={18} />
          </IconAction>
        </div>
      </div>
      {more.menu}
    </>
  )
}
