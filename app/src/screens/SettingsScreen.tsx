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
import { resolvedTheme, setPrefs, surfaceShown, usePrefs, type Prefs } from '../lib/prefs'
import { ACCENTS, STYLES, accentRoles, colourDistance, styleInfo, type StyleInfo } from '../lib/themes'
import { Page as Frame, Segmented as Choice, Switch } from '../components/ui'
import { checkForUpdate, installUpdate, remember, savedUpdates, useUpdates } from '../lib/updates'
import { SpotifyAccount } from '../components/SpotifyAccount'
import { Brand } from '../components/Brand'
import { APP_ICONS, DEFAULT_ICON, chooseAppIcon, iconSrc, useAppIcon } from '../lib/appIcons'
import { takeSettingsSection, type SettingsPage } from './settingsPage'

const spring = { type: 'spring', stiffness: 420, damping: 34 } as const

type Page = SettingsPage
const PAGES: { id: Page; label: string }[] = [
  { id: 'look', label: 'Look and feel' },
  { id: 'desktop', label: 'Startup and tray' },
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
  const [page, setPage] = useState<Page>(() => {
    // Opened to a section from elsewhere (the update notice opens About).
    const p = takeSettingsSection() ?? props.page ?? 'look'
    return p
  })
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
                  // Lifted like the rail's chosen place, tinted with the accent.
                  style={{
                    background: 'color-mix(in oklab, var(--accent) 24%, color-mix(in oklab, var(--ink) 5%, transparent))',
                    boxShadow: 'var(--pill-shadow)',
                  }}
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
        {page === 'desktop' && <Desktop />}
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

/**
 * Lumen's colour: from the music, one colour you keep (a swatch or your own), or none; how
 * colourful; and a preview with the colours as they show now.
 */
function ColourRows() {
  const p = usePrefs()
  const set = (c: Partial<Prefs>) => setPrefs(c)
  const light = resolvedTheme(p) === 'light'
  const own = p.accent.startsWith('#') ? p.accent : null
  const fixed = p.accentMode === 'fixed'
  // Your own colour, as it shows: moved in lightness only when it wouldn't read.
  const shown = own ? accentRoles(own, light, p.intensity)?.accent : null
  const adjusted = own && shown && colourDistance(own, shown) > 0.08
  return (
    <>
      <Row
        title="Colour"
        hint={
          p.accentMode === 'album'
            ? 'Each album brings its own colour: the one playing, or the one you open. Covers without colour use the one below.'
            : fixed
              ? 'Always this colour, whatever plays.'
              : 'No accent: white controls on dark, graphite on light. The album art is the only colour.'
        }
      >
        <Segmented
          id="colour"
          value={p.accentMode}
          onChange={(accentMode) => set({ accentMode })}
          options={[
            { value: 'album', label: 'From the music' },
            { value: 'fixed', label: 'One colour' },
            { value: 'neutral', label: 'Neutral' },
          ]}
        />
      </Row>
      {p.accentMode !== 'neutral' && (
        <>
          <Row title="Intensity" hint="How colourful the accent may be. Every setting stays readable: buttons, text and the progress bar keep their contrast.">
            <Segmented
              id="intensity"
              value={p.intensity}
              onChange={(intensity) => set({ intensity })}
              options={[
                { value: 'subtle', label: 'Subtle' },
                { value: 'balanced', label: 'Balanced' },
                { value: 'vivid', label: 'Vivid' },
              ]}
            />
          </Row>
          <Row
            title={fixed ? 'Your colour' : 'For covers without colour'}
            hint={
              adjusted
                ? `Made a little ${light ? 'darker' : 'lighter'} so it reads on the ${light ? 'page' : 'glass'}.`
                : fixed
                  ? 'One of these, or any colour you pick.'
                  : 'Black-and-white and grey covers use this one.'
            }
          >
            <div role="radiogroup" aria-label="Colour" className="flex flex-wrap items-center gap-2">
              {ACCENTS.map((a) => {
                const on = !own && p.accent === a.name
                // As it will show on this theme.
                const hex = accentRoles(a.seed, light, p.intensity)?.accent ?? a.seed
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
              <label
                title="Your own colour"
                className="keep-round relative grid h-8 w-8 cursor-pointer place-items-center overflow-hidden rounded-full transition-transform hover:scale-110"
                style={{
                  background: shown ?? 'conic-gradient(#f0624a, #f2a33a, #d2c23a, #2fbf7f, #20b5c4, #3d8bff, #b45be6, #ec4f8c, #f0624a)',
                  boxShadow: own ? `0 0 0 2px var(--solid), 0 0 0 4px ${shown}` : 'inset 0 0 0 1px rgb(0 0 0 / 0.15)',
                }}
              >
                <input
                  type="color"
                  aria-label="Your own colour"
                  value={own ?? '#3d8bff'}
                  onChange={(e) => set({ accent: e.target.value })}
                  className="absolute inset-0 cursor-pointer opacity-0"
                />
                {!own && (
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="#fff" strokeWidth="2.6" strokeLinecap="round" aria-hidden>
                    <path d="M12 5v14M5 12h14" />
                  </svg>
                )}
              </label>
            </div>
          </Row>
        </>
      )}
      <Row title="Preview" hint={p.accentMode === 'album' ? "With the colour of what's playing now." : undefined}>
        <AccentPreview />
      </Row>
    </>
  )
}

/** The accent where it shows: the main button, progress, the playing song, a choice. */
function AccentPreview() {
  return (
    <div className="glass flex flex-wrap items-center gap-4 rounded-2xl px-4 py-3" aria-hidden>
      <span className="btn-primary btn-sm">
        <svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor">
          <path d="M7 4.5v15l13-7.5z" />
        </svg>
        Play
      </span>
      <span className="h-1 w-24 overflow-hidden rounded-full bg-ink/15">
        <span className="block h-full w-3/5 rounded-full bg-accent" />
      </span>
      <span className="flex items-center gap-2 text-[13px] font-medium text-accent-ink">
        <span className="flex h-3 items-end gap-[2px]">
          <span className="h-full w-[3px] rounded-sm bg-current" />
          <span className="h-2/3 w-[3px] rounded-sm bg-current" />
          <span className="h-1/3 w-[3px] rounded-sm bg-current" />
        </span>
        The song playing
      </span>
      <span className="rounded-full bg-[color-mix(in_oklab,var(--accent)_18%,transparent)] px-2.5 py-1 text-[12px] font-medium text-accent-ink">Chosen</span>
    </div>
  )
}

/** Which of Venuta's icons is on the taskbar, the tray, the windows and the shortcuts. */
function AppIconPicker() {
  const id = useAppIcon()
  const [error, setError] = useState('')
  const pick = (next: string) => {
    setError('')
    chooseAppIcon(next).catch((e) => setError(`Couldn't change the icon: ${String(e)}`))
  }
  const name = APP_ICONS.find((i) => i.id === id)?.name ?? ''
  return (
    <div className="border-b border-edge py-5">
      <div className="flex items-baseline gap-3">
        <div className="text-[14px] font-medium">App icon</div>
        <div className="text-[12.5px] text-ink/55">{name}</div>
      </div>
      <div className="mt-0.5 max-w-[70ch] text-[12.5px] leading-relaxed text-ink/55">
        On the taskbar, the tray, the windows and the shortcuts that open Venuta. The program file itself keeps Dusk glass.
      </div>
      <div role="radiogroup" aria-label="App icon" className="mt-4 grid grid-cols-[repeat(auto-fill,minmax(68px,1fr))] gap-2.5">
        {APP_ICONS.map((icon) => {
          const on = icon.id === id
          return (
            <button
              key={icon.id}
              role="radio"
              aria-checked={on}
              aria-label={icon.name}
              title={icon.id === DEFAULT_ICON ? `${icon.name} (the default)` : icon.name}
              onClick={() => pick(icon.id)}
              className="group relative aspect-square rounded-[18px] p-1 transition-transform duration-150 hover:-translate-y-0.5"
              style={{ boxShadow: on ? '0 0 0 2.5px var(--accent)' : undefined }}
            >
              <img src={iconSrc(icon.id)} alt="" loading="lazy" className="h-full w-full rounded-[14px] object-cover shadow-md shadow-black/30" draggable={false} />
              {icon.id === DEFAULT_ICON && (
                <span className="absolute -bottom-1 left-1/2 -translate-x-1/2 rounded-full bg-[var(--glass)] px-1.5 text-[10px] font-medium text-ink/70 shadow-[inset_0_0_0_1px_var(--edge)]">
                  Default
                </span>
              )}
            </button>
          )
        })}
      </div>
      {error && <p className="mt-2 text-[12.5px] text-problem">{error}</p>}
    </div>
  )
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

      <AppIconPicker />

      {info.modes && (
        <Row title={info.modeNames?.title ?? 'Mode'} hint={info.modeNames?.hint}>
          <Segmented
            id="theme"
            value={p.theme}
            onChange={(theme) => set({ theme })}
            options={
              info.modeNames
                ? [
                    { value: 'light', label: info.modeNames.light },
                    { value: 'dark', label: info.modeNames.dark },
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
      {info.surfaces && (
        <>
          <Row
            title="Surface"
            hint={
              surfaceShown(p) !== p.surface
                ? 'Windows has transparency effects turned off, so Lumen is Solid for now. Your choice comes back when they are on again (Windows Settings, Personalisation, Colours).'
                : 'Glass shows the album art through the window. Frosted is calm and near-opaque, like Windows itself. Solid is opaque and fastest. AMOLED is true black, for dark only.'
            }
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
        </>
      )}
      {info.accent === 'yours' && <ColourRows />}
      {info.albumBackground && (
        <Row
          title="Album background"
          hint={info.surfaces ? 'The blurred artwork behind everything. Not shown with Solid or AMOLED.' : "The artwork behind everything, in this theme's colours."}
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
      {/* The ring is the app's own accent, outside the preview (which has the theme's). */}
      <div
        className={`keep-round rounded-xl transition-shadow ${
          props.on ? 'shadow-[0_0_0_2px_var(--color-accent)]' : 'shadow-[0_0_0_1px_var(--edge)] group-hover:shadow-[0_0_0_1px_var(--edge-strong)]'
        }`}
      >
      <div
        data-preview={s.id}
        className="keep-round relative aspect-[16/10] overflow-hidden rounded-xl"
        style={{ backgroundColor: 'var(--app-bg)', backgroundImage: 'var(--app-image)' }}
      >
        {s.accent === 'yours' && (
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
    await setSettings(change)
    setS({ ...s, ...change })
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
            <button onClick={props.onOpenLibrary} className="btn">
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
            className="btn"
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
        <button onClick={props.onChooseLibrary} className="btn">
          Choose a folder
        </button>
      </Row>
      <Row title="Organising rules" hint={(s?.rules_dir ?? '') + '. Artist names, categories, folder layout and genres, as plain text files.'}>
        <button
          onClick={async () => {
            const p = await pick('Folder for your organising rules')
            if (p) await save({ rules_dir: p })
          }}
          className="btn"
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
          className="btn-primary"
        >
          Save
        </button>
        {props.value && (
          <button onClick={() => props.onSave(null)} className="btn-ghost">
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
        below are optional and add more. They are kept in your library's own settings (its .venuta folder), on this
        computer, and are never sent anywhere but the service they belong to.
      </p>
      <h3 className="mt-6 text-[15px] font-semibold">Accounts</h3>
      <SpotifyAccount />
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

/** What closing the window does, and starting with Windows. */
function Desktop() {
  const { s, save, saved } = useSettings()
  const [startup, setStartup] = useState<boolean | null>(null)
  useEffect(() => {
    if (!inApp()) return
    import('@tauri-apps/plugin-autostart')
      .then((a) => a.isEnabled())
      .then(setStartup)
      .catch(() => setStartup(false))
  }, [])
  async function setStartWithWindows(on: boolean) {
    const a = await import('@tauri-apps/plugin-autostart')
    await (on ? a.enable() : a.disable())
    setStartup(await a.isEnabled())
  }
  return (
    <div>
      <h2 className="title text-[22px]">Startup and tray</h2>
      <Row
        title="When you close the window"
        hint={
          s && !s.close_action
            ? 'Not chosen yet: Venuta asks the first time you close it.'
            : 'In the tray, Venuta keeps playing and answers your media keys. Click its icon by the clock to come back; right-click it for play, next and quit.'
        }
      >
        <Segmented
          id="close"
          value={(s?.close_action ?? '') as 'tray' | 'quit'}
          onChange={(close_action) => save({ close_action }, close_action === 'tray' ? 'Closing keeps music playing in the tray.' : 'Closing quits Venuta.')}
          options={[
            { value: 'tray', label: 'Keep playing in the tray' },
            { value: 'quit', label: 'Quit Venuta' },
          ]}
        />
      </Row>
      <Row title="Start with Windows" hint="When you sign in, Venuta starts in the tray without opening a window, ready for your media keys.">
        <Switch label="Start with Windows" on={!!startup} disabled={startup === null} onChange={(on) => void setStartWithWindows(on)} />
      </Row>
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
  const u = useUpdates()
  const [auto, setAuto] = useState(savedUpdates().auto)
  const line =
    u.at === 'checking'
      ? 'Checking…'
      : u.at === 'current'
        ? 'Venuta is up to date.'
        : u.at === 'found'
          ? `Venuta ${u.info.version} is ready to install (you have ${u.info.current}).`
          : u.at === 'downloading'
            ? `Downloading Venuta ${u.info.version}…`
            : u.at === 'failed'
              ? u.message
              : 'New versions are signed, and installed only when you choose.'
  return (
    <div className="select-text">
      <h2 className="sr-only">About</h2>
      <div className="flex flex-col items-start gap-4 py-2">
        <Brand size={96} word={62} />
        <p className="text-[13.5px] text-ink/70">
          The essence of the flute. Version {v}, free and open source under the MIT licence.
        </p>
      </div>
      <Row title="Updates" hint={line}>
        {u.at === 'found' ? (
          <button onClick={() => void installUpdate()} className="btn-primary">
            Install and restart
          </button>
        ) : (
          <button onClick={() => void checkForUpdate()} disabled={u.at === 'checking' || u.at === 'downloading'} className="btn">
            {u.at === 'checking' ? 'Checking…' : 'Check for updates'}
          </button>
        )}
      </Row>
      {u.at === 'downloading' && (
        <div className="-mt-1 mb-3 h-1.5 overflow-hidden rounded-full bg-ink/10" role="progressbar" aria-label="Downloading the update">
          <div className="h-full rounded-full bg-accent transition-[width]" style={{ width: u.total ? `${Math.round((u.done / u.total) * 100)}%` : '15%' }} />
        </div>
      )}
      {(u.at === 'found' || u.at === 'downloading') && u.info.notes && (
        <div className="mb-3 max-w-[66ch] whitespace-pre-line rounded-xl bg-ink/[0.05] px-4 py-3 text-[12.5px] leading-relaxed text-ink/70">{u.info.notes}</div>
      )}
      <Row title="Check when Venuta starts" hint="At most once a day, it asks GitHub whether there is a new version. Nothing else is sent.">
        <Switch
          label="Check for updates when Venuta starts"
          on={auto}
          onChange={(on) => {
            setAuto(on)
            remember({ auto: on })
          }}
        />
      </Row>
      <p className="mt-4 max-w-[62ch] text-[13px] leading-relaxed text-ink/55">
        Details come from MusicBrainz (CC0), Apple's iTunes Search API, the Deezer API and LRCLIB, each used within its
        published limits. Optional: AcoustID and Discogs. No account, no tracking, and the app never downloads music.
      </p>
    </div>
  )
}

