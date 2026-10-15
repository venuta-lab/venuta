import { useEffect, useMemo, useState } from 'react'
import { playlistSourcesCheck, type BehindSource, type PlaylistSummary, type PlaylistsView, type SmartRule } from '../../lib/engine'
import { fromIs, songItemOf, usePlayerValue } from '../../lib/player'
import { toggle } from '../../lib/player'
import { Mosaic } from '../../components/Mosaic'
import { Page } from '../../components/ui'
import { IconPause, IconPlay, IconSearch } from '../../components/Icons'
import { menuFor, playlistFacts, playlistWork } from '../../lib/actions'
import { menuFromEvent } from '../../lib/menu'
import { dragSource, useDropTarget } from '../../lib/drag'
import { addSongsTo } from './ops'
import { playlistSongs, playPlaylist, plural, sourceName } from './common'
import { keyFor, matcher } from '../../lib/textsearch'

/** A playlist with no songs yet, small: opens it, has its menu, takes songs dropped on it. */
function EmptyPlaylist(props: { root: string; p: PlaylistSummary; onOpen: () => void }) {
  const { root, p } = props
  const drop = useDropTarget({
    label: `Add to ${p.name}`,
    drop: async (c) => void addSongsTo(root, p.name, (await c.songs()).map(songItemOf), c.label),
  })
  return (
    <button
      {...drop.props}
      onClick={props.onOpen}
      onContextMenu={(e) => menuFromEvent(e, menuFor('playlist', playlistFacts(p), playlistWork(root, p)))}
      className="flex max-w-[260px] items-center gap-2 rounded-full bg-ink/[0.06] px-3.5 py-1.5 text-[13px] text-ink/80 shadow-[inset_0_0_0_1px_var(--edge)] transition-colors hover:bg-ink/[0.1] hover:text-ink"
      title={p.source ? `From ${sourceName(p.source)}` : undefined}
    >
      <span aria-hidden className="text-ink/40">♪</span>
      <span className="truncate">{p.name}</span>
    </button>
  )
}

/** A playlist as a card: its mosaic, name and size; hover to play, right-click for the rest. */
export function PlaylistCard(props: {
  root: string
  p: PlaylistSummary
  rule?: SmartRule
  count?: number
  onOpen: () => void
}) {
  const { root, p } = props
  const from = usePlayerValue((s) => s.from)
  const playing = usePlayerValue((s) => s.playing)
  const isNow = fromIs(from, 'playlist', p.name)
  // Local files only (not online-only, not still-to-get).
  const local = props.count ?? Math.max(0, p.tracks - p.missing - (p.online ?? 0))
  const online = p.online ?? 0
  const toGet = p.missing
  const playable = local + online
  const line = p.smart
    ? `Smart, ${plural(playable || (props.count ?? 0), 'song')}`
    : toGet > 0 || online > 0
      ? [
          local ? `${local.toLocaleString('en-IN')} local` : '',
          online ? `${online.toLocaleString('en-IN')} online` : '',
          toGet ? `${toGet.toLocaleString('en-IN')} to get` : '',
        ]
          .filter(Boolean)
          .join(' · ')
      : plural(local, 'song')
  // Songs dropped on it are added to it (not onto itself, not into a smart playlist's rules).
  const drop = useDropTarget(
    p.smart
      ? null
      : {
          label: `Add to ${p.name}`,
          accepts: (c) => c.from !== `playlist:${p.name}`,
          drop: async (c) => void addSongsTo(root, p.name, (await c.songs()).map(songItemOf), c.label),
        },
  )
  return (
    <div
      {...drop.props}
      onContextMenu={(e) => menuFromEvent(e, menuFor('playlist', playlistFacts(p), playlistWork(root, p, props.rule)))}
      {...dragSource(() => ({ label: p.name, count: playable, songs: () => playlistSongs(root, p.name, props.rule), from: `playlist:${p.name}` }))}
      className="card group relative rounded-2xl"
    >
      <button onClick={props.onOpen} className="block w-full text-left">
        <Mosaic root={root} dirs={p.dirs} smart={p.smart} className="card-art aspect-square rounded-2xl shadow-xl shadow-black/30" />
        <div className={`mt-2.5 truncate text-[14px] font-medium ${isNow ? 'text-accent-ink' : ''}`}>{p.name}</div>
        <div className="truncate text-[12.5px] text-ink/55" title={line}>
          {line}
        </div>
        {(toGet > 0 || online > 0) && !p.smart && p.tracks > 0 && (
          <div
            className="mt-1.5 flex h-[3px] overflow-hidden rounded-full bg-ink/[0.1]"
            aria-hidden
            title={`${local} local, ${online} online, ${toGet} to get`}
          >
            {local > 0 && (
              <div className="h-full bg-ink/55" style={{ width: `${(local / p.tracks) * 100}%` }} />
            )}
            {online > 0 && (
              <div className="h-full bg-accent/70" style={{ width: `${(online / p.tracks) * 100}%` }} />
            )}
          </div>
        )}
      </button>
      {playable > 0 && (
        <div className="pointer-events-none absolute inset-x-0 top-0 aspect-square">
          <button
            aria-label={isNow && playing ? `Pause ${p.name}` : `Play ${p.name}`}
            onClick={() => (isNow ? void toggle() : void playPlaylist(root, p, 'play', props.rule))}
            className={`card-play accent-fill pointer-events-auto absolute bottom-3 right-3 grid h-11 w-11 place-items-center rounded-full text-on-accent shadow-lg shadow-black/30 transition-[opacity,translate,scale] duration-200 active:scale-[0.92] group-hover:translate-y-0 group-hover:opacity-100 focus-visible:translate-y-0 focus-visible:opacity-100 ${
              isNow ? 'opacity-100' : 'translate-y-1.5 opacity-0'
            }`}
          >
            {isNow && playing ? <IconPause /> : <IconPlay />}
          </button>
        </div>
      )}
    </div>
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
  /** Bring these Apple Music playlists in whole (the source's names). */
  onComplete?: (only: string[]) => void
  /** Shown inside Music: no page of its own, its buttons in a row at the top. */
  embedded?: boolean
}) {
  const { data, root } = props
  const [q, setQ] = useState('')
  const all = data?.playlists ?? []
  const has = matcher(q)
  const match = (p: PlaylistSummary) => has(keyFor(p, () => p.name))
  const smartNames = new Set(data?.smart.map((r) => r.name) ?? [])
  // Empty ones (no songs yet) sit in a compact row under the grid, not as full tiles.
  const empty = all.filter((p) => !p.smart && !smartNames.has(p.name) && p.tracks === 0 && match(p))
  const yours = all.filter((p) => !p.smart && !smartNames.has(p.name) && !p.source && p.tracks > 0 && match(p))
  const imported = useMemo(() => {
    const groups = new Map<string, PlaylistSummary[]>()
    for (const p of all.filter((x) => !x.smart && !smartNames.has(x.name) && x.source && x.tracks > 0 && match(x))) {
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
          : ({ name: r.name, smart: true, tracks: data?.smart_counts[r.name] ?? 0, missing: 0, online: 0, dirs, source: null, file: '', duration_ms: 0, modified: 0 } as PlaylistSummary),
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

  const intro = data
    ? `${ownCount ? `${plural(ownCount, 'playlist')} and ${plural(data.smart.length, 'smart one')}` : `${plural(data.smart.length, 'smart playlist')}, filled by rules`}. Saved as .m3u8 files beside your music, so other players and cards can use them too.`
    : 'Reading your playlists…'
  const buttons =
    ownCount === 0 && !q ? undefined : (
      <>
        <button onClick={props.onNew} className="btn btn-lg">
          New playlist
        </button>
        <button onClick={props.onImport} className="btn-primary btn-lg">
          Import & export
        </button>
      </>
    )
  const find =
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

  const body = (
    <>
      {props.error && <p className="mb-6 text-[13px] text-problem">{props.error}</p>}
      {data && !q && props.onComplete && <CompleteBanner root={root} onComplete={props.onComplete} />}
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
                  Import & export
                </button>
              </div>
            </div>
          )}
          {yours.length > 0 &&
            section(
              'Made by you',
              <div className={GRID}>
                {yours.map((p) => (
                  <PlaylistCard key={p.name} root={root} p={p} onOpen={() => props.onOpen(p.name)} />
                ))}
              </div>,
            )}
          {imported.map(([from, lists]) =>
            section(
              from === 'Playlist file' ? 'From other players' : `From ${from}`,
              <div className={GRID}>
                {lists.map((p) => (
                  <PlaylistCard key={p.name} root={root} p={p} onOpen={() => props.onOpen(p.name)} />
                ))}
              </div>,
            ),
          )}
          {empty.length > 0 &&
            section(
              'Empty',
              <div className="flex flex-wrap gap-2">
                {empty.map((p) => (
                  <EmptyPlaylist key={p.name} root={root} p={p} onOpen={() => props.onOpen(p.name)} />
                ))}
              </div>,
              <span className="text-[12.5px] text-ink/50">No songs yet: open one to add some, or drop songs on it.</span>,
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
    </>
  )

  if (props.embedded)
    return (
      <div className="pb-16">
        <div className="mb-8 flex flex-wrap items-center gap-3">
          <p className="min-w-[240px] flex-1 text-[13px] text-ink/55">{intro}</p>
          {find}
          {buttons}
        </div>
        {body}
      </div>
    )
  return (
    <Page title="Playlists" line={intro} actions={buttons} bar={find}>
      {body}
    </Page>
  )
}

/**
 * Playlists an older import brought over only in part (it kept just the songs it matched), or
 * never: offered once, to bring in whole. Checked against sources on this computer only.
 */
function CompleteBanner(props: { root: string; onComplete: (only: string[]) => void }) {
  const [behind, setBehind] = useState<BehindSource[]>([])
  const [dismissed, setDismissed] = useState(() => {
    try {
      return localStorage.getItem('venuta.complete-dismissed') ?? ''
    } catch {
      return ''
    }
  })
  useEffect(() => {
    let live = true
    const load = () =>
      playlistSourcesCheck(props.root)
        .then((b) => live && setBehind(b))
        .catch(() => {})
    void load()
    window.addEventListener('venuta:playlists-changed', load)
    return () => {
      live = false
      window.removeEventListener('venuta:playlists-changed', load)
    }
  }, [props.root])
  const short = behind.filter((b) => b.yours)
  const fresh = behind.filter((b) => !b.yours)
  const songs = short.reduce((n, b) => n + b.source_songs - b.have, 0)
  // Dismissed for what it said then; offered again only when there is more.
  const said = `${behind.length}:${songs + fresh.reduce((n, b) => n + b.source_songs, 0)}`
  if (!behind.length || said === dismissed) return null
  const dismiss = () => {
    try {
      localStorage.setItem('venuta.complete-dismissed', said)
    } catch {
      /* remembered for this visit only */
    }
    setDismissed(said)
  }
  return (
    <section className="glass mb-10 flex flex-wrap items-center gap-x-6 gap-y-4 rounded-[22px] p-5 pl-6" aria-label="Complete your Apple Music playlists">
      <div className="min-w-[280px] flex-1">
        <h2 className="text-[16px] font-semibold">
          {short.length
            ? `${plural(short.length, 'playlist')} ${short.length === 1 ? 'is' : 'are'} missing ${plural(songs, 'song')} that Apple Music still has`
            : `${plural(fresh.length, 'Apple Music playlist')} never came over`}
        </h2>
        <p className="mt-1 max-w-[70ch] text-[13px] leading-relaxed text-ink/60">
          {short.length ? 'An earlier import kept only the songs it found in your library. ' : ''}
          {short.length && fresh.length
            ? fresh.length <= 2
              ? `${fresh.map((b) => `“${b.name}”`).join(' and ')} never came over at all. `
              : `${fresh.length} more never came over at all. `
            : ''}
          Complete them: every song comes in, in Apple Music's order. Songs you have play from your files; the rest keep their places, so you can see what to get.{short.length ? ' Songs you added stay.' : ''} You review it first, and can undo it.
        </p>
      </div>
      <div className="flex gap-2">
        <button onClick={dismiss} className="btn">
          Not now
        </button>
        <button onClick={() => props.onComplete(behind.map((b) => b.name))} className="btn-primary">
          Review and complete
        </button>
      </div>
    </section>
  )
}
