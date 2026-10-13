import { motion } from 'motion/react'
import { useEffect, useState } from 'react'
import { open } from '@tauri-apps/plugin-dialog'
import {
  checkHome,
  libraryHome,
  type HomeInfo,
  engineVersion,
  getSettings,
  inApp,
  setSettings,
  type Settings,
} from '../lib/engine'
import { ACCENTS, setPrefs, usePrefs, type Prefs } from '../lib/prefs'

const spring = { type: 'spring', stiffness: 420, damping: 34 } as const

type Page = 'look' | 'folders' | 'online' | 'players' | 'safety' | 'about'
const PAGES: { id: Page; label: string }[] = [
  { id: 'look', label: 'Look and feel' },
  { id: 'folders', label: 'Music folders' },
  { id: 'online', label: 'Online sources' },
  { id: 'players', label: 'Players' },
  { id: 'safety', label: 'Safety and undo' },
  { id: 'about', label: 'About' },
]

export function SettingsScreen(props: {
  root: string
  home: string | null
  music: string | null
  onChooseLibrary: () => void
  onOpenLibrary: () => void
  onChooseHome: (path: string) => Promise<void>
  page?: Page
}) {
  const [page, setPage] = useState<Page>(props.page ?? 'look')
  return (
    <div className="mx-auto flex h-full max-w-[1040px] gap-6 pb-8 pt-6">
      <aside className="w-[220px] shrink-0">
        <h1 className="text-[34px] font-semibold tracking-[-0.03em]">Settings</h1>
        <nav aria-label="Settings sections" className="mt-5 grid gap-0.5">
          {PAGES.map((p) => (
            <button
              key={p.id}
              onClick={() => setPage(p.id)}
              className={`relative rounded-xl px-3 py-2 text-left text-[13.5px] ${page === p.id ? 'text-ink' : 'text-ink/65 hover:text-ink'}`}
            >
              {page === p.id && (
                <motion.span
                  layoutId="settings-pill"
                  transition={spring}
                  className="absolute inset-0 rounded-xl"
                  style={{ background: 'color-mix(in oklab, var(--accent) 24%, color-mix(in oklab, var(--ink) 5%, transparent))' }}
                />
              )}
              <span className="relative">{p.label}</span>
            </button>
          ))}
        </nav>
      </aside>
      <main className="glass no-scrollbar min-w-0 flex-1 overflow-y-auto rounded-[24px] px-7 py-6">
        {page === 'look' && <LookAndFeel />}
        {page === 'folders' && (
          <Folders
            root={props.root}
            home={props.home}
            music={props.music}
            onChooseLibrary={props.onChooseLibrary}
            onOpenLibrary={props.onOpenLibrary}
            onChooseHome={props.onChooseHome}
          />
        )}
        {page === 'online' && <Online />}
        {page === 'players' && <Players />}
        {page === 'safety' && <Safety />}
        {page === 'about' && <About />}
      </main>
    </div>
  )
}

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

function Segmented<T extends string>(props: {
  id: string
  value: T
  options: { value: T; label: string; disabled?: boolean }[]
  onChange: (v: T) => void
}) {
  return (
    <div role="radiogroup" className="inline-flex rounded-full bg-ink/[0.07] p-1">
      {props.options.map((o) => {
        const on = o.value === props.value
        return (
          <button
            key={o.value}
            role="radio"
            aria-checked={on}
            disabled={o.disabled}
            onClick={() => props.onChange(o.value)}
            className={`relative rounded-full px-3.5 py-1.5 text-[12.5px] disabled:opacity-35 ${on ? 'text-ink' : 'text-ink/60 hover:text-ink'}`}
          >
            {on && (
              <motion.span
                layoutId={`seg-${props.id}`}
                transition={spring}
                className="absolute inset-0 rounded-full bg-ink/[0.14] shadow-sm"
              />
            )}
            <span className="relative">{o.label}</span>
          </button>
        )
      })}
    </div>
  )
}

function LookAndFeel() {
  const p = usePrefs()
  const set = (c: Partial<Prefs>) => setPrefs(c)
  return (
    <div>
      <h2 className="text-[22px] font-semibold tracking-[-0.02em]">Look and feel</h2>
      <p className="mt-1 text-[13px] text-ink/55">Changes apply as you choose them.</p>
      <Row title="Theme" hint="Light and dark both keep the glass.">
        <Segmented
          id="theme"
          value={p.theme}
          onChange={(theme) => set({ theme })}
          options={[
            { value: 'light', label: 'Light' },
            { value: 'dark', label: 'Dark' },
            { value: 'system', label: 'Match Windows' },
          ]}
        />
      </Row>
      <Row
        title="Surface"
        hint="Glass shows the desktop through the window. Frosted is softer. Solid is opaque and fastest. AMOLED is true black, for dark only."
      >
        <Segmented
          id="surface"
          value={p.surface}
          onChange={(surface) => set({ surface })}
          options={[
            { value: 'glass', label: 'Glass' },
            { value: 'frosted', label: 'Frosted' },
            { value: 'solid', label: 'Solid' },
            { value: 'amoled', label: 'AMOLED', disabled: p.theme === 'light' },
          ]}
        />
      </Row>
      <Row title="Accent colour" hint="From the album you are looking at, or one colour of your choice.">
        <Segmented
          id="accent"
          value={p.accentMode}
          onChange={(accentMode) => {
            set({ accentMode })
            if (accentMode === 'fixed') document.documentElement.style.setProperty('--accent', p.accent)
          }}
          options={[
            { value: 'album', label: 'From the album' },
            { value: 'fixed', label: 'Fixed' },
          ]}
        />
      </Row>
      <Row title="Fixed accent" hint="Used when the accent is fixed.">
        <div role="radiogroup" aria-label="Fixed accent" className="flex flex-wrap gap-2">
          {ACCENTS.map((a) => {
            const on = p.accent === a.hex
            return (
              <button
                key={a.hex}
                role="radio"
                aria-checked={on}
                aria-label={a.name}
                title={a.name}
                onClick={() => set({ accent: a.hex, accentMode: 'fixed' })}
                className="grid h-8 w-8 place-items-center rounded-full"
                style={{ background: a.hex, boxShadow: on ? '0 0 0 2px var(--app-bg), 0 0 0 4px ' + a.hex : 'none' }}
              />
            )
          })}
        </div>
      </Row>
      <Row title="Album background" hint="The blurred artwork behind everything. Not shown with Solid or AMOLED.">
        <Segmented
          id="backdrop"
          value={p.backdrop ? 'on' : 'off'}
          onChange={(v) => set({ backdrop: v === 'on' })}
          options={[
            { value: 'on', label: 'On' },
            { value: 'off', label: 'Off' },
          ]}
        />
      </Row>
      <Row title="Motion" hint="Reduced keeps fades and drops springs, slides and the drifting background.">
        <Segmented
          id="motion"
          value={p.motion}
          onChange={(motion) => set({ motion })}
          options={[
            { value: 'full', label: 'Full' },
            { value: 'reduced', label: 'Reduced' },
            { value: 'system', label: 'Match Windows' },
          ]}
        />
      </Row>
    </div>
  )
}

function useSettings() {
  const [s, setS] = useState<Settings | null>(null)
  const [saved, setSaved] = useState('')
  useEffect(() => {
    getSettings().then(setS)
  }, [])
  async function save(change: Partial<Settings>, message = 'Saved.') {
    if (!s) return
    const next = { ...s, ...change }
    await setSettings(next)
    setS(next)
    setSaved(message)
    window.setTimeout(() => setSaved(''), 2200)
  }
  return { s, save, saved }
}

function Folders(props: {
  root: string
  home: string | null
  music: string | null
  onChooseLibrary: () => void
  onOpenLibrary: () => void
  onChooseHome: (path: string) => Promise<void>
}) {
  const { s, save, saved } = useSettings()
  const [error, setError] = useState('')
  const [info, setInfo] = useState<HomeInfo | null>(null)
  useEffect(() => {
    void libraryHome().then(setInfo)
  }, [props.home])
  async function pick(title: string) {
    const picked = inApp() ? await open({ directory: true, title }) : null
    return typeof picked === 'string' ? picked : null
  }
  const norm = (p: string) => p.replace(/[\\/]+/g, '/').replace(/\/$/, '').toLowerCase()
  const showingLibrary = !!props.music && norm(props.root) === norm(props.music)
  return (
    <div>
      <h2 className="text-[22px] font-semibold tracking-[-0.02em]">Music folders</h2>
      <Row
        title="Your library"
        hint={
          props.home
            ? `${props.home}. Your albums are in its Music folder, with Playlists, Add music here and Set aside beside it; everything Venuta knows (settings, answers, history, covers) is kept with it in ${info?.data_dir ?? '.venuta'}. Move or back up the folder and it all goes along.`
            : 'Not chosen yet.'
        }
      >
        <div className="flex gap-2">
          {props.home && !showingLibrary && (
            <button onClick={props.onOpenLibrary} className="rounded-full bg-ink/[0.08] px-4 py-2 text-[13px] hover:bg-ink/[0.14]">
              Open
            </button>
          )}
          <button
            onClick={async () => {
              const p = await pick('Choose your library (a new, empty folder or one Venuta made)')
              if (!p) return
              try {
                await checkHome(p)
                await props.onChooseHome(p)
                setError('')
              } catch (e) {
                setError(e instanceof Error ? e.message : String(e))
              }
            }}
            className="rounded-full bg-ink/[0.08] px-4 py-2 text-[13px] hover:bg-ink/[0.14]"
          >
            {props.home ? 'Use another library' : 'Choose'}
          </button>
        </div>
      </Row>
      <Row
        title="Music to bring in"
        hint={
          !props.root || showingLibrary
            ? 'A folder of music you already have. It is only read, never changed; Venuta builds a clean copy in your library.'
            : `${props.root}. Only read, never changed; Organize builds a clean copy of it in your library.`
        }
      >
        <button onClick={props.onChooseLibrary} className="rounded-full bg-ink/[0.08] px-4 py-2 text-[13px] hover:bg-ink/[0.14]">
          Choose a folder
        </button>
      </Row>
      <Row title="Organising rules" hint={(s?.rules_dir ?? '') + '. Artist names, categories, folder layout and genres, as plain text files.'}>
        <button
          onClick={async () => {
            const p = await pick('Folder for your organising rules')
            if (p) await save({ rules_dir: p })
          }}
          className="rounded-full bg-ink/[0.08] px-4 py-2 text-[13px] hover:bg-ink/[0.14]"
        >
          Choose
        </button>
      </Row>
      {error && <p className="mt-3 text-[13px] text-problem">{error}</p>}
      {saved && <p className="mt-3 text-[13px] text-good">{saved}</p>}
    </div>
  )
}

function KeyField(props: {
  id: string
  title: string
  hint: string
  where: string
  value: string | null | undefined
  length?: number
  onSave: (v: string | null) => Promise<void>
}) {
  const [v, setV] = useState(props.value ?? '')
  useEffect(() => setV(props.value ?? ''), [props.value])
  const dirty = v.trim() !== (props.value ?? '')
  const valid = !props.length || v.trim().length === 0 || v.trim().length === props.length
  return (
    <div className="border-b border-edge py-4 last:border-b-0">
      <div className="flex flex-wrap items-baseline gap-2">
        <label htmlFor={props.id} className="text-[14px] font-medium">
          {props.title}
        </label>
        <span className={`text-[12px] ${props.value ? 'text-good' : 'text-ink/45'}`}>{props.value ? 'Added' : 'Not added'}</span>
      </div>
      <p className="mt-0.5 max-w-[62ch] text-[12.5px] leading-relaxed text-ink/55">{props.hint}</p>
      <p className="mt-1 select-text text-[12px] text-ink/45">Get one at {props.where}</p>
      <div className="mt-2.5 flex flex-wrap gap-2">
        <input
          id={props.id}
          type="password"
          autoComplete="off"
          spellCheck={false}
          value={v}
          onChange={(e) => setV(e.target.value)}
          placeholder="Paste the key"
          className="min-w-0 flex-1 rounded-xl bg-ink/[0.06] px-3.5 py-2 text-[13px] outline-none focus:ring-1 focus:ring-[var(--accent)]"
        />
        <button
          disabled={!dirty || !valid}
          onClick={() => props.onSave(v.trim() || null)}
          className="rounded-full bg-accent px-4 py-2 text-[13px] font-medium text-on-accent disabled:opacity-35"
        >
          Save
        </button>
        {props.value && (
          <button onClick={() => props.onSave(null)} className="rounded-full px-3 py-2 text-[13px] text-ink/65 hover:bg-ink/10">
            Remove
          </button>
        )}
      </div>
      {!valid && <p className="mt-1.5 text-[12px] text-problem">This key is {props.length} characters long.</p>}
    </div>
  )
}

function Online() {
  const { s, save, saved } = useSettings()
  return (
    <div>
      <h2 className="text-[22px] font-semibold tracking-[-0.02em]">Online sources</h2>
      <p className="mt-1 max-w-[62ch] text-[13px] leading-relaxed text-ink/55">
        Looking up details online uses MusicBrainz, Apple's catalogue, Deezer and LRCLIB, which need no key. The keys
        below are optional and add more. They stay on this computer and are never written next to your library.
      </p>
      {(
        [
          ['musicbrainz', 'MusicBrainz', 'Original years, composers and lyricists, release types. Free and open (CC0).'],
          ['apple', "Apple's catalogue", 'Sharper covers (up to 3000 px), genres and release dates.'],
          ['deezer', 'Deezer', 'Covers, labels and recording codes.'],
          ['lrclib', 'LRCLIB', 'Synced lyrics, checked for timing and script before use.'],
        ] as const
      ).map(([key, title, hint]) => (
        <Row key={key} title={title} hint={hint}>
          <Segmented
            id={`source-${key}`}
            value={(s?.sources?.[key] ?? true) ? 'on' : 'off'}
            onChange={(v) =>
              save({
                sources: {
                  musicbrainz: s?.sources?.musicbrainz ?? true,
                  apple: s?.sources?.apple ?? true,
                  deezer: s?.sources?.deezer ?? true,
                  lrclib: s?.sources?.lrclib ?? true,
                  [key]: v === 'on',
                },
              })
            }
            options={[
              { value: 'on', label: 'On' },
              { value: 'off', label: 'Off' },
            ]}
          />
        </Row>
      ))}
      <h3 className="mt-6 text-[15px] font-medium">Optional keys</h3>
      <KeyField
        id="acoustid"
        title="AcoustID"
        hint="Identifies files without tags by their sound. Free for open-source apps. Only an acoustic fingerprint is sent, never the audio."
        where="acoustid.org/new-application"
        value={s?.acoustid_key}
        onSave={(v) => save({ acoustid_key: v }, v ? 'AcoustID key saved.' : 'AcoustID key removed.')}
      />
      <KeyField
        id="discogs"
        title="Discogs"
        hint="Credits and original release years for albums MusicBrainz doesn't have, common for Indian releases. Free personal token."
        where="discogs.com/settings/developers"
        value={s?.discogs_token}
        onSave={(v) => save({ discogs_token: v }, v ? 'Discogs token saved.' : 'Discogs token removed.')}
      />
      <KeyField
        id="apple"
        title="Apple Music library key"
        hint="Reads your own Apple Music library to bring your playlists over. 16 characters; published in open-source Apple Music tools and not included with this app."
        where="open-source Apple Music library readers"
        value={s?.apple_key}
        length={16}
        onSave={(v) => save({ apple_key: v }, v ? 'Apple Music key saved.' : 'Apple Music key removed.')}
      />
      {saved && <p className="mt-3 text-[13px] text-good">{saved}</p>}
    </div>
  )
}

function Players() {
  return (
    <div>
      <h2 className="text-[22px] font-semibold tracking-[-0.02em]">Players</h2>
      <p className="mt-2 max-w-[62ch] text-[13.5px] leading-relaxed text-ink/65">
        Each card is written for the player that reads it: Poweramp, Snowsky Disc, Echo Mini or Astell&amp;Kern. The
        profiles and the player test are set up from Devices when you connect a card.
      </p>
    </div>
  )
}

function Safety() {
  const { s, save, saved } = useSettings()
  return (
    <div>
      <h2 className="text-[22px] font-semibold tracking-[-0.02em]">Safety and undo</h2>
      <Row
        title="I have a backup of my music"
        hint="Editing tags or ratings changes your music files (safely, and each change can be undone). This confirms you have a copy elsewhere."
      >
        <Segmented
          id="backup"
          value={s?.backup_confirmed ? 'yes' : 'no'}
          onChange={(v) => save({ backup_confirmed: v === 'yes' })}
          options={[
            { value: 'yes', label: 'Yes' },
            { value: 'no', label: 'Not yet' },
          ]}
        />
      </Row>
      <Row title="Nothing is ever deleted" hint="Duplicates and removed playlists are set aside in _Quarantine, and every change is recorded in Activity, where it can be undone." >
        <span />
      </Row>
      {saved && <p className="mt-3 text-[13px] text-good">{saved}</p>}
    </div>
  )
}

function About() {
  const [v, setV] = useState('')
  useEffect(() => {
    engineVersion().then(setV)
  }, [])
  return (
    <div className="select-text">
      <h2 className="text-[22px] font-semibold tracking-[-0.02em]">About</h2>
      <p className="mt-2 text-[13.5px] text-ink/70">Venuta (वेणुता) {v}. Free and open source under the MIT licence.</p>
      <p className="mt-3 max-w-[62ch] text-[13px] leading-relaxed text-ink/55">
        Details come from MusicBrainz (CC0), Apple's iTunes Search API, the Deezer API and LRCLIB, each used within its
        published limits. Optional: AcoustID and Discogs. No account, no tracking, and the app never downloads music.
      </p>
    </div>
  )
}

