import { useEffect, useState, useSyncExternalStore } from 'react'
import { Segmented, Switch } from '../../components/ui'
import { invoke } from '@tauri-apps/api/core'
import { inApp } from '../../lib/engine'
import { listening, listeningVersion, loadListening, subscribeListening, updateListening } from '../../lib/listening'
import { notify } from '../../lib/notify'
import { setMyName, TOGETHER } from '../../lib/together'

/*
  Settings > Listening: what plays next, how online songs come, which lyrics sources are asked
  and in what order, and who is never offered. Saved with your settings; the online catalog is
  told at once.
*/

/** The lyrics sources, in their own order (crates/engine lyrics_online SOURCES). */
const SOURCES: [string, string, string][] = [
  ['lrclib', 'LRCLIB', 'Timed lyrics, free and open'],
  ['youtube', 'YouTube Music', "The catalog's own lyrics, timed when it has them"],
  ['simpmusic', 'SimpMusic', 'Lyrics by the song itself (answers only SimpMusic now)'],
  ['lyricsplus', 'LyricsPlus', 'Word by word, with Roman letters for some songs'],
  ['boidu', 'Better Lyrics', 'Word by word, from Apple Music'],
  ['netease', 'Netease', 'Chinese songs, often with translations'],
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

function Row(props: { title: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-wrap items-center gap-4 border-b border-edge py-4 last:border-b-0">
      <div className="min-w-[220px] flex-1">
        <div className="text-[14px] font-medium">{props.title}</div>
        {props.hint && <div className="mt-0.5 text-[12.5px] leading-relaxed text-ink/55">{props.hint}</div>}
      </div>
      {props.children}
    </div>
  )
}

/** Listening together: the name others see, and how rooms connect (for the next room). */
function TogetherSettings() {
  const l = listening()
  // Your edits while typing; what's saved otherwise.
  const [name, setName] = useState<string | null>(null)
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
    <>
      <h3 className="mt-8 text-[13px] font-semibold text-ink/60">{TOGETHER}</h3>
      <Row title="Your name" hint="What others in a room see. Empty: this computer's user name.">
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
    </>
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

export function ListeningSettings() {
  useSyncExternalStore(subscribeListening, listeningVersion)
  useEffect(() => {
    void loadListening(true)
  }, [])
  const l = listening()
  const set = (change: Parameters<typeof updateListening>[0]) => void updateListening(change).catch((e) => notify(String(e)))
  const order = sourceOrder(l.lyrics_sources)
  const moveSource = (i: number, by: -1 | 1) => {
    const next = order.slice()
    const j = i + by
    if (j < 0 || j >= next.length) return
    ;[next[i], next[j]] = [next[j], next[i]]
    set({ lyrics_sources: toSetting(next) })
  }

  return (
    <div>
      <h2 className="title text-[22px]">Listening</h2>

      <h3 className="mt-6 text-[13px] font-semibold text-ink/60">Your music and online</h3>
      <Row
        title="Local only"
        hint="Only your own music: the online catalog's songs, shelves and searches are hidden, online songs in your playlists are skipped, and Venuta doesn't go online at all (no lyrics or details lookups, covers or update checks). Off: your files and the online catalog together."
      >
        <Switch on={l.local_only} onChange={(on) => set({ local_only: on })} label="Local only" />
      </Row>

      <h3 className="mt-6 text-[13px] font-semibold text-ink/60">What plays</h3>
      <Row title="Autoplay" hint="When the queue runs out, keep playing songs like the last one, from the online catalog (yours play from your files).">
        <Switch on={l.autoplay} onChange={(on) => set({ autoplay: on })} label="Autoplay" />
      </Row>
      <Row title="Open the player when you press play" hint="Now Playing opens each time something new plays.">
        <Switch on={l.open_player_on_play} onChange={(on) => set({ open_player_on_play: on })} label="Open the player when you press play" />
      </Row>
      <Row title="Song notifications" hint="When a new song starts while Venuta is in the background, Windows says what it is.">
        <Switch on={l.song_notifications} onChange={(on) => set({ song_notifications: on })} label="Song notifications" />
      </Row>
      <Row title="Keep a history" hint="What you play, from your files and online, for Listening's stats and history, Home and Most played. Kept with your library, never sent anywhere.">
        <Switch on={l.history} onChange={(on) => set({ history: on })} label="Keep a history" />
      </Row>

      <TogetherSettings />

      <h3 className="mt-8 text-[13px] font-semibold text-ink/60">Online songs</h3>
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
      <Row title="Hide music videos" hint="Leave videos out of search and of what plays next, so only songs come up.">
        <Switch on={l.hide_videos} onChange={(on) => set({ hide_videos: on })} label="Hide music videos" />
      </Row>
      <Row title="Levelling online songs" hint="With Levelling on (Sound), online songs are evened out by the catalog's own loudness for each, the way ReplayGain evens out your files." >
        <span className="text-[12.5px] text-ink/50">With Levelling</span>
      </Row>

      <h3 className="mt-8 text-[13px] font-semibold text-ink/60">Lyrics sources</h3>
      <p className="mt-1 text-[12.5px] leading-relaxed text-ink/55">
        For songs without lyrics of their own. Automatic takes the first source in this order with timed lyrics. Any song can use another (Now Playing, under its lyrics).
      </p>
      <ol className="mt-3 grid gap-0.5">
        {order.map((s, i) => {
          const [, label, note] = SOURCES.find(([k]) => k === s.id) ?? [s.id, s.id, '']
          return (
            <li key={s.id} className="flex items-center gap-3 rounded-xl px-2 py-1.5 hover:bg-ink/[0.04]">
              <span className="w-5 text-right text-[12px] tabular-nums text-ink/45">{i + 1}</span>
              <Switch on={s.on} onChange={(on) => set({ lyrics_sources: toSetting(order.map((x) => (x.id === s.id ? { ...x, on } : x))) })} label={label} />
              <span className="min-w-0 flex-1">
                <span className={`block truncate text-[13.5px] ${s.on ? '' : 'text-ink/45'}`}>{label}</span>
                <span className="block truncate text-[12px] text-ink/50">{note}</span>
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
      <div className="mt-2 flex flex-wrap gap-2">
        {l.lyrics_sources.length > 0 && (
          <button onClick={() => set({ lyrics_sources: [] })} className="btn-ghost btn-sm">
            Back to the usual order
          </button>
        )}
        {inApp() && (
          <button
            onClick={() =>
              void invoke<number>('lyrics_forget_all')
                .then((n) => notify(n ? `Forgot the lyrics kept for ${n.toLocaleString('en-IN')} songs; they are found again as songs play.` : 'No lyrics were kept.'))
                .catch((e) => notify(String(e)))
            }
            className="btn-ghost btn-sm"
            title="Lyrics found online are kept so each song is asked about once. Your choices per song stay."
          >
            Forget kept lyrics
          </button>
        )}
      </div>

      <h3 className="mt-8 text-[13px] font-semibold text-ink/60">Never offered</h3>
      <Row
        title="Blocked artists"
        hint={l.blocked_artists.length ? 'Radio, autoplay and suggestions never offer them. Their own pages, and songs you choose, still play.' : 'None. Block an artist from a song’s menu (Block artist).'}
      >
        <span />
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
      <Row title="Disliked songs" hint={l.disliked.length ? `${l.disliked.length.toLocaleString('en-IN')} songs radio and autoplay never play.` : 'None. Dislike an online song from its menu.'}>
        {l.disliked.length > 0 && inApp() && (
          <button onClick={() => set({ disliked: [] })} className="btn-ghost btn-sm">
            Forget them
          </button>
        )}
      </Row>
    </div>
  )
}
