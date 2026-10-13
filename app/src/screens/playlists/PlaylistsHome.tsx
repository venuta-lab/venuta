import { motion } from 'motion/react'
import { useMemo, useState } from 'react'
import type { PlaylistSummary, PlaylistsView, SmartRule } from '../../lib/engine'
import { usePlayerValue } from '../../lib/player'
import { toggle } from '../../lib/player'
import { Mosaic } from '../../components/Mosaic'
import { Page } from '../../components/ui'
import { IconPause, IconPlay, IconSearch } from '../../components/Icons'
import { useContextMenu } from '../listen/parts'
import { playPlaylist, plural, sourceName } from './common'

const spring = { type: 'spring', stiffness: 360, damping: 30 } as const

export interface PlaylistMenu {
  rename: (p: PlaylistSummary) => void
  duplicate: (p: PlaylistSummary) => void
  exportFile: (p: PlaylistSummary) => void
  remove: (p: PlaylistSummary) => void
}

/** A playlist as a card: its mosaic, name and size; hover to play, right-click for the rest. */
export function PlaylistCard(props: {
  root: string
  p: PlaylistSummary
  rule?: SmartRule
  count?: number
  onOpen: () => void
  menu?: PlaylistMenu
}) {
  const { root, p } = props
  const from = usePlayerValue((s) => s.from)
  const playing = usePlayerValue((s) => s.playing)
  const isNow = from === p.name
  const songs = props.count ?? p.tracks - p.missing
  const ctx = useContextMenu(() => [
    ['Play', () => void playPlaylist(root, p, 'play', props.rule)],
    ['Shuffle', () => void playPlaylist(root, p, 'shuffle', props.rule)],
    ['Play next', () => void playPlaylist(root, p, 'next', props.rule)],
    ['Add to queue', () => void playPlaylist(root, p, 'queue', props.rule)],
    ...((props.menu && !p.smart
      ? [
          ['Rename', () => props.menu!.rename(p)],
          ['Duplicate', () => props.menu!.duplicate(p)],
          ['Export as a file…', () => props.menu!.exportFile(p)],
          ['Delete', () => props.menu!.remove(p)],
        ]
      : props.menu && p.smart
        ? [
            ['Duplicate as an ordinary playlist', () => props.menu!.duplicate(p)],
            ['Export as a file…', () => props.menu!.exportFile(p)],
          ]
        : []) as [string, () => void][]),
  ])
  return (
    <motion.div whileHover={{ y: -6 }} transition={spring} onContextMenu={ctx.onContextMenu} className="group relative">
      {ctx.menu}
      <button onClick={props.onOpen} className="block w-full text-left">
        <Mosaic root={root} dirs={p.dirs} smart={p.smart} className="aspect-square rounded-2xl shadow-xl shadow-black/30" />
        <div className={`mt-2.5 truncate text-[14px] font-medium ${isNow ? 'text-accent-ink' : ''}`}>{p.name}</div>
        <div className="truncate text-[12.5px] text-ink/55">
          {p.smart ? `Smart, ${plural(songs, 'song')}` : plural(songs, 'song')}
          {p.missing > 0 && !p.smart ? `, ${p.missing} to find` : ''}
        </div>
      </button>
      {songs > 0 && (
        <div className="pointer-events-none absolute inset-x-0 top-0 aspect-square">
          <motion.button
            whileTap={{ scale: 0.92 }}
            transition={spring}
            aria-label={isNow && playing ? `Pause ${p.name}` : `Play ${p.name}`}
            onClick={() => (isNow ? void toggle() : void playPlaylist(root, p, 'play', props.rule))}
            className={`pointer-events-auto absolute bottom-3 right-3 grid h-11 w-11 place-items-center rounded-full bg-accent text-on-accent shadow-lg shadow-black/30 transition-[opacity,translate] duration-200 group-hover:translate-y-0 group-hover:opacity-100 focus-visible:translate-y-0 focus-visible:opacity-100 ${
              isNow ? 'opacity-100' : 'translate-y-1.5 opacity-0'
            }`}
          >
            {isNow && playing ? <IconPause /> : <IconPlay />}
          </motion.button>
        </div>
      )}
    </motion.div>
  )
}

const GRID = 'grid grid-cols-[repeat(auto-fill,minmax(168px,1fr))] gap-x-5 gap-y-7'

/** All your playlists: made by you, smart, imported (by where from), and the albums to get. */
export function PlaylistsHome(props: {
  root: string
  data: PlaylistsView | null
  error: string
  onOpen: (name: string) => void
  onOpenWanted: () => void
  onNew: () => void
  onImport: () => void
  menu: PlaylistMenu
}) {
  const { data, root } = props
  const [q, setQ] = useState('')
  const all = data?.playlists ?? []
  const match = (p: PlaylistSummary) => !q.trim() || p.name.toLowerCase().includes(q.trim().toLowerCase())
  const smartNames = new Set(data?.smart.map((r) => r.name) ?? [])
  const yours = all.filter((p) => !p.smart && !smartNames.has(p.name) && !p.source && match(p))
  const imported = useMemo(() => {
    const groups = new Map<string, PlaylistSummary[]>()
    for (const p of all.filter((x) => !x.smart && !smartNames.has(x.name) && x.source && match(x))) {
      const k = sourceName(p.source) || 'Elsewhere'
      groups.set(k, [...(groups.get(k) ?? []), p])
    }
    return [...groups.entries()]
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [all, q, data])
  // Smart playlists show even before their files exist (a library without a Playlists folder).
  const smart = (data?.smart ?? [])
    .filter((r) => match({ name: r.name } as PlaylistSummary))
    .map((r) => {
      const file = all.find((x) => x.name === r.name)
      const dirs = data?.smart_dirs[r.name] ?? []
      return {
        rule: r,
        p: file
          ? { ...file, smart: true, dirs: file.dirs.length ? file.dirs : dirs }
          : ({ name: r.name, smart: true, tracks: data?.smart_counts[r.name] ?? 0, missing: 0, dirs, source: null, file: '', duration_ms: 0, modified: 0 } as PlaylistSummary),
      }
    })
  const ownCount = all.filter((p) => !p.smart && !smartNames.has(p.name)).length
  const wanted = data?.wanted.length ?? 0

  const section = (title: string, children: React.ReactNode, extra?: React.ReactNode) => (
    <section key={title} className="mt-10 first:mt-2">
      <div className="mb-4 flex items-baseline gap-3">
        <h2 className="title text-[19px]">{title}</h2>
        {extra}
      </div>
      {children}
    </section>
  )

  return (
    <Page
      title="Playlists"
      line={
        data
          ? `${ownCount ? `${plural(ownCount, 'playlist')} and ${plural(data.smart.length, 'smart one')}` : `${plural(data.smart.length, 'smart playlist')}, filled by rules`}. Saved as .m3u8 files beside your music, so other players and cards can use them too.`
          : 'Reading your playlists…'
      }
      actions={
        ownCount === 0 && !q ? undefined : (
        <>
          <button onClick={props.onNew} className="btn btn-lg">
            New playlist
          </button>
          <button onClick={props.onImport} className="btn-primary btn-lg">
            Import playlists
          </button>
        </>
        )
      }
      bar={
        all.length + (data?.smart.length ?? 0) > 6 ? (
          <div className="flex min-w-[240px] max-w-[380px] flex-1 items-center gap-2 rounded-full bg-ink/[0.07] px-4 shadow-[inset_0_0_0_1px_var(--edge)] focus-within:bg-ink/[0.1]">
            <IconSearch className="shrink-0 text-ink/45" />
            <input
              value={q}
              onChange={(e) => setQ(e.target.value)}
              onKeyDown={(e) => e.key === 'Escape' && setQ('')}
              placeholder="Find a playlist"
              aria-label="Find a playlist"
              className="min-w-0 flex-1 bg-transparent py-2 text-[13.5px] text-ink outline-none placeholder:text-ink/45"
            />
          </div>
        ) : undefined
      }
    >
      {props.error && <p className="mb-6 text-[13px] text-problem">{props.error}</p>}
      {!data ? (
        <div className={GRID} aria-hidden>
          {Array.from({ length: 6 }, (_, i) => (
            <div key={i}>
              <div className="shimmer aspect-square rounded-2xl" />
              <div className="shimmer mt-3 h-3 w-3/4 rounded-full" />
            </div>
          ))}
        </div>
      ) : (
        <>
          {ownCount === 0 && !q && (
            <div className="glass flex flex-wrap items-center gap-6 rounded-[24px] p-7">
              <div className="min-w-[260px] flex-1">
                <h2 className="title text-[22px]">Your playlists, all in one place</h2>
                <p className="mt-2 max-w-[62ch] text-[13.5px] leading-relaxed text-ink/65">
                  Make one here, or bring yours over from Spotify, Apple Music, YouTube Music, Deezer, TIDAL or another player. Songs you have
                  are matched to your files; the rest keep their place, to find later.
                </p>
              </div>
              <div className="flex gap-2">
                <button onClick={props.onNew} className="btn btn-lg">
                  New playlist
                </button>
                <button onClick={props.onImport} className="btn-primary btn-lg">
                  Import playlists
                </button>
              </div>
            </div>
          )}
          {yours.length > 0 &&
            section(
              'Made by you',
              <div className={GRID}>
                {yours.map((p) => (
                  <PlaylistCard key={p.name} root={root} p={p} onOpen={() => props.onOpen(p.name)} menu={props.menu} />
                ))}
              </div>,
            )}
          {imported.map(([from, lists]) =>
            section(
              from === 'Playlist file' ? 'From other players' : `From ${from}`,
              <div className={GRID}>
                {lists.map((p) => (
                  <PlaylistCard key={p.name} root={root} p={p} onOpen={() => props.onOpen(p.name)} menu={props.menu} />
                ))}
              </div>,
            ),
          )}
          {smart.length > 0 &&
            section(
              'Smart',
              <div className={GRID}>
                {smart.map(({ rule, p }) => (
                  <PlaylistCard
                    key={rule.name}
                    root={root}
                    p={p}
                    rule={rule}
                    count={data.smart_counts[rule.name]}
                    onOpen={() => props.onOpen(rule.name)}
                    menu={props.menu}
                  />
                ))}
              </div>,
              <span className="text-[12.5px] text-ink/50">They fill themselves from rules, every time you open Playlists.</span>,
            )}
          {wanted > 0 &&
            !q &&
            section(
              'To get',
              <button
                onClick={props.onOpenWanted}
                className="glass group flex w-full max-w-[560px] items-center gap-4 rounded-2xl p-4 text-left transition-colors hover:bg-ink/[0.06]"
              >
                <span className="grid h-14 w-14 shrink-0 place-items-center rounded-xl bg-ink/[0.08] text-[22px]" aria-hidden>
                  ◎
                </span>
                <span className="min-w-0 flex-1">
                  <span className="block text-[15px] font-medium">{plural(wanted, 'album')} your playlists want</span>
                  <span className="block text-[12.5px] text-ink/55">Songs in your playlists that aren't in your library yet, grouped by album.</span>
                </span>
                <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" className="text-ink/40 group-hover:text-ink" aria-hidden>
                  <path d="m9 6 6 6-6 6" />
                </svg>
              </button>,
            )}
          {q && !yours.length && !imported.length && !smart.length && (
            <p className="py-16 text-center text-[14px] text-ink/55">No playlist called “{q.trim()}”.</p>
          )}
        </>
      )}
    </Page>
  )
}
