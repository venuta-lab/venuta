import { useEffect, useState, useSyncExternalStore } from 'react'
import { Segmented, Switch } from '../../components/ui'
import { lyricsSourcesHealth, type LyricsSourceHealth } from '../../lib/engine'
import { listening, listeningVersion, loadListening, subscribeListening, updateListening } from '../../lib/listening'
import { notify } from '../../lib/notify'
import { setMyName } from '../../lib/together'
import { Row } from './parts'

/*
  The listening settings, as the pieces the Settings pages are made of: what plays next (Playback),
  how online songs come and which country the catalog answers for (Playback, Online), which lyrics
  sources are asked and in what order (Lyrics), who is never offered (Playback), the name others see
  in a room and how rooms connect (Online), and your history (System). Saved with your settings;
  the online catalog is told at once.
*/

/** The lyrics sources, in their own order (crates/engine lyrics_online SOURCES). */
const SOURCES: [string, string, string][] = [
  ['lrclib', 'LRCLIB', 'Timed lyrics, free and open'],
  ['youtube', 'YouTube Music', "The catalog's own lyrics, timed when it has them"],
  ['simpmusic', 'SimpMusic', 'Lyrics by the song itself (answers only SimpMusic now)'],
  ['lyricsplus', 'LyricsPlus', 'Word by word, with Roman letters for some songs'],
  ['boidu', 'Better Lyrics', 'Word by word, from Apple Music. Needs a key for most songs now'],
  ['netease', 'Netease', 'Chinese songs, often with translations'],
  ['amll', 'AMLL TTML DB', 'Word by word for many Asian songs, often with Roman letters. Looked up through Netease'],
  ['qq', 'QQ Music', 'Chinese songs'],
  ['kugou', 'Kugou', 'Chinese and other Asian songs'],
]

/** Countries most asked for, then the rest by name. Codes are ISO 3166. */
const REGIONS: [string, string][] = [
  ['IN', 'India'],
  ['US', 'United States'],
  ['GB', 'United Kingdom'],
  ['CA', 'Canada'],
  ['AU', 'Australia'],
  ['AE', 'United Arab Emirates'],
  ['SG', 'Singapore'],
  ['PK', 'Pakistan'],
  ['BD', 'Bangladesh'],
  ['LK', 'Sri Lanka'],
  ['NP', 'Nepal'],
  ['DE', 'Germany'],
  ['FR', 'France'],
  ['JP', 'Japan'],
  ['KR', 'South Korea'],
  ['BR', 'Brazil'],
  ['MX', 'Mexico'],
  ['ZA', 'South Africa'],
]

/** The listening settings as they are now, read once when a page shows, and the way to change them. */
function useListening() {
  useSyncExternalStore(subscribeListening, listeningVersion)
  useEffect(() => {
    void loadListening(true)
  }, [])
  const set = (change: Parameters<typeof updateListening>[0]) => void updateListening(change).catch((e) => notify(String(e)))
  return { l: listening(), set }
}

/** Local only: your own music and nothing online. */
export function LocalOnlyRow() {
  const { l, set } = useListening()
  return (
    <Row
      title="Local only"
      hint="Only your own music. The online catalog's songs, shelves and searches are hidden, online songs in your playlists are skipped, and Venuta doesn't go online at all: no lyrics or details lookups, covers or automatic update checks. Off: your files and the online catalog together."
    >
      <Switch on={l.local_only} onChange={(on) => set({ local_only: on })} label="Local only" />
    </Row>
  )
}

/** What plays when the queue runs out, and what Venuta does when a song starts. */
export function WhatPlaysRows() {
  const { l, set } = useListening()
  return (
    <>
      <Row title="Autoplay" hint="When an album or playlist ends, keep playing what YouTube Music plays after it. Also in the queue.">
        <Switch on={l.autoplay} onChange={(on) => set({ autoplay: on })} label="Autoplay" />
      </Row>
      <Row title="Play my copy when I have it" hint="When a song YouTube Music plays next is in your library as the same song, it plays from your file, in the same place. Off: everything streams.">
        <Switch on={l.play_my_copy} onChange={(on) => set({ play_my_copy: on })} label="Play my copy when I have it" />
      </Row>
      <Row title="Hide music videos" hint="Leave videos out of search and of what plays next, so only songs come up.">
        <Switch on={l.hide_videos} onChange={(on) => set({ hide_videos: on })} label="Hide music videos" />
      </Row>
      <Row title="Open Now Playing when you press play" hint="Now Playing opens each time something new plays.">
        <Switch on={l.open_player_on_play} onChange={(on) => set({ open_player_on_play: on })} label="Open Now Playing when you press play" />
      </Row>
      <Row title="Song notifications" hint="When a new song starts while Venuta is in the background, Windows says what it is.">
        <Switch on={l.song_notifications} onChange={(on) => set({ song_notifications: on })} label="Song notifications" />
      </Row>
    </>
  )
}

/** How good a stream online songs come as. */
export function StreamQualityRow() {
  const { l, set } = useListening()
  return (
    <Row title="Stream quality" hint={l.stream_quality === 'low' ? 'The smallest stream each song offers: less data, less detail.' : 'The best stream each song offers (Opus or AAC, whichever is better).'}>
      <Segmented
        size="sm"
        label="Stream quality"
        value={l.stream_quality === 'low' ? 'low' : 'auto'}
        onChange={(v) => set({ stream_quality: v })}
        options={[
          { value: 'auto', label: 'Best' },
          { value: 'low', label: 'Data saver' },
        ]}
      />
    </Row>
  )
}

/** Which country the online catalog answers for. */
export function RegionRow() {
  const { l, set } = useListening()
  return (
    <Row title="Region" hint="Which country the online catalog answers for: its charts, Home, moods and search. Automatic follows Windows' region (Settings > Time & language).">
      <select value={l.catalog_region} onChange={(e) => set({ catalog_region: e.target.value })} aria-label="Region for online music" className="field h-9 rounded-full px-3 text-[13px]">
        <option value="">Automatic</option>
        {REGIONS.map(([code, name]) => (
          <option key={code} value={code}>
            {name}
          </option>
        ))}
      </select>
    </Row>
  )
}

/** Your listening history, for Stats, Home and Most played. */
export function HistoryRow() {
  const { l, set } = useListening()
  return (
    <Row title="Keep a history" hint="What you play, from your files and online, for Stats, Home and Most played. Kept with your library, never sent anywhere. Also on Stats.">
      <Switch on={l.history} onChange={(on) => set({ history: on })} label="Keep a history" />
    </Row>
  )
}

/** Listening together: the name others see, and hiding your address. */
export function TogetherRows() {
  const { l } = useListening()
  // Your edits while typing; what's saved otherwise.
  const [name, setName] = useState<string | null>(null)
  return (
    <>
      <Row title="Your name" hint="What others in a room see. Empty: this computer's user name. Also in the Listen together panel.">
        <input
          value={name ?? l.together_name}
          onChange={(e) => setName(e.target.value)}
          onBlur={() => {
            if (name !== null && name !== l.together_name) void setMyName(name)
            setName(null)
          }}
          maxLength={40}
          aria-label="Your name"
          className="field h-8 w-[220px] rounded-full px-3 text-[13px]"
        />
      </Row>
      <Row
        title="Hide your IP address"
        hint="Rooms connect through a relay only, so the people in them never see your IP address. Sound arrives a little later; sync stays the same. From the next room you start or join."
      >
        <Switch on={l.together_hide_ip} onChange={(on) => void updateListening({ together_hide_ip: on }).catch((e) => notify(String(e)))} label="Hide your IP address" />
      </Row>
    </>
  )
}

/** Your own relay for rooms (from the next room). */
export function RelayRow() {
  const { l } = useListening()
  const [relay, setRelay] = useState<string | null>(null)
  const relayNow = relay ?? l.together_relay ?? ''
  const relayOk = !relayNow.trim() || /^https:\/\/[^\s/]+/.test(relayNow.trim())
  const saveRelay = () => {
    if (relay === null) return
    const v = relay.trim()
    if (v && !relayOk) return notify('A relay address starts with https://')
    void updateListening({ together_relay: v || null })
      .then(() => setRelay(null))
      .catch((e) => notify(String(e)))
  }
  return (
    <Row
      title="Your own relay"
      hint={
        l.local_only
          ? 'With Local only on, rooms stay on your own network and use no relay.'
          : 'When two computers cannot reach each other directly, a relay passes the encrypted sound between them. Empty: the free public relays. From the next room.'
      }
    >
      <input
        value={relayNow}
        onChange={(e) => setRelay(e.target.value)}
        onBlur={saveRelay}
        onKeyDown={(e) => e.key === 'Enter' && saveRelay()}
        placeholder="https://relay.example.com"
        spellCheck={false}
        aria-label="Your own relay"
        aria-invalid={!relayOk}
        className={`field h-8 w-[260px] rounded-full px-3 text-[13px] ${relayOk ? '' : 'ring-1 ring-[#d05050]'}`}
      />
    </Row>
  )
}

/** The sources in your order, each on or off ("-id" off), the rest after in their own order. */
function sourceOrder(setting: string[]): { id: string; on: boolean }[] {
  const out: { id: string; on: boolean }[] = []
  for (const s of setting) {
    const id = s.replace(/^-/, '')
    if (SOURCES.some(([k]) => k === id) && !out.some((o) => o.id === id)) out.push({ id, on: !s.startsWith('-') })
  }
  for (const [id] of SOURCES) if (!out.some((o) => o.id === id)) out.push({ id, on: true })
  return out
}
const toSetting = (list: { id: string; on: boolean }[]) => list.map((s) => (s.on ? s.id : `-${s.id}`))

/** The lyrics sources asked while playing, in your order, each on or off. */
export function LyricsSources() {
  const { l, set } = useListening()
  const order = sourceOrder(l.lyrics_sources)
  // How each source has answered since Venuta started: one that turned Venuta away or didn't
  // answer says so, instead of looking as if it had nothing.
  const [health, setHealth] = useState<LyricsSourceHealth[]>([])
  useEffect(() => {
    void lyricsSourcesHealth().then(setHealth, () => setHealth([]))
  }, [])
  const moveSource = (i: number, by: -1 | 1) => {
    const next = order.slice()
    const j = i + by
    if (j < 0 || j >= next.length) return
    ;[next[i], next[j]] = [next[j], next[i]]
    set({ lyrics_sources: toSetting(next) })
  }
  return (
    <>
      <ol className="mt-3 grid gap-0.5">
        {order.map((s, i) => {
          const [, label, note] = SOURCES.find(([k]) => k === s.id) ?? [s.id, s.id, '']
          const h = health.find((x) => x.source === s.id)
          const silent = h?.state === 'refused' || h?.state === 'down'
          return (
            <li key={s.id} className="flex items-center gap-3 rounded-xl px-2 py-1.5 hover:bg-ink/[0.04]">
              <span className="w-5 text-right text-[12px] tabular-nums text-ink/45">{i + 1}</span>
              <Switch on={s.on} onChange={(on) => set({ lyrics_sources: toSetting(order.map((x) => (x.id === s.id ? { ...x, on } : x))) })} label={label} />
              <span className="min-w-0 flex-1">
                <span className={`block truncate text-[13.5px] ${s.on ? '' : 'text-ink/45'}`}>{label}</span>
                <span className="block truncate text-[12px] text-ink/50">{note}</span>
                {silent && (
                  <span className="block truncate text-[12px] text-problem" title={h?.why ?? undefined}>
                    {h?.state === 'refused' ? 'Not answering right now. Asked again when Venuta restarts' : 'Not answering right now'}
                  </span>
                )}
              </span>
              <button disabled={i === 0} onClick={() => moveSource(i, -1)} aria-label={`Move ${label} up`} className="grid h-7 w-7 place-items-center rounded-full text-ink/60 hover:bg-ink/[0.1] hover:text-ink disabled:opacity-30">
                ↑
              </button>
              <button disabled={i === order.length - 1} onClick={() => moveSource(i, 1)} aria-label={`Move ${label} down`} className="grid h-7 w-7 place-items-center rounded-full text-ink/60 hover:bg-ink/[0.1] hover:text-ink disabled:opacity-30">
                ↓
              </button>
            </li>
          )
        })}
      </ol>
      {l.lyrics_sources.length > 0 && (
        <div className="mt-2">
          <button onClick={() => set({ lyrics_sources: [] })} className="btn-ghost btn-sm">
            Back to the usual order
          </button>
        </div>
      )}
    </>
  )
}

/** Who radio and autoplay never offer: blocked artists and disliked songs, in one place. */
export function NeverOffered() {
  const { l, set } = useListening()
  const none = !l.blocked_artists.length && !l.disliked.length
  return (
    <>
      <Row
        title="Blocked artists and disliked songs"
        hint={
          none
            ? 'None yet. Block an artist or dislike an online song from its menu; radio and autoplay then never offer them.'
            : 'Radio, autoplay and suggestions never offer them. Their own pages, and songs you choose, still play.'
        }
      >
        {l.disliked.length > 0 && (
          <span className="flex items-center gap-2 text-[12.5px] text-ink/60">
            {`${l.disliked.length.toLocaleString('en-IN')} disliked ${l.disliked.length === 1 ? 'song' : 'songs'}`}
            <button onClick={() => set({ disliked: [] })} className="btn-ghost btn-sm">
              Forget them
            </button>
          </span>
        )}
      </Row>
      {l.blocked_artists.length > 0 && (
        <ul className="grid gap-1 pb-2">
          {l.blocked_artists.map((b) => (
            <li key={b.name} className="flex items-center gap-3 rounded-xl px-2 py-1.5 hover:bg-ink/[0.04]">
              <span className="min-w-0 flex-1 truncate text-[13.5px]">{b.name}</span>
              <button onClick={() => set({ blocked_artists: l.blocked_artists.filter((x) => x.name !== b.name) })} className="btn-ghost btn-sm">
                Unblock
              </button>
            </li>
          ))}
        </ul>
      )}
    </>
  )
}
