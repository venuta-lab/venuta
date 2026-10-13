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
import { resolvedTheme, setPrefs, usePrefs, type Prefs } from '../lib/prefs'
import { ACCENTS, STYLES, styleInfo, tuneAccent, type StyleInfo } from '../lib/themes'
import { Page as Frame, Segmented as Choice, Switch } from '../components/ui'

const spring = { type: 'spring', stiffness: 420, damping: 34 } as const

type Page = 'look' | 'folders' | 'online' | 'safety' | 'about'
const PAGES: { id: Page; label: string }[] = [
  { id: 'look', label: 'Look and feel' },
  { id: 'folders', label: 'Music folders' },
  { id: 'online', label: 'Online sources' },
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
    <Frame title="Settings" line="Set up once: how it looks, where your music lives, and where details come from.">
      <div className="flex items-start gap-6">
      <aside className="sticky top-[68px] w-[220px] shrink-0">
        <nav aria-label="Settings sections" className="grid gap-0.5">
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
      <motion.section
        key={page}
        initial={{ opacity: 0, y: 6 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.2 }}
        className="glass min-w-0 flex-1 rounded-[24px] px-7 py-6"
      >
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
        {page === 'safety' && <Safety />}
        {page === 'about' && <About />}
      </motion.section>
      </div>
    </Frame>
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
  return <Choice label={props.id} value={props.value} options={props.options} onChange={props.onChange} size="sm" />
}

function LookAndFeel() {
  const p = usePrefs()
  const set = (c: Partial<Prefs>) => setPrefs(c)
  const info = styleInfo(p.style)
  const light = resolvedTheme(p) === 'light'
  return (
    <div>
      <h2 className="title text-[22px]">Look and feel</h2>
      <p className="mt-1 text-[13px] text-ink/55">Changes apply as you choose them.</p>

      <div role="radiogroup" aria-label="Theme" className="mt-5 grid grid-cols-[repeat(auto-fill,minmax(200px,1fr))] gap-3">
        {STYLES.map((s) => (
          <ThemeCard key={s.id} style={s} on={p.style === s.id} onPick={() => set({ style: s.id })} />
        ))}
      </div>
      <p className="mt-3 text-[12.5px] leading-relaxed text-ink/55">{info.line}</p>

      {info.modes && (
        <Row title={p.style === 'bounty' ? 'Session' : 'Mode'} hint={p.style === 'bounty' ? 'Day is ink on paper; night is paper on ink.' : undefined}>
          <Segmented
            id="theme"
            value={p.theme}
            onChange={(theme) => set({ theme })}
            options={
              p.style === 'bounty'
                ? [
                    { value: 'light', label: 'Day' },
                    { value: 'dark', label: 'Night' },
                    { value: 'system', label: 'Match Windows' },
                  ]
                : [
                    { value: 'light', label: 'Light' },
                    { value: 'dark', label: 'Dark' },
                    { value: 'system', label: 'Match Windows' },
                  ]
            }
          />
        </Row>
      )}
      {p.style === 'venuta' && (
        <>
          <Row
            title="Surface"
            hint="Glass shows the album art through the window. Frosted is calm and near-opaque, like Windows itself. Solid is opaque and fastest. AMOLED is true black, for dark only."
          >
            <Segmented
              id="surface"
              value={p.surface}
              onChange={(surface) => set({ surface })}
              options={[
                { value: 'glass', label: 'Glass' },
                { value: 'frosted', label: 'Frosted' },
                { value: 'solid', label: 'Solid' },
                { value: 'amoled', label: 'AMOLED', disabled: light },
              ]}
            />
          </Row>
          <Row title="Accent colour" hint="From the album you're looking at or playing, or always the colour you pick below.">
            <Segmented
              id="accent"
              value={p.accentMode}
              onChange={(accentMode) => set({ accentMode })}
              options={[
                { value: 'album', label: 'From the album' },
                { value: 'fixed', label: 'Fixed' },
              ]}
            />
          </Row>
          <Row
            title={p.accentMode === 'fixed' ? 'Accent' : 'When a cover has no colour'}
            hint={p.accentMode === 'fixed' ? undefined : 'Black-and-white and grey covers use this one.'}
          >
            <div role="radiogroup" aria-label="Accent" className="flex flex-wrap gap-2">
              {ACCENTS.map((a) => {
                const on = p.accent === a.name
                // As it will show: dark-theme accents are softened the same way album colours are.
                const hex = light ? a.light : (tuneAccent(a.dark, false) ?? a.dark)
                return (
                  <button
                    key={a.name}
                    role="radio"
                    aria-checked={on}
                    aria-label={a.name}
                    title={a.name}
                    onClick={() => set({ accent: a.name })}
                    className="keep-round grid h-8 w-8 place-items-center rounded-full transition-transform hover:scale-110"
                    style={{ background: hex, boxShadow: on ? `0 0 0 2px var(--solid), 0 0 0 4px ${hex}` : 'inset 0 0 0 1px rgb(0 0 0 / 0.15)' }}
                  />
                )
              })}
            </div>
          </Row>
        </>
      )}
      {p.style !== 'bounty' && p.style !== 'skin99' && (
        <Row
          title="Album background"
          hint={p.style === 'venuta' ? 'The blurred artwork behind everything. Not shown with Solid or AMOLED.' : "The artwork behind everything, in this theme's colours."}
        >
          <Switch label="Album background" on={p.backdrop} onChange={(backdrop) => set({ backdrop })} />
        </Row>
      )}
      <Row title="Motion" hint="Reduced keeps fades and drops springs, slides, the drifting background and the themes' moving effects.">
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

/** A theme as a small live window: its rail, title, covers and player, in its own tokens. */
function ThemeCard(props: { style: StyleInfo; on: boolean; onPick: () => void }) {
  const s = props.style
  return (
    <button role="radio" aria-checked={props.on} onClick={props.onPick} className="group text-left transition-transform duration-200 hover:-translate-y-0.5">
      <div
        data-preview={s.id}
        className={`keep-round relative aspect-[16/10] overflow-hidden rounded-xl transition-shadow ${
          props.on ? 'shadow-[0_0_0_2px_var(--color-accent)]' : 'shadow-[0_0_0_1px_var(--edge)] group-hover:shadow-[0_0_0_1px_var(--edge-strong)]'
        }`}
        style={{ backgroundColor: 'var(--app-bg)', backgroundImage: 'var(--app-image)' }}
      >
        {s.id === 'venuta' && (
          <div
            aria-hidden
            className="absolute inset-0"
            style={{
              background:
                'radial-gradient(70% 80% at 20% 20%, #c0602e 0, transparent 60%), radial-gradient(60% 70% at 85% 70%, #2d4f8a 0, transparent 65%), #14121a',
              filter: 'blur(8px) saturate(1.3)',
              opacity: 0.85,
            }}
          />
        )}
        <div className="rail absolute bottom-1.5 left-1.5 top-1.5 flex w-[14%] flex-col items-center gap-1.5 pt-2" style={{ borderRadius: 'min(var(--r-rail), 8px)' }}>
          <span className="rail-pill h-[13%] w-[70%]" style={{ borderRadius: 'min(var(--r-rail), 6px)' }} />
          <span className="keep-round h-1 w-[45%] rounded-full bg-ink/25" />
          <span className="keep-round h-1 w-[45%] rounded-full bg-ink/25" />
        </div>
        <div className="absolute left-[21%] right-2 top-2">
          <div className="title truncate text-[15px] leading-tight text-ink">Music</div>
          <div className="mt-1.5 flex gap-1.5">
            {[0, 1, 2].map((i) => (
              <div
                key={i}
                className="aspect-square flex-1"
                style={{
                  borderRadius: 'min(var(--r-np), 6px)',
                  background: i === 0 ? 'color-mix(in oklab, var(--accent) 55%, var(--glass))' : 'var(--glass)',
                  boxShadow: 'inset 0 0 0 1px var(--edge)',
                }}
              />
            ))}
          </div>
        </div>
        <div className="np absolute bottom-1.5 left-[21%] right-1.5 flex h-[19%] items-center gap-1.5 px-2" style={{ borderRadius: 'min(var(--r-np), 8px)' }}>
          <span className="keep-round h-[55%] shrink-0 rounded-full bg-accent" style={{ aspectRatio: '1' }} />
          <span className="keep-round h-[3px] flex-1 rounded-full bg-ink/20">
            <span className="keep-round block h-full w-2/5 rounded-full bg-accent" />
          </span>
        </div>
      </div>
      <div className="mt-2 flex items-baseline gap-2 px-0.5">
        <span className={`text-[13.5px] font-medium ${props.on ? 'text-ink' : 'text-ink/80'}`}>{s.name}</span>
        {props.on && <span className="text-[11.5px] text-accent-ink">In use</span>}
      </div>
    </button>
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
          <Switch
            label={title}
            on={s?.sources?.[key] ?? true}
            onChange={(on) =>
              save({
                sources: {
                  musicbrainz: s?.sources?.musicbrainz ?? true,
                  apple: s?.sources?.apple ?? true,
                  deezer: s?.sources?.deezer ?? true,
                  lrclib: s?.sources?.lrclib ?? true,
                  [key]: on,
                },
              })
            }
          />
        </Row>
      ))}
      <h3 className="mt-8 text-[15px] font-semibold">Optional keys</h3>
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

function Safety() {
  const { s, save, saved } = useSettings()
  return (
    <div>
      <h2 className="text-[22px] font-semibold tracking-[-0.02em]">Safety and undo</h2>
      <Row
        title="I have a backup of my music"
        hint="Editing tags or ratings changes your music files (safely, and each change can be undone). This confirms you have a copy elsewhere."
      >
        <Switch label="I have a backup of my music" on={!!s?.backup_confirmed} onChange={(on) => save({ backup_confirmed: on })} />
      </Row>
      <Row title="Nothing is ever deleted" hint="Duplicates and removed playlists go to the Set aside folder beside your music, and every change is recorded in Activity, where it can be undone.">
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

