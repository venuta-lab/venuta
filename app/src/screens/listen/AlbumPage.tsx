import { motion } from 'motion/react'
import { memo, useMemo, useRef, useState } from 'react'
import { quality, type CatalogTrack } from '../../lib/engine'
import { useCovers, usePageAccent } from '../../lib/covers'
import { clock, play, toggle, toItem } from '../../lib/player'
import { IconPause, IconPlay, IconSelect } from '../../components/Icons'
import { PickMark, SelectBar } from '../../components/SelectBar'
import { NO_PICKS, pick, pickAll, picked, type Picks } from '../../lib/selection'
import { albumFrom, coverId, minutes, plural, spring, useNav, useNowPlaying, type Album } from './model'
import { Detail } from '../../components/ui'
import { Art, Equalizer, HiResPill, PlayButtons, QualityBadge, TrackMenu } from './parts'
import { menuFor, songFacts, songWork } from '../../lib/actions'
import { menuFromEvent } from '../../lib/menu'
import { LikeButton } from '../../components/LikeButton'
import { dragSource } from '../../lib/drag'

/** One album: its cover large, what it is, and its songs to play. */
export function AlbumPage({ album: a, origin, onBack }: { album: Album; origin?: string; onBack: () => void }) {
  const nav = useNav()
  // Picking songs to act on them together (Select).
  const [picking, setPicking] = useState(false)
  const [picks, setPicks] = useState<Picks>(NO_PICKS)
  const keys = useMemo(() => a.tracks.map((t) => t.path), [a.tracks])
  const pickedItems = useMemo(() => {
    const set = new Set(picked(picks, keys))
    return a.tracks.filter((t) => set.has(t.path)).map(toItem)
  }, [picks, keys, a.tracks])
  const stopPicking = () => {
    setPicking(false)
    setPicks(NO_PICKS)
  }
  const cover = useCovers(320)(a.dir)
  const accent = cover?.accent
  usePageAccent(accent)

  const meta = [a.year, a.genre].filter(Boolean).join(', ')
  return (
    <Detail title={a.title} onBack={onBack} glow={cover?.src} tools={<PlayButtons tracks={a.tracks} from={albumFrom(a)} dir={a.dir} size="sm" />}>
      <div className="relative">
        <div className="relative flex flex-wrap items-end gap-8">
          <motion.div
            layoutId={coverId(origin, a.dir)}
            transition={spring}
            className="aspect-square w-[240px] shrink-0 overflow-hidden rounded-3xl bg-ink/[0.06] shadow-2xl shadow-black/40"
          >
            <Art cover={cover} album={a} />
          </motion.div>
          <motion.div
            className="min-w-[260px] flex-1 pb-1"
            initial={{ opacity: 0, y: 12 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ ...spring, delay: 0.05 }}
          >
            <h1 className="title hero-title text-[34px] leading-[1.1]">{a.title}</h1>
            <button
              onClick={() => nav.openArtist(a.artist)}
              className="mt-2 rounded-md text-left text-[16px] font-medium text-ink/85 hover:text-ink hover:underline"
            >
              {a.artist}
            </button>
            {meta && <p className="mt-1 text-[13.5px] text-ink/60">{meta}</p>}
            <div className="mt-2.5 flex flex-wrap items-center gap-2 text-[13px] text-ink/60">
              <span>
                {plural(a.tracks.length, 'song')}, {minutes(a.duration_ms)}
              </span>
              <QualityBadge text={a.quality} />
              {a.hiRes && <HiResPill />}
            </div>
            <div className="mt-5 flex flex-wrap items-center gap-2.5">
              <PlayButtons tracks={a.tracks} from={albumFrom(a)} dir={a.dir} />
              <button onClick={() => nav.editDetails(a.dir)} className="btn-ghost btn-lg">
                Edit details
              </button>
              <button onClick={() => (picking ? stopPicking() : setPicking(true))} aria-pressed={picking} className="btn-ghost btn-lg">
                <IconSelect width={18} height={18} />
                Select
              </button>
            </div>
          </motion.div>
        </div>
      </div>
      <TrackList album={a} pick={picking ? { keys: picks.keys, onPick: (key, range) => setPicks((p) => pick(p, keys, key, { range })) } : undefined} />
      {/* Room under the songs, so the bar never covers the last of them. */}
      {picking && <div aria-hidden className="h-24" />}
      <SelectBar
        open={picking}
        root={nav.root}
        items={pickedItems}
        total={keys.length}
        from={albumFrom(a)}
        label={a.title}
        onAll={() => setPicks((p) => pickAll(p, keys))}
        onDone={stopPicking}
      />
    </Detail>
  )
}

const TrackList = memo(function TrackList({ album: a, pick }: { album: Album; pick?: { keys: string[]; onPick: (key: string, range: boolean) => void } }) {
  const nav = useNav()
  const { root } = nav
  // The album's cover, carried with a song dragged from here.
  const art = useCovers(320)(a.dir)
  const now = useNowPlaying()
  const list = useRef<HTMLDivElement>(null)
  const discs = useMemo(() => {
    const m = new Map<number, { track: CatalogTrack; index: number }[]>()
    a.tracks.forEach((track, index) => {
      const d = track.disc || 1
      const g = m.get(d)
      if (g) g.push({ track, index })
      else m.set(d, [{ track, index }])
    })
    return [...m.entries()]
  }, [a.tracks])

  const playAt = (i: number) => void play(root, a.tracks, i, albumFrom(a))
  const move = (i: number, by: number) => {
    const to = Math.min(a.tracks.length - 1, Math.max(0, i + by))
    list.current?.querySelector<HTMLElement>(`[data-index="${to}"]`)?.focus()
  }

  return (
    <div ref={list} className="mt-9" role="list">
      {discs.map(([disc, rows]) => (
        <div key={disc} role="presentation">
          {discs.length > 1 && <h3 className="mb-1 mt-5 px-3 text-[13px] font-semibold text-ink/65 first:mt-0">Disc {disc}</h3>}
          {rows.map(({ track: t, index: i }) => {
            const state = t.path === now.path ? (now.playing ? 'playing' : 'paused') : 'no'
            const q = quality(t)
            const isNow = state !== 'no'
            const picked = !!pick && pick.keys.includes(t.path)
            return (
              <div
                key={t.path}
                role={pick ? 'checkbox' : 'listitem'}
                aria-checked={pick ? picked : undefined}
                onContextMenu={(e) => menuFromEvent(e, menuFor('song', songFacts(t), songWork(nav, t, () => playAt(i))))}
                {...dragSource(() => ({ label: t.title, count: 1, cover: art?.src, songs: async () => [toItem(t)] }))}
                tabIndex={0}
                data-index={i}
                aria-current={isNow || undefined}
                onClick={(e) => {
                  if ((e.target as HTMLElement).closest('button, a, input') && (e.target as HTMLElement).closest('button, a, input') !== e.currentTarget) return
                  if (pick) pick.onPick(t.path, e.shiftKey)
                  // The first click of a double-click already plays it; the second must not pause it again.
                  else if (e.detail > 1) return
                  else if (isNow) void toggle()
                  else playAt(i)
                }}
                onKeyDown={(e) => {
                  if (e.target !== e.currentTarget) return
                  if (pick && (e.key === ' ' || e.key === 'Enter')) {
                    e.preventDefault()
                    pick.onPick(t.path, e.shiftKey)
                  } else if (e.key === 'Enter') {
                    e.preventDefault()
                    playAt(i)
                  } else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
                    e.preventDefault()
                    move(i, e.key === 'ArrowDown' ? 1 : -1)
                  }
                }}
                className={`row group grid min-h-[52px] grid-cols-[40px_minmax(0,1fr)_auto_32px_52px_32px] items-center gap-3 rounded-xl px-3 py-1.5 hover:bg-ink/[0.05] focus-visible:bg-ink/[0.06] active:bg-ink/[0.09] ${
                  isNow ? 'text-accent-ink' : ''
                } ${picked ? 'bg-ink/[0.07]' : ''}`}
              >
                {pick ? (
                  <span className="grid h-8 place-items-center">
                    <PickMark picked={picked} />
                  </span>
                ) : (
                <span className="relative grid h-8 place-items-center">
                  {isNow ? (
                    <span className="group-hover:hidden">
                      <Equalizer playing={state === 'playing'} />
                    </span>
                  ) : (
                    <span className="text-[13px] text-ink/50 group-hover:hidden">{t.track || i + 1}</span>
                  )}
                  {/* The song playing now pauses and resumes; any other starts. */}
                  <button
                    tabIndex={-1}
                    aria-label={state === 'playing' ? `Pause ${t.title}` : `Play ${t.title}`}
                    onClick={() => (isNow ? void toggle() : playAt(i))}
                    className="absolute inset-0 hidden place-items-center rounded-full text-ink hover:text-accent-ink group-hover:grid"
                  >
                    {state === 'playing' ? <IconPause width={15} height={15} /> : <IconPlay width={15} height={15} />}
                  </button>
                </span>
                )}
                <span className="min-w-0">
                  <span className="block truncate text-[14px] font-medium">{t.title}</span>
                  {t.artists.length > 0 && (
                    <span className={`block truncate text-[12.5px] ${isNow ? 'opacity-80' : 'text-ink/55'}`}>{t.artists.join(', ')}</span>
                  )}
                </span>
                <span>{q !== a.quality && <QualityBadge text={q} />}</span>
                <LikeButton root={nav.root} item={toItem(t)} quiet />
                <span className={`text-right text-[12.5px] ${isNow ? '' : 'text-ink/55'}`}>{clock(t.duration_ms)}</span>
                <TrackMenu track={t} label={t.title} play={() => playAt(i)} />
              </div>
            )
          })}
        </div>
      ))}
    </div>
  )
})
