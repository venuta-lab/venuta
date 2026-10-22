import { invoke } from '@tauri-apps/api/core'
import { AnimatePresence, motion } from 'motion/react'
import { useCallback, useEffect, useState } from 'react'
import { open } from '@tauri-apps/plugin-dialog'
import {
  checkHome,
  libraryHome,
  type HomeInfo,
  engineVersion,
  getSettings,
  inApp,
  onlineCatalogStatus,
  onlineEnsureStreamTool,
  type OnlineCatalogStatus,
  setSettings,
  type Settings,
} from '../lib/engine'
import { reducedMotion, resolvedTheme, setPrefs, surfaceShown, themeInfo, usePrefs, type Prefs } from '../lib/prefs'
import { ACCENTS, accentRoles, colourDistance } from '../lib/themes'
import { AppearanceChoice, Themes, useRemembered } from '../components/themes/ThemeBrowser'
import { Page as Frame, Segmented as Choice, Switch } from '../components/ui'
import { checkForUpdate, installUpdate, openDownloadPage, remember, savedUpdates, useUpdates } from '../lib/updates'
import { knownUpdate, statusLine } from '../lib/updateModel'
import { SpotifyAccount } from '../components/SpotifyAccount'
import { YtmAccount } from '../components/YtmAccount'
import { ProxySetting } from './settings/ProxySetting'
import { CacheSetting } from './settings/CacheSetting'
import { HotkeysSetting } from './settings/HotkeysSetting'
import { readZoom, setZoom, ZOOM_EVENT } from '../lib/zoom'
import { notify } from '../lib/notify'
import { Brand } from '../components/Brand'
import { APP_ICONS, DEFAULT_ICON, chooseAppIcon, iconSrc, useAppIcon } from '../lib/appIcons'
import { onSettingsSection, peekSettingsSection, takeSettingsSection, type SettingsPage } from './settingsPage'
import { ActivityPage } from './ActivityScreen'
import { HistoryRow, LocalOnlyRow, LyricsSources, RegionRow, RelayRow, TogetherRows } from './settings/ListeningSettings'
import { PlaybackSettings } from './settings/PlaybackSettings'
import { LyricWindowsSettings } from './settings/LyricWindowsSettings'
import { LyricLinesSettings } from './settings/LyricLinesSettings'
import { Group, More, PageHead, Row } from './settings/parts'

const spring = { type: 'spring', stiffness: 420, damping: 34 } as const

type Page = SettingsPage
// One subject a page: how it looks, what plays, where lyrics come from, where your music lives,
// what goes online, how Venuta runs on Windows, and (last) what it changed, each with Undo.
const PAGES: { id: Page; label: string; line: string }[] = [
  { id: 'look', label: 'Look and feel', line: 'Theme, colour, size' },
  { id: 'playback', label: 'Playback', line: 'Sound, what plays' },
  { id: 'lyrics', label: 'Lyrics', line: 'Sources and windows' },
  { id: 'library', label: 'Library', line: 'Folders and details' },
  { id: 'online', label: 'Online', line: 'Accounts and rooms' },
  { id: 'system', label: 'System', line: 'Windows, updates, about' },
  { id: 'activity', label: 'Activity', line: 'Changes and undo' },
]

export function SettingsScreen(props: {
  root: string
  home: string | null
  music: string | null
  onChooseLibrary: () => void
  onOpenLibrary: () => void
  onChooseHome: (path: string) => Promise<void>
  /** Now Playing's Sound (the equalizer and tone). */
  onOpenSound: () => void
  /** The sheet of every keyboard shortcut. */
  onShortcuts: () => void
  page?: Page
}) {
  const [page, setPage] = useState<Page>(() => {
    // Opened to a section from elsewhere (the update notice opens About).
    // Looked at, not used up: the page transition can render Settings twice before it shows.
    const p = peekSettingsSection() ?? props.page ?? 'look'
    return p
  })
  // Asked for while Settings is open (Ctrl+H, a change's "See all"): that page, at once. And
  // once it shows, the page it was opened on is used up.
  useEffect(() => {
    takeSettingsSection()
    return onSettingsSection(() => {
      const p = takeSettingsSection()
      if (p) setPage(p)
    })
  }, [])
  return (
    <Frame title="Settings" line="Set up once: how it looks, what plays, where your music lives.">
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
              <span className="relative block">{p.label}</span>
              <span className="relative block text-[11.5px] text-ink/45">{p.line}</span>
            </button>
          ))}
        </nav>
        <p className="mt-3 px-3 text-[12px] leading-relaxed text-ink/45">Ctrl+K finds any setting by its name.</p>
      </aside>
      <motion.section
        key={page}
        initial={{ opacity: 0, y: 6 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.2 }}
        className="glass min-w-0 flex-1 rounded-[24px] px-7 py-6"
      >
        {page === 'look' && <LookAndFeel />}
        {page === 'playback' && <PlaybackSettings onOpenSound={props.onOpenSound} />}
        {page === 'lyrics' && <LyricsPage />}
        {page === 'library' && (
          <LibraryPage
            root={props.root}
            home={props.home}
            music={props.music}
            onChooseLibrary={props.onChooseLibrary}
            onOpenLibrary={props.onOpenLibrary}
            onChooseHome={props.onChooseHome}
          />
        )}
        {page === 'online' && <OnlinePage />}
        {page === 'system' && <SystemPage onShortcuts={props.onShortcuts} />}
        {page === 'activity' && <ActivityPage root={props.root} />}
      </motion.section>
      </div>
    </Frame>
  )
}

/**
 * Updates: the state in words, when Venuta last heard from GitHub, and when something went wrong,
 * why, with Try again and the installer to download by hand.
 */
function UpdatesGroup(props: { version: string }) {
  const u = useUpdates()
  const [auto, setAuto] = useState(savedUpdates().auto)
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    const t = window.setInterval(() => setNow(Date.now()), 30_000)
    return () => window.clearInterval(t)
  }, [])
  const info = knownUpdate(u)
  const busy = u.at === 'checking' || u.at === 'downloading' || u.at === 'installing'
  const installFailed = u.at === 'failed' && !!info
  return (
    <Group title="Updates">
      <Row title="Updates" hint={statusLine(u, savedUpdates().last, now, props.version)}>
        <div className="flex flex-wrap items-center justify-end gap-2">
          {u.at === 'local_only' ? (
            <button onClick={() => void checkForUpdate({ manual: true, anyway: true })} className="btn">
              Check once anyway
            </button>
          ) : (
            <button onClick={() => void checkForUpdate({ manual: true })} disabled={busy} className="btn">
              {u.at === 'checking' ? 'Checking…' : u.at === 'failed' && !info ? 'Try again' : 'Check for updates'}
            </button>
          )}
          {info && (u.at === 'found' || u.at === 'failed' || (u.at === 'checking' && !!u.info)) && (
            <button onClick={() => void installUpdate()} disabled={busy} className="btn-primary">
              {installFailed ? 'Try again' : `Update to ${info.version}`}
            </button>
          )}
        </div>
      </Row>
      {u.at === 'failed' && (
        <p className="-mt-1 mb-3 max-w-[66ch] text-[12.5px] leading-relaxed text-ink/60">
          Or{' '}
          <button onClick={openDownloadPage} className="font-semibold text-accent-ink hover:underline">
            download the installer
          </button>{' '}
          from GitHub and run it. Your music, playlists and settings stay as they are.
        </p>
      )}
      {(u.at === 'downloading' || u.at === 'installing') && (
        <div className="-mt-1 mb-3 h-1.5 overflow-hidden rounded-full bg-ink/10" role="progressbar" aria-label="Downloading the update">
          <div
            className="h-full rounded-full bg-accent transition-[width]"
            style={{ width: u.at === 'installing' ? '100%' : u.total ? `${Math.round((u.done / u.total) * 100)}%` : '15%' }}
          />
        </div>
      )}
      {info?.notes && (u.at === 'found' || u.at === 'downloading' || u.at === 'installing' || u.at === 'failed') && (
        <div className="mb-3 max-w-[66ch] whitespace-pre-line rounded-xl bg-ink/[0.05] px-4 py-3 text-[12.5px] leading-relaxed text-ink/70">{info.notes}</div>
      )}
      <Row
        title="Check automatically"
        hint="Soon after Venuta starts and every few hours while it runs, it asks GitHub whether there is a new version. Nothing else is sent. Local only stops these checks."
      >
        <Switch
          label="Check for updates automatically"
          on={auto}
          onChange={(on) => {
            setAuto(on)
            remember({ auto: on })
          }}
        />
      </Row>
      <BetaRow />
    </Group>
  )
}

/** Pre-releases (beta): newer versions before they are released to everyone. */
function BetaRow() {
  const [beta, setBeta] = useState<boolean | null>(null)
  useEffect(() => {
    if (!inApp()) return
    void getSettings().then((s) => setBeta(s.update_channel === 'beta'))
  }, [])
  if (beta === null) return null
  return (
    <Row title="Pre-releases" hint="Also offer versions released early, to try what is new before everyone. Each is signed like any release, and installed only when you choose.">
      <Switch
        label="Offer pre-releases"
        on={beta}
        onChange={(on) => {
          setBeta(on)
          void setSettings({ update_channel: on ? 'beta' : 'stable' }).then(() => void checkForUpdate({ manual: true }))
        }}
      />
    </Row>
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
  return (
    <div>
      <div className="max-w-[70ch] text-[12.5px] leading-relaxed text-ink/55">
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

/**
 * Look and feel: light or dark beside the title, then every theme as a gallery to choose from,
 * then the rest as drawers that say what they're set to, so nothing needs opening to know it.
 */
function LookAndFeel() {
  const p = usePrefs()
  const set = (c: Partial<Prefs>) => setPrefs(c)
  const info = themeInfo(p)
  const light = resolvedTheme(p) === 'light'
  const surfaceName = { glass: 'Glass', frosted: 'Frosted', solid: 'Solid', amoled: 'AMOLED' }[surfaceShown(p)]
  const windowParts = [
    info.surfaces ? surfaceName : '',
    info.albumBackground ? (p.backdrop ? 'album background on' : 'album background off') : '',
  ].filter(Boolean)
  const windowSummary = windowParts.join(', ')
  const motionSummary =
    p.motion === 'system' ? `Match Windows (${reducedMotion(p) ? 'reduced' : 'full'} now)` : p.motion === 'full' ? 'Full' : 'Reduced'
  return (
    <div>
      <div className="flex min-h-[32px] flex-wrap items-center justify-between gap-x-4 gap-y-2">
        <h2 className="title text-[22px]">Look and feel</h2>
        <AppearanceChoice />
      </div>

      <div className="mt-5">
        <Themes />
      </div>

      <div className="mt-8">
        {info.accent === 'yours' && (
          <Drawer id="colour" title="Colour" summary={<ColourSummary />}>
            <ColourRows />
          </Drawer>
        )}
        {windowSummary && (
          <Drawer id="window" title="Window" summary={windowSummary.charAt(0).toUpperCase() + windowSummary.slice(1)}>
            {info.surfaces && (
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
            )}
            {info.albumBackground && (
              <Row
                title="Album background"
                hint={info.surfaces ? 'The blurred artwork behind everything. Not shown with Solid or AMOLED.' : "The artwork behind everything, in this theme's colours."}
              >
                <Switch label="Album background" on={p.backdrop} onChange={(backdrop) => set({ backdrop })} />
              </Row>
            )}
          </Drawer>
        )}
        <Drawer id="seek" title="Seek bar" summary={p.seekBar === 'theme' && info.seekBar !== 'wave' ? "The theme's own" : 'Waveform'}>
          <Row
            title="Seek bar"
            hint={
              info.seekBar === 'wave'
                ? 'This theme draws the waveform either way.'
                : "The song's waveform in this theme's colours, or the seek bar the theme was made with."
            }
          >
            <Segmented
              id="seek-bar"
              value={p.seekBar}
              onChange={(seekBar) => set({ seekBar })}
              options={[
                { value: 'wave', label: 'Waveform' },
                { value: 'theme', label: "The theme's own", disabled: info.seekBar === 'wave' },
              ]}
            />
          </Row>
        </Drawer>
        <Drawer id="size" title="Size" summary={<SizeSummary />}>
          <InterfaceSize />
        </Drawer>
        <Drawer id="motion" title="Motion" summary={motionSummary}>
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
        </Drawer>
        <Drawer id="icon" title="App icon" summary={<IconSummary />}>
          <AppIconPicker />
        </Drawer>
      </div>
    </div>
  )
}

/** The size the window draws at, kept current when Ctrl + or Ctrl - changes it. */
function useZoom() {
  const [zoom, setZoomShown] = useState(readZoom)
  useEffect(() => {
    const read = () => setZoomShown(readZoom())
    window.addEventListener(ZOOM_EVENT, read)
    return () => window.removeEventListener(ZOOM_EVENT, read)
  }, [])
  return zoom
}

const pct = (z: number) => `${Math.round(z * 100)}%`

function SizeSummary() {
  const zoom = useZoom()
  return <>{zoom === 1 ? '100%, as designed' : pct(zoom)}</>
}

function InterfaceSize() {
  const zoom = useZoom()
  const shown = [0.8, 0.9, 1, 1.1, 1.25, 1.5]
  const options = (shown.includes(zoom) ? shown : [...shown, zoom].sort((a, b) => a - b)).map((z) => ({ value: String(z), label: pct(z) }))
  return (
    <Row title="Interface size" hint="Text, covers and controls everywhere, in every theme. Ctrl + and Ctrl - change it from anywhere, Ctrl 0 goes back to 100%.">
      <Segmented id="ui-size" value={String(zoom)} onChange={(v) => setZoom(Number(v))} options={options} />
    </Row>
  )
}

/**
 * A part of Look and feel as one line saying what it is set to, opened to change it. Whether it
 * was left open is remembered.
 */
function Drawer(props: { id: string; title: string; summary: React.ReactNode; children: React.ReactNode }) {
  const [open, setOpen] = useRemembered(`look-open:${props.id}`, false)
  return (
    <div className="border-t border-edge last:border-b">
      <button aria-expanded={open} onClick={() => setOpen(!open)} className="group flex w-full items-center gap-4 py-3.5 text-left">
        <span className="w-[120px] shrink-0 text-[14px] font-medium">{props.title}</span>
        <span className="flex min-w-0 flex-1 items-center gap-2 truncate text-[12.5px] text-ink/60">{props.summary}</span>
        <svg
          width="14"
          height="14"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2.2"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden
          className={`shrink-0 text-ink/45 transition-transform duration-200 group-hover:text-ink ${open ? 'rotate-180' : ''}`}
        >
          <path d="m6 9 6 6 6-6" />
        </svg>
      </button>
      <AnimatePresence initial={false}>
        {open && (
          <motion.div
            key="body"
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: 'auto', opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{ duration: 0.24, ease: [0.2, 0.8, 0.2, 1] }}
            className="overflow-hidden"
          >
            <div className="pb-4">{props.children}</div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  )
}

/** Where the accent comes from, and how colourful: one line and the colour itself. */
function ColourSummary() {
  const p = usePrefs()
  const own = p.accent.startsWith('#')
  const what = p.accentMode === 'album' ? 'From the music' : p.accentMode === 'fixed' ? `One colour: ${own ? 'your own' : p.accent}` : 'Neutral'
  return (
    <>
      <span className="keep-round h-3.5 w-3.5 shrink-0 rounded-full bg-accent shadow-[inset_0_0_0_1px_rgb(0_0_0/0.15)]" />
      <span className="truncate">
        {what}
        {p.accentMode !== 'neutral' && `, ${p.intensity}`}
      </span>
    </>
  )
}

function IconSummary() {
  const id = useAppIcon()
  const name = APP_ICONS.find((i) => i.id === id)?.name ?? ''
  return (
    <>
      <img src={iconSrc(id)} alt="" className="h-6 w-6 shrink-0 rounded-md shadow-sm shadow-black/30" draggable={false} />
      <span className="truncate">
        {name}
        {id === DEFAULT_ICON && ' (the default)'}
      </span>
    </>
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

/** Where lyrics come from, and the two windows that show them away from Venuta. */
function LyricsPage() {
  return (
    <div>
      <PageHead title="Lyrics" line="Where lyrics come from, what shows under each line, and the two windows that show them away from Venuta." />
      <Group
        title="Sources, in the order they are asked"
        line="For songs without lyrics of their own, the first source with timed lyrics is used. Any song can use another one (Now Playing, under its lyrics)."
      >
        <LyricsSources />
      </Group>
      <LyricLinesSettings />
      <LyricWindowsSettings />
      <p className="mt-6 max-w-[66ch] text-[12.5px] leading-relaxed text-ink/50">
        Each window's own toolbar changes the same settings while it is open. Lyrics found online are kept so each song is
        asked once; Clear cache in System empties them with the rest.
      </p>
    </div>
  )
}

const DETAIL_SOURCES = [
  ['musicbrainz', 'MusicBrainz', 'Original years, composers and lyricists, release types. Free and open (CC0).'],
  ['apple', "Apple's catalogue", 'Sharper covers (up to 3000 px), genres and release dates.'],
  ['deezer', 'Deezer', 'Covers, labels and recording codes.'],
  ['lrclib', 'LRCLIB, into your files', "Timed lyrics saved inside your files' tags, checked for timing and script first. (Lyrics while playing: the Lyrics page.)"],
] as const

function LibraryPage(props: {
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
  const keys = [s?.acoustid_key, s?.discogs_token].filter(Boolean).length
  return (
    <div>
      <PageHead title="Library" line="Where your music lives, where its details come from, and how it is kept safe." />
      <Group title="Folders">
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
              ? 'A folder of music you already have. Venuta only reads it and builds a tidy copy in your library. Also in Organize, New music.'
              : `${props.root}. Only read, never changed; Organize builds a tidy copy of it in your library.`
          }
        >
          <button onClick={props.onChooseLibrary} className="btn">
            Choose a folder
          </button>
        </Row>
      </Group>
      <Group title="Where details come from" line="Tags, covers and lyrics for your files. Optional keys stay on this computer.">
        {DETAIL_SOURCES.map(([key, title, hint]) => (
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
        <More
          title="Sources that need a key"
          hint={`AcoustID recognises untagged files by their sound; Discogs fills credits MusicBrainz lacks. ${keys ? `${keys} added.` : 'None added.'}`}
        >
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
        </More>
      </Group>
      <Group title="Keeping it safe">
        <Row
          title="Your backup"
          hint={
            s?.backup_confirmed
              ? 'You told Venuta you have a copy of your music elsewhere. Editing tags changes your files, safely, and each change can be undone.'
              : 'Not confirmed yet. Editing tags changes your files (safely, and each change can be undone); Venuta asks once, before its first edit.'
          }
        >
          {s?.backup_confirmed ? (
            <button onClick={() => save({ backup_confirmed: false })} className="btn-ghost btn-sm">
              I don't have one any more
            </button>
          ) : (
            <button onClick={() => save({ backup_confirmed: true })} className="btn">
              I have a backup
            </button>
          )}
        </Row>
        <p className="mt-2 max-w-[66ch] rounded-2xl bg-ink/[0.04] px-4 py-3 text-[12.5px] leading-relaxed text-ink/60">
          Nothing is ever deleted. Duplicates and removed playlists go to the Set aside folder beside your music, and every
          change is recorded in Activity, where it can be undone.
        </p>
      </Group>
      <Group title="More">
        <More title="Organising rules" hint={`Artist names, categories, folder layout and genres, as plain text files you can edit. ${s?.rules_dir ?? ''}`}>
          <Row title="Rules folder" hint={s?.rules_dir ?? ''}>
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
        </More>
      </Group>
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

/** The online catalog's state in words: what works now, and the one thing to do when it doesn't. */
const CATALOG_STATES: Record<string, { label: string; tone: 'good' | 'problem' | 'quiet'; line: string }> = {
  live: { label: 'Live', tone: 'good', line: 'Search and full-length play are on.' },
  live_no_tool: {
    label: 'Live',
    tone: 'good',
    line: 'Search and full-length play are on. The backup stream tool, for the odd song the catalog won’t hand over directly, isn’t installed yet: Venuta downloads it the first time it’s needed.',
  },
  off: {
    label: 'Off',
    tone: 'quiet',
    line: 'The online catalog is forced off in this session (test mode), so online search shows a few sample songs only. Songs in your library play as always.',
  },
  fixture_only: {
    label: 'Test catalog',
    tone: 'quiet',
    line: 'This build has a small test catalog only: a few sample songs, no real search.',
  },
}

/** A command to copy, with a button that copies it. */
function Command(props: { text: string }) {
  return (
    <span className="mt-1.5 flex max-w-full items-center gap-2">
      <code className="min-w-0 truncate rounded-lg bg-ink/[0.06] px-2 py-1 text-[12.5px] text-ink/80">{props.text}</code>
      <button
        onClick={() => void navigator.clipboard.writeText(props.text).then(() => notify('Copied.'))}
        className="btn-ghost btn-sm shrink-0"
      >
        Copy
      </button>
    </span>
  )
}

/** Playback catalog health: Live / Off / stream tool missing, and how to fix what isn't working. */
function CatalogHealth() {
  const [catalog, setCatalog] = useState<OnlineCatalogStatus | null>(null)
  const [failed, setFailed] = useState(false)
  const check = useCallback(() => {
    onlineCatalogStatus()
      .then((c) => {
        setCatalog(c)
        setFailed(false)
      })
      .catch(() => setFailed(true))
  }, [])
  useEffect(check, [check])
  const state = catalog ? (CATALOG_STATES[catalog.mode] ?? CATALOG_STATES.off) : null
  const tone = state?.tone === 'good' ? 'bg-good' : state?.tone === 'problem' ? 'bg-fix' : 'bg-ink/35'
  return (
    <div className="border-b border-edge py-4">
      <div className="flex flex-wrap items-center gap-4">
        <div className="min-w-[220px] flex-1">
          <div className="flex items-center gap-2 text-[14px] font-medium">
            <span aria-hidden className={`h-2 w-2 shrink-0 rounded-full ${tone}`} />
            {failed ? 'Couldn’t read the online catalog’s state' : state ? state.label : 'Checking…'}
          </div>
          {state && <div className="mt-0.5 text-[12.5px] leading-relaxed text-ink/55">{state.line}</div>}
        </div>
        <button onClick={check} className="btn-ghost btn-sm">
          Check again
        </button>
      </div>
      {catalog?.mode === 'live' && catalog.stream_tool_kind && (
        <p className="mt-2 text-[12px] text-ink/45">
          Backup stream tool: {catalog.stream_tool_kind}
          {catalog.stream_tool_detail ? ` (${catalog.stream_tool_detail})` : ''}
        </p>
      )}
      {catalog?.mode === 'live_no_tool' && (
        <div className="mt-3 max-w-[64ch] rounded-xl bg-ink/[0.04] px-3.5 py-3 text-[12.5px] leading-relaxed text-ink/65">
          <p className="mb-2">To have the backup ready now, Venuta can download it (no Python required):</p>
          <button
            className="btn-primary btn-sm"
            onClick={() =>
              void onlineEnsureStreamTool()
                .then((p) => {
                  notify(p ? `Stream tool ready (${p}).` : 'Stream tool ready.')
                  check()
                })
                .catch((e) => notify(e instanceof Error ? e.message : String(e)))
            }
          >
            Install stream tool
          </button>
          <p className="mt-3 text-ink/50">Or install yt-dlp yourself:</p>
          <Command text="python -m pip install -U yt-dlp" />
          <p className="mt-2 text-ink/50">
            Optional: set <code className="rounded bg-ink/[0.06] px-1">VENUTA_STREAM_RESOLVER</code> to a yt-dlp.exe path.
          </p>
        </div>
      )}
      {catalog?.mode === 'off' && (
        <div className="mt-3 max-w-[64ch] text-[12.5px] leading-relaxed text-ink/50">
          <p>
            Live catalog is the normal mode. This session opted into fixtures only. Unset{" "}
            <code className="rounded bg-ink/[0.06] px-1">VENUTA_FAKE_CATALOG</code> and restart, or start:
          </p>
          <Command text="pwsh -File scripts/hybrid-dev.ps1" />
        </div>
      )}
    </div>
  )
}

function OnlinePage() {
  const { s, save, saved } = useSettings()
  return (
    <div>
      <PageHead
        title="Online"
        line="The online catalog plays songs you don't have as files yet, in the same player, queue and playlists. Your file always plays first when you have the song. No ads; playlists keep only each song's id."
      />
      <Group title="Your music and online">
        <LocalOnlyRow />
        <CatalogHealth />
      </Group>
      <Group title="Accounts">
        <YtmAccount />
        <SpotifyAccount />
        <More title="Apple Music" hint={`Bring playlists over from your Apple Music library, with a library key. ${s?.apple_key ? 'Key added.' : 'Not added.'}`}>
          <KeyField
            id="apple"
            title="Apple Music library key"
            hint="Reads your own Apple Music library to bring your playlists over. 16 characters; published in open-source Apple Music tools and not included with this app."
            where="open-source Apple Music library readers"
            value={s?.apple_key}
            length={16}
            onSave={(v) => save({ apple_key: v }, v ? 'Apple Music key saved.' : 'Apple Music key removed.')}
          />
        </More>
      </Group>
      <Group title="The catalog">
        <RegionRow />
      </Group>
      <Group title="Listen together">
        <TogetherRows />
      </Group>
      <Group title="More">
        <More title="Network" hint="Your own relay for rooms, and a proxy for networks that need one.">
          <RelayRow />
          <ProxySetting />
        </More>
      </Group>
      {saved && <p className="mt-3 text-[13px] text-good">{saved}</p>}
    </div>
  )
}

function SystemPage(props: { onShortcuts: () => void }) {
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
  const [v, setV] = useState('')
  useEffect(() => {
    engineVersion().then(setV)
  }, [])
  return (
    <div>
      <PageHead title="System" line="How Venuta runs on Windows, what it keeps, and its updates." />
      <Group title="On Windows">
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
      </Group>
      <HotkeysSetting />
      <Group title="History and storage">
        <HistoryRow />
        <CacheSetting />
      </Group>
      <UpdatesGroup version={v} />
      <Group title="Help">
        <Row title="Diagnostics" hint="Venuta's version, how online music is set up, and the last of the playback log, as text to paste into a report. No keys or accounts.">
          <button
            onClick={() =>
              void invoke<string>('diagnostics')
                .then((t) => navigator.clipboard.writeText(t))
                .then(() => notify('Diagnostics copied.'))
                .catch((e) => notify(String(e)))
            }
            className="btn"
          >
            Copy diagnostics
          </button>
        </Row>
        <Row title="Keyboard shortcuts" hint="Every key Venuta knows, in the app and from anywhere. Also on ?.">
          <button onClick={props.onShortcuts} className="btn">
            Show
          </button>
        </Row>
      </Group>
      <div className="mt-8 flex flex-col items-start gap-3 select-text">
        <Brand size={64} word={40} />
        <p className="text-[13px] text-ink/70">The essence of the flute. Version {v}, free and open source under the MIT licence.</p>
        <p className="max-w-[62ch] text-[12.5px] leading-relaxed text-ink/50">
          Details come from MusicBrainz (CC0), Apple's iTunes Search API, the Deezer API and LRCLIB, each used within its
          published limits. Optional: AcoustID and Discogs. No account, no tracking, and the app never downloads music.
        </p>
        <p className="max-w-[62ch] text-[12.5px] leading-relaxed text-ink/50">
          Pronunciation under lyrics is made on this PC with word lists adapted from Google's Dakshina dataset (CC BY-SA 4.0),
          OpenCC's character tables (Apache-2.0), pinyin_pro (MIT), a port of koroman (MIT), and lindera (MIT) with the IPADIC
          dictionary (under its NAIST notice), fetched the first time a Japanese song needs it.
        </p>
        <p className="max-w-[62ch] text-[12.5px] leading-relaxed text-ink/50">
          Translation on this PC runs Mozilla's Firefox Translations models, downloaded from Mozilla when you ask for one,
          with fxtranslate (MPL-2.0); nothing you play or read is sent anywhere. Translation with your own key sends a
          song's lyrics to the service you chose (DeepL, Microsoft, Google or Anthropic) only when you translate.
        </p>
      </div>
      {saved && <p className="mt-3 text-[13px] text-good">{saved}</p>}
    </div>
  )
}
