import { invoke } from '@tauri-apps/api/core'
import { useEffect, useMemo, useRef, useState } from 'react'
import { IconPlay } from '../../components/Icons'
import { RemoteArt } from '../../components/RemoteArt'
import { inApp, type OnlineShelf, type OnlineTrack } from '../../lib/engine'
import { startRadio } from '../../lib/radio'
import { current, queueKey, toRemoteItem, usePlayerValue } from '../../lib/player'
import { statsOf, type Play } from '../../lib/stats'
import { trackAt } from '../../lib/catalog'
import { artistFor, useCardWidth, useNav, type Library } from './model'
import { OnlineShelfRow } from './OnlineCards'
import { OnlineSongCard } from './OnlineCards'
import { ArtistCard } from './views'
import { useLocalOnly } from '../../lib/localOnly'

/*
  Home's parts from your listening and the online catalog: the catalog's mood chips and its
  rows (Quick picks as a list of songs, the rest as shelves, more as you scroll), artists you
  have played lately, and online songs you played, for Jump back in.
*/

export interface HomeChip {
  title: string
  params: string
}
interface HomeShelf extends OnlineShelf {
  kind: string | null
}
interface OnlineHomeAnswer {
  chips: HomeChip[]
  shelves: HomeShelf[]
  continuation: string | null
}

/** The catalog's home feed for a chip (null: the whole feed), and its next rows on request. */
export function useOnlineHome(chip: string | null) {
  const [home, setHome] = useState<{ chip: string | null; chips: HomeChip[]; shelves: HomeShelf[]; next: string | null } | null>(null)
  const loading = useRef(false)
  const localOnly = useLocalOnly()
  useEffect(() => {
    if (!inApp() || localOnly) return
    let live = true
    invoke<OnlineHomeAnswer>('online_home', { params: chip, continuation: null })
      .then((h) => live && setHome((was) => ({ chip, chips: h.chips.length ? h.chips : (was?.chips ?? []), shelves: h.shelves, next: h.continuation })))
      .catch(() => live && setHome((was) => ({ chip, chips: was?.chips ?? [], shelves: [], next: null })))
    return () => {
      live = false
    }
  }, [chip, localOnly])
  const more = () => {
    if (!home?.next || loading.current) return
    loading.current = true
    invoke<OnlineHomeAnswer>('online_home', { params: null, continuation: home.next })
      .then((h) => setHome((was) => (was && was.chip === home.chip ? { ...was, shelves: [...was.shelves, ...h.shelves], next: h.continuation } : was)))
      .catch(() => setHome((was) => (was ? { ...was, next: null } : was)))
      .finally(() => (loading.current = false))
  }
  const now = home && home.chip === chip ? home : null
  // Local only: no catalog rows or chips (none asked for).
  if (localOnly) return { chips: [], shelves: [], more: null }
  return { chips: home?.chips ?? [], shelves: now?.shelves ?? null, more: now?.next ? more : null }
}

/** The catalog's mood chips: one picked filters the online rows to it. */
export function HomeChips({ chips, chip, onChip }: { chips: HomeChip[]; chip: string | null; onChip: (params: string | null) => void }) {
  if (!chips.length) return null
  const pill = (on: boolean) =>
    `shrink-0 rounded-full px-3.5 py-1.5 text-[13px] font-medium transition-colors ${on ? 'accent-fill text-on-accent' : 'bg-ink/[0.07] text-ink/75 shadow-[inset_0_0_0_1px_var(--edge)] hover:bg-ink/[0.12] hover:text-ink'}`
  return (
    <div role="toolbar" aria-label="Moods" className="no-scrollbar -mx-1 flex gap-2 overflow-x-auto px-1 py-1">
      {chip && (
        <button onClick={() => onChip(null)} className={pill(false)}>
          All
        </button>
      )}
      {chips.map((c) => (
        <button key={c.params} aria-pressed={chip === c.params} onClick={() => onChip(chip === c.params ? null : c.params)} className={pill(chip === c.params)}>
          {c.title}
        </button>
      ))}
    </div>
  )
}

const songsOf = (s: OnlineShelf): OnlineTrack[] => s.items.flatMap((i) => (i.kind === 'song' ? [i.track] : []))

/** Quick picks: songs in two columns, numbered; playing one starts its radio. */
export function QuickPicks({ shelf }: { shelf: OnlineShelf }) {
  const { root } = useNav()
  const songs = useMemo(() => songsOf(shelf).slice(0, 16), [shelf])
  const nowKey = usePlayerValue((s) => {
    const c = current(s)
    return c ? queueKey(c) : ''
  })
  if (!songs.length) return null
  return (
    <section className="mt-10">
      <h2 className="title mb-3 text-[19px]">{shelf.title}</h2>
      <ol className="grid gap-x-6 gap-y-0.5 md:grid-cols-2">
        {songs.map((t, i) => {
          const item = toRemoteItem(t)
          const now = queueKey(item) === nowKey
          return (
            <li key={t.track_id}>
              <button onClick={() => void startRadio(root, item)} className={`group grid h-[54px] w-full grid-cols-[22px_42px_minmax(0,1fr)] items-center gap-3 rounded-xl px-2 text-left hover:bg-ink/[0.05] ${now ? 'text-accent-ink' : ''}`} title={`Play “${t.title}” and songs like it`}>
                <span className="text-right text-[12.5px] tabular-nums text-ink/45">{i + 1}</span>
                <span className="relative h-[42px] w-[42px] overflow-hidden rounded-lg bg-ink/[0.06]">
                  <RemoteArt url={t.artwork_url} seed={t.album || t.title} size={42} imgClassName="h-full w-full object-cover" />
                  <span className="absolute inset-0 grid place-items-center bg-scrim text-ink opacity-0 transition-opacity group-hover:opacity-100">
                    <IconPlay width={14} height={14} />
                  </span>
                </span>
                <span className="min-w-0">
                  <span className="block truncate text-[13.5px] font-medium">{t.title}</span>
                  <span className="block truncate text-[12px] text-ink/55">{t.artists.join(', ')}</span>
                </span>
              </button>
            </li>
          )
        })}
      </ol>
    </section>
  )
}

/** The catalog's other rows, more coming in as the last one nears the screen. */
export function OnlineShelves({ shelves, more, label }: { shelves: OnlineShelf[]; more: (() => void) | null; label: string }) {
  const nav = useNav()
  const end = useRef<HTMLDivElement>(null)
  // Each time the end of the rows comes near the screen, the next rows are asked for.
  const ask = useRef(more)
  useEffect(() => {
    ask.current = more
  })
  useEffect(() => {
    const el = end.current
    if (!el) return
    const io = new IntersectionObserver((seen) => seen.some((e) => e.isIntersecting) && ask.current?.(), { rootMargin: '800px 0px' })
    io.observe(el)
    return () => io.disconnect()
  }, [shelves.length])
  return (
    <>
      {shelves.map((s, i) => (
        <OnlineShelfRow key={`${s.title}:${i}`} shelf={s} label={label} onSeeAll={s.more ? () => s.more && nav.openOnlineShelf(s.title, '', s.more) : undefined} />
      ))}
      <div ref={end} aria-hidden className="h-px" />
    </>
  )
}

/** Your plays of the last three months (for Familiar artists and Jump back in). */
export function useRecentPlays(root: string, stamp: unknown): Play[] {
  const [plays, setPlays] = useState<Play[]>([])
  useEffect(() => {
    if (!inApp() || !root) return
    let live = true
    invoke<Play[]>('listening_plays', { root, since: Date.now() - 90 * 86_400_000, limit: 5000 })
      .then((p) => live && setPlays(p))
      .catch(() => {})
    return () => {
      live = false
    }
  }, [root, stamp])
  return plays
}

/** Online songs you played lately, newest first, as cards for Jump back in. */
export function useRecentOnline(plays: Play[]) {
  const nav = useNav()
  const localOnly = useLocalOnly()
  return useMemo(() => {
    if (localOnly) return []
    const seen = new Set<string>()
    return plays
      .filter((p) => p.file.startsWith('online:') && !seen.has(p.file) && seen.add(p.file))
      .slice(0, 12)
      .map((p) => {
        const [, provider, ...id] = p.file.split(':')
        const t: OnlineTrack = { provider, track_id: id.join(':'), title: p.title ?? '', artists: p.artists, album: p.album, album_id: null, duration_ms: null, artwork_url: p.art, isrc: null }
        return { key: p.file, at: p.at, node: <OnlineSongCard track={t} now={false} onPlay={() => void startRadio(nav.root, toRemoteItem(t))} /> }
      })
  }, [plays, nav.root, localOnly])
}

/** Artists you have played most lately: yours open their page, others their online one. */
export function FamiliarArtists({ lib, plays }: { lib: Library; plays: Play[] }) {
  const nav = useNav()
  // Measured from the newest play, so the list doesn't depend on the clock while drawn.
  const top = useMemo(() => {
    if (!plays.length) return []
    const info = (file: string) => {
      const t = trackAt(file)
      return t ? { title: t.title, artists: t.artists.length ? t.artists : [t.album_artist], album: t.album, dir: t.dir } : null
    }
    const last = Math.max(...plays.map((p) => p.at))
    return statsOf(plays, info, { from: 0, to: last, now: last }, 12).topArtists.filter((a) => a.plays >= 2)
  }, [plays])
  const width = useCardWidth()
  if (top.length < 3) return null
  return (
    <section className="mt-10">
      <h2 className="title mb-3.5 text-[19px]">Familiar artists</h2>
      <div className="no-scrollbar -mx-3 -my-3 flex gap-5 overflow-x-auto px-3 py-3">
        {top.map((a) => {
          const mine = artistFor(lib, a.name)
          return (
            <div key={a.name} className="shrink-0" style={{ width: Math.round(width * 0.9) }}>
              {mine.albums.length ? (
                <ArtistCard artist={mine} />
              ) : (
                <button onClick={() => nav.openArtistNamed(a.name)} title={`Open ${a.name}`} className="lift group block w-full text-center">
                  <span className="grid aspect-square place-items-center rounded-full text-[44px] font-semibold text-ink/70 shadow-xl shadow-black/25" style={{ background: 'linear-gradient(135deg, color-mix(in oklab, var(--accent) 55%, transparent), color-mix(in oklab, var(--accent) 14%, transparent))' }}>
                    {a.name.slice(0, 1)}
                  </span>
                  <span className="mt-3 block truncate text-[14px] font-medium">{a.name}</span>
                </button>
              )}
              <span className="mt-0.5 block text-center text-[12px] text-ink/50">{a.plays.toLocaleString('en-IN')} plays</span>
            </div>
          )
        })}
      </div>
    </section>
  )
}
