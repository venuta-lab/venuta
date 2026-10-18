import { invoke } from '@tauri-apps/api/core'
import { motion } from 'motion/react'
import { useEffect, useMemo, useState, useSyncExternalStore } from 'react'
import { AvailabilityBadge } from '../../components/AvailabilityBadge'
import { confirmAsk } from '../../components/Confirm'
import { IconPlay } from '../../components/Icons'
import { RemoteArt } from '../../components/RemoteArt'
import { Segmented, Switch } from '../../components/ui'
import { trackAt } from '../../lib/catalog'
import { loadCovers, useCovers } from '../../lib/covers'
import { inApp } from '../../lib/engine'
import { listening, listeningVersion, loadListening, subscribeListening, updateListening } from '../../lib/listening'
import { notify } from '../../lib/notify'
import { play, playItems, toRemoteItem } from '../../lib/player'
import { statsOf, type Play, type SongInfo, type Stats } from '../../lib/stats'
import { spring, type Library } from './model'

/*
  Listening: what you have played, your files and online songs alike. Stats over a range (how
  long, how many, which days and hours, who and what most), and History, day by day, each song
  ready to play again. Kept with your library; History can be switched off and cleared.
*/

type Range = '7' | '30' | 'year' | 'all'
type Part = 'stats' | 'history'

function rangeOf(r: Range, now: number) {
  const today = new Date(now)
  const start = (days: number) => new Date(today.getFullYear(), today.getMonth(), today.getDate() - days + 1).getTime()
  if (r === '7') return start(7)
  if (r === '30') return start(30)
  if (r === 'year') return new Date(today.getFullYear(), 0, 1).getTime()
  return 0
}

/** 45 min, 3 h 20 min, 41 h. */
function spent(ms: number): string {
  const min = Math.round(ms / 60_000)
  if (min < 60) return `${min} min`
  const h = Math.floor(min / 60)
  const m = min % 60
  return h >= 20 || !m ? `${h} h` : `${h} h ${m} min`
}

const plural = (n: number, one: string) => `${n.toLocaleString('en-IN')} ${one}${n === 1 ? '' : 's'}`

export function ListeningView({ lib, root }: { lib: Library; root: string }) {
  const [range, setRange] = useState<Range>('30')
  const [part, setPart] = useState<Part>('stats')
  // The plays read, and the moment they were read (what "today" is for them).
  const [read, setRead] = useState<{ plays: Play[]; now: number } | null>(null)
  const [stamp, setStamp] = useState(0)
  const plays = read?.plays ?? null
  const now = read?.now ?? 0
  useSyncExternalStore(subscribeListening, listeningVersion)
  useEffect(() => {
    void loadListening()
  }, [])
  useEffect(() => {
    let live = true
    const at = Date.now()
    const since = part === 'history' ? 0 : rangeOf(range, at)
    const ask = inApp() && root ? invoke<Play[]>('listening_plays', { root, since, limit: part === 'history' ? 600 : null }) : Promise.resolve([])
    ask
      .then((p) => live && setRead({ plays: p, now: at }))
      .catch(() => live && setRead({ plays: [], now: at }))
    return () => {
      live = false
    }
  }, [root, range, part, stamp])

  const info = (file: string): SongInfo | null => {
    const t = trackAt(file)
    return t ? { title: t.title, artists: t.artists.length ? t.artists : t.album_artist ? [t.album_artist] : [], album: t.album, dir: t.dir } : null
  }
  const stats = useMemo(() => (plays && part === 'stats' ? statsOf(plays, info, { from: rangeOf(range, now), to: now, now }) : null), [plays, range, now, part, lib]) // eslint-disable-line react-hooks/exhaustive-deps

  const clear = async () => {
    const ok = await confirmAsk({ title: 'Clear your listening history?', text: 'Every play kept for this library is forgotten: Stats, History and Most played start again. Your music and playlists stay as they are.', confirm: 'Clear history', danger: true })
    if (!ok) return
    const n = await invoke<number>('listening_forget', { root, since: null }).catch(() => 0)
    notify(n ? `Forgot ${plural(n, 'play')}.` : 'There was nothing to forget.')
    setStamp((s) => s + 1)
  }

  return (
    <div className="pb-16">
      <div className="flex flex-wrap items-center gap-3">
        <Segmented
          tabs
          size="sm"
          label="Listening"
          value={part}
          onChange={setPart}
          options={[
            { value: 'stats', label: 'Stats' },
            { value: 'history', label: 'History' },
          ]}
        />
        {part === 'stats' && (
          <Segmented
            size="sm"
            label="Over"
            value={range}
            onChange={setRange}
            options={[
              { value: '7', label: '7 days' },
              { value: '30', label: '30 days' },
              { value: 'year', label: 'This year' },
              { value: 'all', label: 'All time' },
            ]}
          />
        )}
        <label className="ml-auto flex items-center gap-2 text-[12.5px] text-ink/60" title="Keep a record of what you play, from your files and online">
          <Switch on={listening().history} onChange={(on) => void updateListening({ history: on })} label="Keep a history" />
          Keep a history
        </label>
        {part === 'history' && (
          <button onClick={() => void clear()} className="btn-ghost btn-sm">
            Clear history
          </button>
        )}
      </div>

      {plays === null ? (
        <p className="mt-10 text-[13px] text-ink/45">Reading what you have played…</p>
      ) : !plays.length ? (
        <div className="mt-10 rounded-2xl border border-dashed border-edge px-6 py-12 text-center">
          <p className="text-[15px] font-medium">{listening().history ? 'Nothing played here yet.' : 'History is off.'}</p>
          <p className="mt-1.5 text-[13px] text-ink/55">
            {listening().history
              ? 'A song counts once you have heard half of it, or four minutes, from your files or online.'
              : 'Switch on Keep a history to see your stats and what you played.'}
          </p>
        </div>
      ) : part === 'stats' && stats ? (
        <StatsBody stats={stats} root={root} range={range} />
      ) : (
        <History plays={plays} root={root} info={info} now={now} />
      )}
    </div>
  )
}

function Tile({ value, label }: { value: string; label: string }) {
  return (
    <div className="glass rounded-2xl px-5 py-4">
      <div className="title text-[26px] leading-none tabular-nums">{value}</div>
      <div className="mt-1.5 text-[12.5px] text-ink/55">{label}</div>
    </div>
  )
}

function StatsBody({ stats: s, root, range }: { stats: Stats; root: string; range: Range }) {
  const max = Math.max(1, ...s.daily.map((d) => d.ms))
  const avg = s.daily.length ? s.ms / s.daily.length : 0
  const hourMax = Math.max(1, ...s.hours)
  const fmtDay = (t: number, long = false) => new Date(t).toLocaleDateString('en-IN', long ? { weekday: 'long', day: 'numeric', month: 'long' } : s.daily.length <= 7 ? { weekday: 'short' } : { day: 'numeric', month: 'short' })
  const dirs = JSON.stringify([...new Set([...s.topSongs, ...s.topAlbums].flatMap((x) => (x.dir ? [x.dir] : [])))])
  useEffect(() => {
    void loadCovers(root, JSON.parse(dirs) as string[], 96)
  }, [root, dirs])
  const cover = useCovers(96)
  const art = (dir: string | null, url: string | null, seed: string) =>
    dir && cover(dir) ? <img src={cover(dir)?.src} alt="" className="h-full w-full object-cover" draggable={false} /> : <RemoteArt url={url} seed={seed} size={40} imgClassName="h-full w-full object-cover" />

  return (
    <motion.div initial={{ opacity: 0, y: 8 }} animate={{ opacity: 1, y: 0 }} transition={spring}>
      <p className="mt-9 text-[13px] text-ink/55">{range === 'all' ? 'All the time you have listened' : range === 'year' ? 'This year you have listened' : `In the last ${range} days you listened`}</p>
      <h2 className="title hero-title mt-1 text-[44px] leading-none">{spent(s.ms)}</h2>
      <div className="mt-6 grid grid-cols-[repeat(auto-fit,minmax(150px,1fr))] gap-3">
        <Tile value={s.plays.toLocaleString('en-IN')} label="Plays" />
        <Tile value={s.songs.toLocaleString('en-IN')} label="Songs" />
        <Tile value={s.artists.toLocaleString('en-IN')} label="Artists" />
        <Tile value={s.streak.toLocaleString('en-IN')} label={s.streak === 1 ? 'Day streak' : 'Days in a row'} />
      </div>

      {s.daily.length > 1 && s.daily.length <= 400 && (
        <section className="mt-10">
          <div className="mb-3 flex flex-wrap items-baseline gap-x-4 gap-y-1">
            <h3 className="title text-[19px]">Listening activity</h3>
            <span className="text-[12.5px] text-ink/55">
              {spent(avg)} a day on average{s.busiest ? `; most on ${fmtDay(s.busiest.day, true)}, ${spent(s.busiest.ms)}` : ''}
            </span>
          </div>
          <div className="glass flex h-[150px] items-end gap-[3px] rounded-2xl px-4 pb-7 pt-4" role="img" aria-label="Time listened each day">
            {s.daily.map((d, i) => (
              <div key={d.day} className="relative flex h-full flex-1 items-end" title={`${fmtDay(d.day, true)}: ${spent(d.ms)}`}>
                <div className="w-full rounded-t-[4px] bg-accent transition-[height] duration-500" style={{ height: `${(d.ms / max) * 100}%`, minHeight: d.ms ? 3 : 0, opacity: d.ms ? 0.85 : 0.15 }} />
                {(s.daily.length <= 7 || i === 0 || i === s.daily.length - 1) && (
                  <span className="absolute -bottom-5 left-1/2 -translate-x-1/2 whitespace-nowrap text-[10.5px] text-ink/50">{fmtDay(d.day)}</span>
                )}
              </div>
            ))}
          </div>
        </section>
      )}

      <div className="mt-10 grid gap-10 lg:grid-cols-2">
        <section>
          <h3 className="title mb-3 text-[19px]">Top artists</h3>
          <ol className="grid gap-1">
            {s.topArtists.map((a, i) => (
              <li key={a.name} className="grid grid-cols-[22px_minmax(0,1fr)_auto] items-center gap-3 rounded-xl px-2 py-1.5">
                <span className="text-right text-[12.5px] tabular-nums text-ink/45">{i + 1}</span>
                <span className="min-w-0">
                  <span className="block truncate text-[13.5px] font-medium">{a.name}</span>
                  <span className="mt-1 block h-1 overflow-hidden rounded-full bg-ink/[0.08]">
                    <span className="block h-full rounded-full bg-accent" style={{ width: `${Math.max(2, a.share * 100)}%` }} />
                  </span>
                </span>
                <span className="text-right text-[12px] tabular-nums text-ink/55">
                  {spent(a.ms)}
                  <span className="block text-[11px] text-ink/40">{Math.round(a.share * 100)}% · {plural(a.plays, 'play')}</span>
                </span>
              </li>
            ))}
          </ol>
        </section>
        <section>
          <h3 className="title mb-3 text-[19px]">Top songs</h3>
          <ol className="grid gap-1">
            {s.topSongs.map((t, i) => (
              <li key={t.file}>
                <button onClick={() => playFile(root, t.file, t.title, t.artists, t.art)} className="group grid w-full grid-cols-[22px_40px_minmax(0,1fr)_auto] items-center gap-3 rounded-xl px-2 py-1.5 text-left hover:bg-ink/[0.05]">
                  <span className="text-right text-[12.5px] tabular-nums text-ink/45">{i + 1}</span>
                  <span className="relative h-10 w-10 overflow-hidden rounded-lg bg-ink/[0.06]">
                    {art(t.dir, t.art, t.title)}
                    <span className="absolute inset-0 grid place-items-center bg-scrim text-ink opacity-0 transition-opacity group-hover:opacity-100">
                      <IconPlay width={14} height={14} />
                    </span>
                  </span>
                  <span className="min-w-0">
                    <span className="block truncate text-[13.5px] font-medium">{t.title}</span>
                    <span className="block truncate text-[12px] text-ink/55">{t.artists.join(', ')}</span>
                  </span>
                  <span className="text-right text-[12px] tabular-nums text-ink/55">{plural(t.plays, 'play')}</span>
                </button>
              </li>
            ))}
          </ol>
        </section>
        <section>
          <h3 className="title mb-3 text-[19px]">Top albums</h3>
          <ol className="grid gap-1">
            {s.topAlbums.map((a, i) => (
              <li key={`${a.album}|${a.artist}`} className="grid grid-cols-[22px_40px_minmax(0,1fr)_auto] items-center gap-3 rounded-xl px-2 py-1.5">
                <span className="text-right text-[12.5px] tabular-nums text-ink/45">{i + 1}</span>
                <span className="h-10 w-10 overflow-hidden rounded-lg bg-ink/[0.06]">{art(a.dir, a.art, a.album)}</span>
                <span className="min-w-0">
                  <span className="block truncate text-[13.5px] font-medium">{a.album}</span>
                  <span className="block truncate text-[12px] text-ink/55">{a.artist}</span>
                </span>
                <span className="text-right text-[12px] tabular-nums text-ink/55">{spent(a.ms)}</span>
              </li>
            ))}
          </ol>
        </section>
        <section>
          <h3 className="title mb-3 text-[19px]">Time of day</h3>
          <div className="glass flex h-[150px] items-end gap-[3px] rounded-2xl px-4 pb-7 pt-4" role="img" aria-label="Time listened in each hour of the day">
            {s.hours.map((ms, h) => (
              <div key={h} className="relative flex h-full flex-1 items-end" title={`${String(h).padStart(2, '0')}:00: ${spent(ms)}`}>
                <div className="w-full rounded-t-[3px] bg-accent" style={{ height: `${(ms / hourMax) * 100}%`, minHeight: ms ? 3 : 0, opacity: ms ? 0.85 : 0.15 }} />
                {h % 6 === 0 && <span className="absolute -bottom-5 left-0 text-[10.5px] text-ink/50">{String(h).padStart(2, '0')}:00</span>}
              </div>
            ))}
          </div>
        </section>
      </div>
    </motion.div>
  )
}

/** Play a song from your history: your file, or the online song. */
function playFile(root: string, file: string, title: string, artists: string[], art: string | null) {
  if (file.startsWith('online:')) {
    const [, provider, ...id] = file.split(':')
    void playItems(root, [toRemoteItem({ provider, track_id: id.join(':'), title, artists, album: null, duration_ms: null, artwork_url: art })], 0, { kind: 'songs', ref: '', label: 'Your history' })
    return
  }
  const t = trackAt(file)
  if (t) void play(root, [t], 0, { kind: 'songs', ref: '', label: 'Your history' })
  else notify(`“${title}” isn't in your library any more.`)
}

function History({ plays, root, info, now }: { plays: Play[]; root: string; info: (f: string) => SongInfo | null; now: number }) {
  const days = useMemo(() => {
    const m = new Map<number, Play[]>()
    for (const p of plays) {
      const d = new Date(p.at)
      const k = new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime()
      m.set(k, [...(m.get(k) ?? []), p])
    }
    return [...m.entries()]
  }, [plays])
  const dirs = JSON.stringify([...new Set(plays.flatMap((p) => (p.file.startsWith('online:') ? [] : [info(p.file)?.dir ?? ''])).filter(Boolean))].slice(0, 200))
  useEffect(() => {
    void loadCovers(root, JSON.parse(dirs) as string[], 96)
  }, [root, dirs])
  const cover = useCovers(96)
  const today = new Date(now)
  const t0 = new Date(today.getFullYear(), today.getMonth(), today.getDate()).getTime()
  const dayName = (k: number) => (k === t0 ? 'Today' : k === new Date(today.getFullYear(), today.getMonth(), today.getDate() - 1).getTime() ? 'Yesterday' : new Date(k).toLocaleDateString('en-IN', { weekday: 'long', day: 'numeric', month: 'long', year: k < new Date(today.getFullYear(), 0, 1).getTime() ? 'numeric' : undefined }))
  return (
    <div className="mt-8 grid gap-8">
      {days.map(([k, list]) => (
        <section key={k}>
          <h3 className="title mb-2 text-[17px]">{dayName(k)}</h3>
          <ol className="grid gap-0.5">
            {list.map((p) => {
              const lib = p.file.startsWith('online:') ? null : info(p.file)
              const title = lib?.title || p.title || p.file.split('/').pop() || ''
              const artists = lib?.artists.length ? lib.artists : p.artists
              const c = lib ? cover(lib.dir) : undefined
              return (
                <li key={`${p.at}|${p.file}`}>
                  <button onClick={() => playFile(root, p.file, title, artists, p.art)} className="group grid h-[52px] w-full grid-cols-[48px_40px_minmax(0,1fr)_auto] items-center gap-3 rounded-xl px-2 text-left hover:bg-ink/[0.05]">
                    <span className="text-[12px] tabular-nums text-ink/45">{new Date(p.at).toLocaleTimeString('en-IN', { hour: '2-digit', minute: '2-digit' })}</span>
                    <span className="relative h-10 w-10 overflow-hidden rounded-lg bg-ink/[0.06]">
                      {c ? <img src={c.src} alt="" className="h-full w-full object-cover" draggable={false} /> : <RemoteArt url={p.art} seed={p.album || title} size={40} imgClassName="h-full w-full object-cover" />}
                      <span className="absolute inset-0 grid place-items-center bg-scrim text-ink opacity-0 transition-opacity group-hover:opacity-100">
                        <IconPlay width={14} height={14} />
                      </span>
                    </span>
                    <span className="min-w-0">
                      <span className="block truncate text-[13.5px] font-medium">{title}</span>
                      <span className="block truncate text-[12px] text-ink/55">{artists.join(', ')}</span>
                    </span>
                    <AvailabilityBadge availability={p.file.startsWith('online:') ? 'online' : 'local'} />
                  </button>
                </li>
              )
            })}
          </ol>
        </section>
      ))}
    </div>
  )
}
