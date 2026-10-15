import { AnimatePresence, motion } from 'motion/react'
import { useMemo, useRef, useState } from 'react'
import { open as openDialog, save as saveDialog } from '@tauri-apps/plugin-dialog'
import {
  openSavedPage,
  openSendLink,
  playlistOnService,
  playlistsBackup,
  playlistsExport,
  playlistText,
  playlistYoutubeLinks,
  type ExportFormat,
  type OnServiceOut,
  type PathStyle,
  type PlaylistSummary,
  type SendService,
  type YouTubeLinks,
} from '../../lib/engine'
import { matcher, searchKey } from '../../lib/textsearch'
import { notify } from '../../lib/notify'
import { Switch } from '../../components/ui'
import { plural } from './common'

/*
  Send out: playlists to a file in any format (with paths for where it goes), a backup of
  everything, or a streaming service by the free ways each one allows. Everything here is free:
  Venuta finds the songs on the service and hands you what that service takes (links to paste,
  YouTube lists to save, a page of links), with the steps.
*/

type Dest = 'file' | 'backup' | 'spotify' | 'youtube' | 'apple' | 'deezer' | 'others' | 'text'

const DESTS: { id: Dest; name: string; line: string; one?: boolean }[] = [
  { id: 'file', name: 'A file', line: 'M3U8, CSV and more, for players, phones and other apps' },
  { id: 'backup', name: 'A backup', line: 'Every playlist in one file, to bring back any time' },
  { id: 'youtube', name: 'YouTube Music', line: 'Links that open the songs as a list to save', one: true },
  { id: 'spotify', name: 'Spotify', line: "Song links to paste into Spotify's app", one: true },
  { id: 'apple', name: 'Apple Music', line: 'A page of links to add, or Apple’s Transfer', one: true },
  { id: 'deezer', name: 'Deezer', line: 'A page of links to add, or Deezer’s Transfer', one: true },
  { id: 'others', name: 'TIDAL, Amazon, JioSaavn…', line: 'A file for TuneMyMusic or Soundiiz', one: true },
  { id: 'text', name: 'Copy as text', line: 'Artist and title, one song a line', one: true },
]

const FORMATS: { value: ExportFormat; label: string; ext: string }[] = [
  { value: 'm3u8', label: 'M3U8: players, phones, Venuta', ext: 'm3u8' },
  { value: 'csv', label: 'CSV: Soundiiz, TuneMyMusic, spreadsheets', ext: 'csv' },
  { value: 'jspf', label: 'JSPF: ListenBrainz, Venuta (keeps everything)', ext: 'jspf' },
  { value: 'xspf', label: 'XSPF: VLC and others', ext: 'xspf' },
  { value: 'm3u', label: 'M3U: older players and devices', ext: 'm3u' },
  { value: 'pls', label: 'PLS: Winamp, Sonos', ext: 'pls' },
  { value: 'wpl', label: 'WPL: Windows Media Player', ext: 'wpl' },
  { value: 'itunes_xml', label: 'iTunes XML: Music on a Mac', ext: 'xml' },
  { value: 'txt', label: 'Text: a list of songs', ext: 'txt' },
]

const STYLES: { value: PathStyle['kind']; label: string; line: string }[] = [
  { value: 'this_pc', label: 'This PC', line: 'Full paths, for players on this computer.' },
  { value: 'next_to_playlist', label: 'Next to the playlist', line: 'Relative to where you save it, so it moves with your music.' },
  { value: 'in_music_folder', label: 'A phone or music player', line: 'Put the file in the Music folder there (Poweramp, FiiO, Sony Walkman, Rockbox…).' },
  { value: 'starts_with', label: 'A music server', line: 'Each path starts with how the server sees your Music folder (Navidrome, Jellyfin, Plex).' },
]

const pathFormats: ExportFormat[] = ['m3u8', 'm3u', 'pls', 'xspf', 'wpl', 'itunes_xml']
const keepsMissing: ExportFormat[] = ['m3u8', 'm3u', 'xspf', 'jspf', 'csv', 'txt']

const why = (e: unknown) => (e instanceof Error ? e.message : String(e))

export function SendOut(props: { root: string; playlists: PlaylistSummary[]; start?: { names: string[] } }) {
  const { root } = props
  const [chosen, setChosen] = useState<Set<string>>(() => new Set(props.start?.names ?? []))
  const [q, setQ] = useState('')
  const [dest, setDest] = useState<Dest | null>(props.start?.names.length ? 'file' : null)
  const shown = useMemo(() => {
    if (!q.trim()) return props.playlists
    const m = matcher(q)
    return props.playlists.filter((p) => m(searchKey(p.name)))
  }, [q, props.playlists])
  const names = props.playlists.filter((p) => chosen.has(p.name)).map((p) => p.name)
  const one = names.length === 1 ? names[0] : null
  const toggle = (n: string) =>
    setChosen((c) => {
      const next = new Set(c)
      if (next.has(n)) next.delete(n)
      else next.add(n)
      return next
    })
  const d = DESTS.find((x) => x.id === dest)
  const shownFor = useRef<Dest | null>(null)

  return (
    <div className="mt-8 grid items-start gap-6 lg:grid-cols-[320px_minmax(0,1fr)]">
      <section className="glass rounded-[20px] p-4" aria-label="Playlists to send">
        <div className="flex items-center gap-2">
          <h2 className="flex-1 text-[14px] font-semibold">Playlists</h2>
          <button onClick={() => setChosen(new Set(props.playlists.map((p) => p.name)))} className="btn-ghost btn-sm">
            All
          </button>
          <button onClick={() => setChosen(new Set())} className="btn-ghost btn-sm">
            None
          </button>
        </div>
        {props.playlists.length > 8 && (
          <input value={q} onChange={(e) => setQ(e.target.value)} placeholder="Find a playlist" aria-label="Find a playlist" className="field mt-2 w-full" />
        )}
        <div className="fade-y thin-scroll mt-2 grid max-h-[460px] gap-0.5 overflow-y-auto" role="group" aria-label="Playlists">
          {shown.map((p) => (
            <label key={p.name} className="flex cursor-pointer items-center gap-3 rounded-xl px-2 py-1.5 hover:bg-ink/[0.05]">
              <input type="checkbox" checked={chosen.has(p.name)} onChange={() => toggle(p.name)} className="h-4 w-4 accent-[var(--accent)]" />
              <span className="min-w-0 flex-1">
                <span className="block truncate text-[13.5px]">{p.name}</span>
                <span className="block truncate text-[12px] text-ink/50">
                  {p.tracks ? plural(p.tracks, 'song') : 'Empty'}
                  {p.smart ? ', smart' : ''}
                </span>
              </span>
            </label>
          ))}
          {!shown.length && <p className="px-2 py-3 text-[12.5px] text-ink/50">No playlist by that name.</p>}
        </div>
        <p className="mt-3 text-[12.5px] text-ink/55">{names.length ? `${plural(names.length, 'playlist')} chosen` : 'Choose the playlists to send.'}</p>
      </section>

      <div className="min-w-0">
        <div role="radiogroup" aria-label="Send to" className="grid grid-cols-[repeat(auto-fill,minmax(210px,1fr))] gap-2.5">
          {DESTS.map((x) => {
            const on = dest === x.id
            return (
              <button
                key={x.id}
                role="radio"
                aria-checked={on}
                onClick={() => setDest(on ? null : x.id)}
                className={`rounded-2xl p-3.5 text-left transition-[background-color,box-shadow] active:scale-[0.99] ${
                  on
                    ? 'bg-[color-mix(in_oklab,var(--accent)_16%,transparent)] shadow-[inset_0_0_0_1.5px_color-mix(in_oklab,var(--accent)_70%,transparent)]'
                    : 'bg-ink/[0.05] shadow-[inset_0_0_0_1px_var(--edge)] hover:bg-ink/[0.08]'
                }`}
              >
                <span className="block text-[14.5px] font-semibold">{x.name}</span>
                <span className="mt-0.5 block text-[12px] leading-snug text-ink/55">{x.line}</span>
              </button>
            )
          })}
        </div>

        <AnimatePresence mode="wait" initial={false}>
          {d && (
            <motion.section
              key={d.id}
              initial={{ opacity: 0, y: -4 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.16 }}
              ref={(el) => {
                // Brought into view once each time a place is chosen.
                if (el && shownFor.current !== d.id) {
                  shownFor.current = d.id
                  el.scrollIntoView({ behavior: 'smooth', block: 'nearest' })
                }
              }}
              className="glass mt-4 scroll-mb-28 rounded-[20px] p-5"
            >
              {d.id === 'backup' ? (
                <Backup root={root} count={props.playlists.length} />
              ) : !names.length ? (
                <p className="text-[13.5px] text-ink/65">Choose one or more playlists on the left first.</p>
              ) : d.one && !one ? (
                <p className="text-[13.5px] text-ink/65">Choose one playlist for {d.name}: it goes one playlist at a time.</p>
              ) : d.id === 'file' ? (
                <ToFile root={root} names={names} />
              ) : d.id === 'text' ? (
                <ToText root={root} name={one!} />
              ) : d.id === 'others' ? (
                <ToOthers root={root} name={one!} />
              ) : d.id === 'youtube' ? (
                <ToYouTube key={one} root={root} name={one!} />
              ) : (
                <ToService key={`${d.id}${one}`} root={root} name={one!} service={d.id === 'spotify' ? 'spotify' : d.id === 'apple' ? 'apple_music' : 'deezer'} />
              )}
            </motion.section>
          )}
        </AnimatePresence>
      </div>
    </div>
  )
}

function Steps(props: { steps: React.ReactNode[] }) {
  return (
    <ol className="grid gap-2 text-[13px] leading-relaxed text-ink/75">
      {props.steps.map((t, i) => (
        <li key={i} className="flex gap-3">
          <span className="mt-[3px] grid h-5 w-5 shrink-0 place-items-center rounded-full bg-ink/[0.08] text-[11px] font-semibold text-ink/60">{i + 1}</span>
          <span>{t}</span>
        </li>
      ))}
    </ol>
  )
}

function Working(props: { what: string; done: number; total: number }) {
  return (
    <div role="status" className="max-w-[460px]">
      <div className="relative h-1.5 overflow-hidden rounded-full bg-ink/[0.08]">
        {props.total ? (
          <div className="absolute inset-y-0 left-0 rounded-full bg-accent transition-[width]" style={{ width: `${Math.round((props.done / props.total) * 100)}%` }} />
        ) : (
          <div className="bar-sweep absolute inset-y-0 w-1/3 rounded-full bg-accent" />
        )}
      </div>
      <p className="mt-3 text-[13px] text-ink/65">
        {props.what}
        {props.total ? ` ${props.done.toLocaleString('en-IN')} of ${props.total.toLocaleString('en-IN')}…` : '…'}
      </p>
    </div>
  )
}

function ToFile(props: { root: string; names: string[] }) {
  const [format, setFormat] = useState<ExportFormat>('m3u8')
  const [style, setStyle] = useState<PathStyle['kind']>('this_pc')
  const [prefix, setPrefix] = useState('/music')
  const [missing, setMissing] = useState(true)
  const [busy, setBusy] = useState(false)
  const [saved, setSaved] = useState<string[]>([])
  const f = FORMATS.find((x) => x.value === format)!
  const paths = pathFormats.includes(format)
  const several = props.names.length > 1

  async function save() {
    const to = several
      ? await openDialog({ directory: true, title: 'Choose a folder for the playlists' })
      : await saveDialog({ defaultPath: `${props.names[0]}.${f.ext}`, filters: [{ name: f.label.split(':')[0], extensions: [f.ext] }] })
    if (!to || Array.isArray(to)) return
    setBusy(true)
    try {
      const pathStyle: PathStyle = style === 'starts_with' ? { kind: 'starts_with', prefix } : { kind: style }
      const files = await playlistsExport(props.root, props.names, to, { format, style: pathStyle, include_missing: missing && keepsMissing.includes(format) })
      setSaved(files)
      notify(`Saved ${plural(files.length, 'playlist')}.`)
    } catch (e) {
      notify(`Couldn't save: ${why(e)}`)
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="grid gap-4">
      <label className="grid gap-1.5 text-[13px]">
        <span className="font-medium text-ink/75">Format</span>
        <select value={format} onChange={(e) => setFormat(e.target.value as ExportFormat)} className="field max-w-[420px]">
          {FORMATS.map((x) => (
            <option key={x.value} value={x.value}>
              {x.label}
            </option>
          ))}
        </select>
      </label>
      {paths && (
        <fieldset className="grid gap-1.5 text-[13px]">
          <legend className="mb-1.5 font-medium text-ink/75">For</legend>
          {STYLES.map((s) => (
            <label key={s.value} className="flex cursor-pointer items-start gap-2.5 rounded-xl px-2 py-1.5 hover:bg-ink/[0.04]">
              <input type="radio" name="style" checked={style === s.value} onChange={() => setStyle(s.value)} className="mt-1 accent-[var(--accent)]" />
              <span>
                <span className="block">{s.label}</span>
                <span className="block text-[12px] text-ink/55">{s.line}</span>
                {s.value === 'starts_with' && style === 'starts_with' && (
                  <input value={prefix} onChange={(e) => setPrefix(e.target.value)} aria-label="Where the server's music starts" className="field mt-1.5 w-[260px]" />
                )}
              </span>
            </label>
          ))}
        </fieldset>
      )}
      {keepsMissing.includes(format) && (
        <label className="flex items-center gap-3 text-[13px]">
          <Switch on={missing} onChange={setMissing} label="Include songs you don't have" />
          <span>
            Include songs you don't have
            <span className="block text-[12px] text-ink/50">They keep their places; Venuta reads them back, other players skip them.</span>
          </span>
        </label>
      )}
      <div className="flex flex-wrap items-center gap-3">
        <button onClick={() => void save()} disabled={busy} className="btn-primary">
          {busy ? 'Saving…' : several ? `Save ${plural(props.names.length, 'playlist')} to a folder…` : 'Save…'}
        </button>
        {saved.length > 0 && <span className="text-[12.5px] text-ink/55">Saved {saved.length === 1 ? saved[0] : `${saved.length} files`}.</span>}
      </div>
    </div>
  )
}

function Backup(props: { root: string; count: number }) {
  const [busy, setBusy] = useState(false)
  async function save() {
    const day = new Date().toISOString().slice(0, 10)
    const to = await saveDialog({ defaultPath: `Venuta playlists ${day}.zip`, filters: [{ name: 'Venuta backup', extensions: ['zip'] }] })
    if (!to) return
    setBusy(true)
    try {
      const n = await playlistsBackup(props.root, to)
      notify(`Backed up ${plural(n, 'playlist')}.`)
    } catch (e) {
      notify(`Couldn't back up: ${why(e)}`)
    } finally {
      setBusy(false)
    }
  }
  return (
    <div className="grid gap-3 text-[13.5px] leading-relaxed text-ink/75">
      <p>
        Every playlist ({props.count}), with the songs you don't have and online songs in their places, your To get list and your smart playlists' rules, in
        one file. To put them back, bring the file in on the Bring in side.
      </p>
      <div>
        <button onClick={() => void save()} disabled={busy} className="btn-primary">
          {busy ? 'Saving…' : 'Save a backup…'}
        </button>
      </div>
    </div>
  )
}

function ToText(props: { root: string; name: string }) {
  async function copy() {
    try {
      const text = await playlistText(props.root, props.name, 'txt')
      await navigator.clipboard.writeText(text)
      notify(`Copied ${plural(text.split('\n').filter(Boolean).length, 'song')}.`)
    } catch (e) {
      notify(`Couldn't copy: ${why(e)}`)
    }
  }
  return (
    <div className="grid gap-3 text-[13.5px] text-ink/75">
      <p>“{props.name}” as Artist - Title, one song a line: for a message, a note, or a site that takes a list of songs.</p>
      <div>
        <button onClick={() => void copy()} className="btn-primary">
          Copy the songs
        </button>
      </div>
    </div>
  )
}

function ToOthers(props: { root: string; name: string }) {
  async function save() {
    const to = await saveDialog({ defaultPath: `${props.name}.csv`, filters: [{ name: 'CSV', extensions: ['csv'] }] })
    if (!to) return
    try {
      await playlistsExport(props.root, [props.name], to, { format: 'csv', style: { kind: 'this_pc' }, include_missing: true })
      notify('Saved. Now upload it on the site.')
    } catch (e) {
      notify(`Couldn't save: ${why(e)}`)
    }
  }
  const site = (url: string, label: string) => (
    <button onClick={() => void openSendLink(url).catch((e) => notify(why(e)))} className="font-medium text-accent-ink underline-offset-4 hover:underline">
      {label}
    </button>
  )
  return (
    <div className="grid gap-4">
      <Steps
        steps={[
          <>Save “{props.name}” as a CSV file (below).</>,
          <>
            Open {site('https://www.tunemymusic.com/transfer', 'TuneMyMusic')} (500 songs free) or {site('https://soundiiz.com/', 'Soundiiz')} (200 songs a
            playlist free), choose to bring in from a file, and pick the CSV.
          </>,
          <>Choose TIDAL, Amazon Music or another service as where it goes. For JioSaavn, {site('https://freeyourmusic.com/', 'FreeYourMusic')} has 600 songs free.</>,
        ]}
      />
      <div>
        <button onClick={() => void save()} className="btn-primary">
          Save as CSV…
        </button>
      </div>
    </div>
  )
}

function ToYouTube(props: { root: string; name: string }) {
  const [busy, setBusy] = useState<{ done: number; total: number } | null>(null)
  const [out, setOut] = useState<YouTubeLinks | null>(null)
  async function find() {
    setBusy({ done: 0, total: 0 })
    try {
      setOut(await playlistYoutubeLinks(props.root, props.name, (done, total) => setBusy({ done, total })))
    } catch (e) {
      notify(`Couldn't find the songs: ${why(e)}`)
    } finally {
      setBusy(null)
    }
  }
  if (busy) return <Working what="Finding the songs on YouTube Music" {...busy} />
  if (!out)
    return (
      <div className="grid gap-4">
        <p className="text-[13.5px] leading-relaxed text-ink/75">
          Venuta finds each song on YouTube Music and makes links that open them as one list (YouTube takes up to 50 songs a link). You save it on YouTube Music,
          signed in there. Free, nothing to set up.
        </p>
        <div>
          <button onClick={() => void find()} className="btn-primary">
            Find the songs on YouTube Music
          </button>
        </div>
      </div>
    )
  return (
    <div className="grid gap-4">
      <p className="text-[13.5px] text-ink/75">
        Found {out.found} of {plural(out.total, 'song')}
        {out.links.length > 1 ? `, in ${out.links.length} parts of up to 50` : ''}.
      </p>
      <div className="grid gap-1.5">
        {out.links.map((url, i) => (
          <div key={url} className="flex items-center gap-3 rounded-xl bg-ink/[0.04] px-3 py-2">
            <span className="flex-1 text-[13px]">
              {out.links.length > 1 ? `Part ${i + 1}: songs ${i * 50 + 1} to ${Math.min(out.found, (i + 1) * 50)}` : `All ${out.found} songs`}
            </span>
            <button onClick={() => void openSendLink(url).catch((e) => notify(why(e)))} className="btn btn-sm">
              Open on YouTube
            </button>
          </div>
        ))}
      </div>
      <Steps
        steps={[
          <>Open it: YouTube shows the songs as one list.</>,
          <>
            Open the list on <b>music.youtube.com</b>, signed in, and choose <b>Save</b>; give it a name.
          </>,
          ...(out.links.length > 1 ? [<>For each further part, add its songs to the same playlist (the list's menu, Add to playlist).</>] : []),
          <>From YouTube Music, Spotify's Import your music, Apple's Transfer Music and Deezer's Transfer bring it anywhere, free.</>,
        ]}
      />
    </div>
  )
}

function ToService(props: { root: string; name: string; service: SendService }) {
  const label = props.service === 'spotify' ? 'Spotify' : props.service === 'apple_music' ? 'Apple Music' : 'Deezer'
  const [busy, setBusy] = useState<{ done: number; total: number } | null>(null)
  const [out, setOut] = useState<OnServiceOut | null>(null)
  async function find() {
    setBusy({ done: 0, total: 0 })
    try {
      setOut(await playlistOnService(props.root, props.name, props.service, (done, total) => setBusy({ done, total })))
    } catch (e) {
      notify(`Couldn't look for the songs: ${why(e)}`)
    } finally {
      setBusy(null)
    }
  }
  const page = (
    <button onClick={() => out && void openSavedPage(out.page).catch((e) => notify(why(e)))} className="btn">
      Open the list of links
    </button>
  )
  const whole =
    props.service === 'spotify'
      ? 'For a long playlist, or songs Spotify’s links miss: send it to YouTube Music first, then in Spotify choose Your Library, Import your music.'
      : props.service === 'apple_music'
        ? 'The whole playlist at once: send it to YouTube Music first, then on music.apple.com open your account and choose Transfer Music.'
        : 'The whole playlist at once: send it to YouTube Music first, then in Deezer’s settings choose Transfer your favourites.'
  if (busy) return <Working what={`Finding the songs on ${label}`} {...busy} />
  if (!out)
    return (
      <div className="grid gap-4">
        <p className="text-[13.5px] leading-relaxed text-ink/75">
          {props.service === 'spotify'
            ? 'Venuta finds each song on Spotify and copies their links; you paste them into a playlist in Spotify’s app. Free, nothing to sign in to.'
            : `Venuta finds each song on ${label} and makes a page with a link to each, to add them there. Free, nothing to sign in to.`}
        </p>
        <div className="flex flex-wrap gap-2.5">
          <button onClick={() => void find()} className="btn-primary">
            Find the songs on {label}
          </button>
        </div>
        <p className="text-[12.5px] leading-relaxed text-ink/55">{whole}</p>
      </div>
    )
  const copy = async () => {
    await navigator.clipboard.writeText(out.paste)
    notify(`Copied ${plural(out.found, 'link')}. Paste them in Spotify.`)
  }
  return (
    <div className="grid gap-4">
      <p className="text-[13.5px] text-ink/75">
        Found {out.found} of {plural(out.total, 'song')} on {label}.{out.missing.length ? ` ${out.missing.length === 1 ? "1 wasn't" : `${out.missing.length} weren't`} found; the list shows which.` : ''}
      </p>
      {props.service === 'spotify' ? (
        <>
          <div className="flex flex-wrap gap-2.5">
            <button onClick={() => void copy()} disabled={!out.found} className="btn-primary">
              Copy {plural(out.found, 'link')}
            </button>
            {page}
          </div>
          <Steps
            steps={[
              <>In Spotify’s desktop app, make a playlist (or open one).</>,
              <>
                Click in its song list and press <b>Ctrl+V</b>: the songs go in, in order.
              </>,
            ]}
          />
        </>
      ) : (
        <>
          <div>{page}</div>
          <Steps steps={[<>Open the list: it opens in your browser.</>, <>Open each song and add it to your playlist on {label}.</>]} />
        </>
      )}
      <p className="text-[12.5px] leading-relaxed text-ink/55">{whole}</p>
    </div>
  )
}
