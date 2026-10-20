import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useMemo, useRef, useState, type MouseEvent } from 'react'
import { open as openDialog, save as saveDialog } from '@tauri-apps/plugin-dialog'
import { inApp } from '../../lib/engine'
import { useEscape } from '../../lib/layers'
import { notify } from '../../lib/notify'
import {
  addBundled,
  bundledPacks,
  duplicateTheme,
  exportTheme,
  freeId,
  loadPacks,
  packFailure,
  setThemeAside,
  showThemeFile,
  usePacks,
  type Bundled,
  type Installed,
} from '../../lib/packs'
import { fontStack, FONT_SLOTS, packInfo, tokensIn, type Mode, type Pack } from '../../lib/packFormat'
import { getPrefs, resolvedTheme, setPrefs, themeInfo, usePrefs } from '../../lib/prefs'
import { recentThemes, themeKey } from '../../lib/recentThemes'
import { filterThemes, MODE_FILTERS, SOURCES, themeHint, type ModeFilter, type Source } from '../../lib/themeFilter'
import { STYLES, styleInfo, type Style } from '../../lib/themes'
import { confirmAsk } from '../Confirm'
import { keyFor, matcher, ranked } from '../../lib/textsearch'
import { Chip, Segmented } from '../ui'
import { useContextMenu } from '../../screens/listen/parts'
import { CardWith, ThemeCard } from './ThemeCard'
import { openStudio } from './ThemeStudio'
import { openImport } from './ThemeImport'
import { copyThemePrompt } from './themePrompt'

/*
  The theme part of Look and feel: every theme you have as a gallery, filtered by where it comes
  from and its modes, the one in use marked and a click choosing another. Venuta's themes not
  yet added, importing a file and the guide for making one are one click away in Explore themes.
*/

/** Put a theme of yours on screen (and keep the choice). */
export const chooseTheme = (pack: Pack) => setPrefs({ style: pack.extends, pack: pack.id })
const chooseBuiltIn = (style: Style) => setPrefs({ style, pack: '' })

/** The theme Venuta starts with, marked in the gallery. */
const DEFAULT_KEY = 'style:venuta'

/** A pack's values for a preview: in the mode it shows in now (or the one it has). */
export function cardTokens(pack: Pack): Record<string, string> {
  const now = resolvedTheme()
  const mode = pack.modes.includes(now) ? now : pack.modes[0]
  const t = tokensIn(pack, mode)
  for (const slot of FONT_SLOTS) {
    const s = fontStack(pack, slot)
    if (s && pack.fonts[slot]?.startsWith('builtin:')) t[`font-${slot}`] = s
  }
  return t
}

/** Any theme, one shape: built in, yours, or one that comes with Venuta not yet added. */
interface Entry {
  key: string
  kind: 'yours' | 'built' | 'venuta'
  name: string
  line: string
  /** On hover: what it is, where it comes from, its modes. */
  hint: string
  preview: Style
  tokens?: Record<string, string>
  albumLight: boolean
  modes: Mode[]
  usable: boolean
  installed?: Installed
  bundled?: Bundled
}
/** A theme you have: built in or yours (the gallery). */
type Mine = Entry & { kind: 'yours' | 'built' }

function useEntries(): { mine: Mine[]; more: Entry[] } {
  const packs = usePacks()
  const [bundled, setBundled] = useState<Bundled[]>([])
  useEffect(() => {
    void bundledPacks().then(setBundled)
  }, [])
  return useMemo(() => {
    const yours: Mine[] = packs.map((t) => {
      const pk = t.pack
      const modes: Mode[] = pk?.modes ?? ['dark']
      const line = pk ? pk.description : "Can't be used."
      return {
        key: `pack:${t.id}`,
        kind: 'yours',
        name: pk?.name ?? t.id,
        line,
        hint: themeHint({ line, kind: 'yours', base: styleInfo(pk?.extends ?? 'venuta').name, modes }),
        preview: pk?.extends ?? 'venuta',
        tokens: pk ? cardTokens(pk) : undefined,
        albumLight: pk ? packInfo(pk).accent === 'yours' : false,
        modes,
        usable: !!pk,
        installed: t,
      }
    })
    const built: Mine[] = STYLES.map((s) => {
      const modes: Mode[] = s.modes ? ['dark', 'light'] : ['dark']
      return {
        key: `style:${s.id}`,
        kind: 'built',
        name: s.name,
        line: s.line,
        hint: themeHint({ line: s.line, kind: 'built', modes }),
        preview: s.id,
        albumLight: s.accent === 'yours',
        modes,
        usable: true,
      }
    })
    const more: Entry[] = bundled
      .filter((b) => !packs.some((t) => t.id === b.id))
      .map((b) => ({
        key: `venuta:${b.id}`,
        kind: 'venuta',
        name: b.pack.name,
        line: b.pack.description,
        hint: themeHint({ line: b.pack.description, kind: 'venuta', base: styleInfo(b.pack.extends).name, modes: b.pack.modes }),
        preview: b.pack.extends,
        tokens: cardTokens(b.pack),
        albumLight: packInfo(b.pack).accent === 'yours',
        modes: b.pack.modes,
        usable: true,
        bundled: b,
      }))
    // Venuta's own themes lead, everywhere.
    return { mine: [...built, ...yours], more }
  }, [packs, bundled])
}

function pick(e: Entry) {
  if (e.kind === 'built') chooseBuiltIn(e.preview)
  else if (e.kind === 'yours' && e.installed?.pack) chooseTheme(e.installed.pack)
  else if (e.kind === 'venuta' && e.bundled) {
    const b = e.bundled
    void addBundled(b)
      .then(() => {
        chooseTheme(b.pack)
        notify(`Added ${b.pack.name} to your themes.`)
      })
      .catch((err) => notify(`Couldn't add ${b.pack.name}: ${String(err)}`))
  }
}

function useRemembered(key: string, initial: boolean): [boolean, (v: boolean) => void] {
  const [v, setV] = useState(() => {
    try {
      const s = localStorage.getItem(key)
      return s === null ? initial : s === '1'
    } catch {
      return initial
    }
  })
  const set = (next: boolean) => {
    setV(next)
    try {
      localStorage.setItem(key, next ? '1' : '0')
    } catch {
      // Remembered for now only.
    }
  }
  return [v, set]
}
export { useRemembered }

/* ------------------------------------------------------------------------------------------ */
/* Light, dark, or as Windows is                                                                */
/* ------------------------------------------------------------------------------------------ */

/** Light, Dark or Match Windows, for a theme that has both (in its own words, if it has them). */
export function AppearanceChoice() {
  const p = usePrefs()
  const info = themeInfo(p)
  if (!info.modes) return null
  return (
    <div title={info.modeNames?.hint}>
      <Segmented
        size="sm"
        label={info.modeNames?.title ?? 'Mode'}
        value={p.theme}
        onChange={(theme) => setPrefs({ theme })}
        options={[
          { value: 'light', label: info.modeNames?.light ?? 'Light' },
          { value: 'dark', label: info.modeNames?.dark ?? 'Dark' },
          { value: 'system', label: 'Match Windows' },
        ]}
      />
    </div>
  )
}

/* ------------------------------------------------------------------------------------------ */
/* The gallery                                                                                  */
/* ------------------------------------------------------------------------------------------ */

/** Every theme you have, its filters, and Explore themes and New theme. */
export function Themes() {
  const p = usePrefs()
  const { mine, more } = useEntries()
  const [source, setSource] = useState<Source>('all')
  const [mode, setMode] = useState<ModeFilter>('all')
  // Finding a theme: the search takes the filters' place and looks through every theme you have.
  const [finding, setFinding] = useState(false)
  const [q, setQ] = useState('')
  const [exploring, setExploring] = useState(false)
  const findButton = useRef<HTMLButtonElement>(null)
  const key = themeKey(p.style, p.pack)
  const current = (mine.find((e) => e.key === key) ?? mine.find((e) => e.key === `style:${p.style}`))?.key ?? key
  const failure = p.pack ? packFailure() : ''
  // The same search as every other box (lib/textsearch.ts): word starts first, then the rest.
  const has = useMemo(() => matcher(q), [q])
  const filtered = finding ? mine : filterThemes(mine, { source, mode }, source === 'recent' ? recentThemes() : [])
  const shown = finding && q.trim() ? ranked(filtered, (e) => has(keyFor(e, () => `${e.name} ${e.line} ${styleInfo(e.preview).name}`))) : filtered
  const showAll = () => {
    setSource('all')
    setMode('all')
  }
  const stopFinding = () => {
    setFinding(false)
    setQ('')
    // Back where the search was opened from.
    requestAnimationFrame(() => findButton.current?.focus())
  }

  return (
    <section aria-label="Themes">
      <div className="flex flex-wrap items-center gap-x-2 gap-y-2.5">
        {finding ? (
          <Find value={q} onChange={setQ} onClose={stopFinding} />
        ) : (
          <>
            <div role="group" aria-label="Filter by source" className="flex items-center">
              {SOURCES.map((s) => (
                <Chip key={s.id} group="theme-source" on={source === s.id} label={s.label} onClick={() => setSource(s.id)} />
              ))}
            </div>
            <span aria-hidden className="mx-1 h-4 w-px bg-ink/15" />
            <div role="group" aria-label="Filter by mode" className="flex items-center">
              {MODE_FILTERS.map((m) => (
                <Chip key={m.id} group="theme-mode" on={mode === m.id} label={m.label} onClick={() => setMode(m.id)} />
              ))}
            </div>
          </>
        )}
        <div className="ml-auto flex items-center gap-2">
          {!finding && (
            <button ref={findButton} className="btn btn-sm h-[30px] w-[30px] p-0!" aria-label="Find a theme" title="Find a theme" onClick={() => setFinding(true)}>
              <SearchIcon />
            </button>
          )}
          <button className="btn btn-sm" aria-haspopup="dialog" onClick={() => setExploring(true)}>
            <Compass />
            Explore themes
          </button>
          <button className="btn btn-sm" onClick={() => openStudio({ create: true })} disabled={!inApp()} title="Start one from the theme in use, the album playing, or a few colours">
            <Plus />
            New theme
          </button>
        </div>
      </div>

      {failure && (
        <div role="alert" className="mt-4 flex flex-wrap items-center gap-3 rounded-xl bg-problem/10 px-4 py-3 text-[12.5px] leading-relaxed">
          <span className="min-w-0 flex-1">
            Your theme couldn't load: {failure} {styleInfo(p.style).name}, the theme it's built on, is showing instead.
          </span>
          <button className="btn btn-sm" onClick={() => setPrefs({ pack: '' })}>
            Use {styleInfo(p.style).name}
          </button>
        </div>
      )}

      {shown.length ? (
        <Grid entries={shown} current={current} />
      ) : (
        <Empty source={source} mode={mode} q={finding ? q : ''} onShowAll={showAll} onExplore={() => setExploring(true)} />
      )}

      <AnimatePresence>{exploring && <Explore entries={more} onClose={() => setExploring(false)} />}</AnimatePresence>
    </section>
  )
}

/** The search, in the filters' place while it is open. Escape clears it, then closes it. */
function Find({ value, onChange, onClose }: { value: string; onChange: (q: string) => void; onClose: () => void }) {
  return (
    <label className="relative block min-w-[240px] flex-1">
      <span className="sr-only">Find a theme</span>
      <span className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-ink/50">
        <SearchIcon />
      </span>
      <input
        autoFocus
        className="field h-[34px] w-full rounded-full! py-0 pl-9! pr-10! text-[13px]"
        placeholder="Find a theme"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          if (e.key !== 'Escape') return
          e.preventDefault()
          if (value) onChange('')
          else onClose()
        }}
      />
      <button
        type="button"
        aria-label="Close the search"
        title="Close (Esc)"
        onClick={onClose}
        className="absolute right-1 top-1/2 flex h-7 w-7 -translate-y-1/2 items-center justify-center rounded-full text-[17px] leading-none text-ink/55 hover:bg-ink/[0.08] hover:text-ink"
      >
        ×
      </button>
    </label>
  )
}

/** Nothing to show for these filters: say so in a line, and the way out. */
function Empty(props: { source: Source; mode: ModeFilter; q: string; onShowAll: () => void; onExplore: () => void }) {
  const { source, mode } = props
  const q = props.q.trim()
  const modeWord = mode === 'all' ? '' : `${mode} `
  const text = q
    ? `No theme matches "${q}".`
    : source === 'created' && mode === 'all'
      ? 'No themes of yours yet.'
      : source === 'recent' && mode === 'all'
        ? 'Themes you use show here.'
        : `No ${modeWord}themes ${source === 'created' ? 'of yours' : source === 'recent' ? 'used lately' : 'here'}.`
  const link = 'text-[12.5px] font-medium text-accent-ink underline-offset-2 hover:underline disabled:opacity-40'
  return (
    <div className="mt-4 flex flex-wrap items-center gap-x-4 gap-y-2 rounded-xl border border-dashed border-[var(--edge-strong)] px-4 py-5 text-[13px] text-ink/60">
      <span className="min-w-0 flex-1">{text}</span>
      {!q && source === 'created' && mode === 'all' ? (
        <>
          <button className={link} onClick={() => openStudio({ create: true })} disabled={!inApp()}>
            New theme
          </button>
          <button className={link} onClick={props.onExplore}>
            Explore themes
          </button>
        </>
      ) : (
        !q &&
        (source !== 'all' || mode !== 'all') && (
          <button className={link} onClick={props.onShowAll}>
            Show all themes
          </button>
        )
      )}
    </div>
  )
}

function Grid({ entries, current }: { entries: Entry[]; current: string }) {
  return (
    <div role="radiogroup" aria-label="Themes" className="mt-4 grid grid-cols-[repeat(auto-fill,minmax(148px,1fr))] gap-x-3 gap-y-4">
      {entries.map((e) => (
        <Card key={e.key} entry={e} on={e.key === current} />
      ))}
    </div>
  )
}

/* ------------------------------------------------------------------------------------------ */
/* A card                                                                                       */
/* ------------------------------------------------------------------------------------------ */

/** What can be done with a theme of yours (its … menu). */
function themeActions(t: Installed, on: boolean): [string, () => void][] {
  const pk = t.pack
  return [
    ...(pk ? ([...(on ? [] : [['Use', () => chooseTheme(pk)]]), ['Edit in Theme Studio', () => openStudio({ id: t.id })]] as [string, () => void][]) : []),
    ['Duplicate', () => void duplicate(t)],
    ...(pk ? ([['Export…', () => void exportOne(t.id, pk.name)]] as [string, () => void][]) : []),
    ['Show the file', () => void showThemeFile(t.id).catch((e) => notify(String(e)))],
    ['Remove…', () => void remove(t)],
  ]
}

/** What can be done with a built-in theme: use it, or start a theme of yours from it. */
function builtInActions(e: Entry, on: boolean): [string, () => void][] {
  return [
    ...(on ? [] : ([['Use', () => pick(e)]] as [string, () => void][])),
    [
      'Make a copy to change',
      () => {
        // The Theme Studio starts from the theme in use.
        if (!on) pick(e)
        openStudio({ create: true })
      },
    ],
  ]
}

/** A theme in the gallery: its live preview and name, and a … menu (also on right-click). */
function Card({ entry, on }: { entry: Entry; on: boolean }) {
  const t = entry.installed
  const pk = t?.pack
  const [notes, setNotes] = useState(false)
  const { onContextMenu, menu } = useContextMenu(() => (t ? themeActions(t, on) : builtInActions(entry, on)))
  const problems = t ? t.problems.filter((x) => !x.fatal) : []
  return (
    <CardWith
      card={
        !t || pk ? (
          <ThemeCard
            name={entry.name}
            preview={entry.preview}
            tokens={entry.tokens}
            albumLight={entry.albumLight}
            on={on}
            onPick={() => pick(entry)}
            onContextMenu={onContextMenu}
            hint={entry.hint}
            tag={entry.key === DEFAULT_KEY ? 'Default' : undefined}
          />
        ) : (
          <div
            onContextMenu={onContextMenu as (e: MouseEvent) => void}
            title={t.problems[0]?.message}
            className="flex aspect-[16/10] flex-col justify-end rounded-xl border border-dashed border-[var(--edge-strong)] p-3 text-[12px] leading-snug text-ink/65"
          >
            <span className="text-problem">Can't be used</span>
            <span className="line-clamp-2">{t.problems[0]?.message}</span>
          </div>
        )
      }
    >
      <button
        aria-label={`More for ${entry.name}`}
        title="More"
        onClick={onContextMenu}
        className="glass absolute right-1.5 top-1.5 flex h-7 w-7 items-center justify-center rounded-full text-ink/75 opacity-0 shadow-[inset_0_0_0_1px_var(--edge)] transition-opacity hover:text-ink focus-visible:opacity-100 [div:hover>&]:opacity-100"
      >
        <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor" aria-hidden>
          <circle cx="5" cy="12" r="1.9" />
          <circle cx="12" cy="12" r="1.9" />
          <circle cx="19" cy="12" r="1.9" />
        </svg>
      </button>
      {t && !pk && <div className="mt-2 truncate px-0.5 text-[13px] font-medium text-ink/80">{t.id}</div>}
      {problems.length > 0 && (
        <div className="mt-0.5 px-0.5">
          <button className="text-[11.5px] text-fix" onClick={() => setNotes((v) => !v)} aria-expanded={notes}>
            {problems.length === 1 ? '1 note' : `${problems.length} notes`}
          </button>
          {notes && (
            <ul className="mt-1 space-y-1 text-[11.5px] leading-snug text-ink/60">
              {problems.map((x, i) => (
                <li key={i}>
                  <code className="text-ink/75">{x.at}</code> {x.message}
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
      {menu}
    </CardWith>
  )
}

/* ------------------------------------------------------------------------------------------ */
/* Explore themes                                                                               */
/* ------------------------------------------------------------------------------------------ */

/** Venuta's themes not yet added, a theme file to import, and how to make one. */
function Explore({ entries, onClose }: { entries: Entry[]; onClose: () => void }) {
  useEscape(true, onClose)
  const box = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const before = document.activeElement as HTMLElement | null
    box.current?.focus()
    return () => before?.focus()
  }, [])

  async function importFile() {
    const path = await openDialog({ multiple: false, filters: [{ name: 'Venuta theme', extensions: ['venuta-theme', 'json', 'zip'] }] })
    if (typeof path === 'string') {
      onClose()
      void openImport(path)
    }
  }

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      transition={{ duration: 0.16 }}
      className="fixed inset-0 z-[90] flex items-center justify-center bg-scrim p-6"
      onPointerDown={(e) => e.target === e.currentTarget && onClose()}
    >
      <motion.div
        ref={box}
        tabIndex={-1}
        role="dialog"
        aria-modal="true"
        aria-labelledby="explore-title"
        initial={{ y: 8, scale: 0.98 }}
        animate={{ y: 0, scale: 1 }}
        exit={{ y: 8, scale: 0.98 }}
        transition={{ duration: 0.18, ease: [0.2, 0.8, 0.2, 1] }}
        className="popover flex max-h-full w-full max-w-[680px] flex-col overflow-hidden rounded-2xl"
        // Focus moves here so Tab starts in the dialog; it is not something to press.
        style={{ outline: 'none' }}
      >
        <div className="flex items-start gap-3 px-6 pb-1 pt-5">
          <div className="min-w-0 flex-1">
            <h2 id="explore-title" className="title text-[20px]">
              Explore themes
            </h2>
            <p className="mt-0.5 text-[12.5px] text-ink/55">Choose one to add it to your themes and use it.</p>
          </div>
          <button aria-label="Close" title="Close (Esc)" onClick={onClose} className="btn-ghost -mr-2 -mt-1 h-8 w-8 rounded-full p-0 text-[18px] leading-none">
            ×
          </button>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto px-6 pb-5 pt-3">
          {entries.length ? (
            <div role="radiogroup" aria-label="Venuta's themes" className="grid grid-cols-[repeat(auto-fill,minmax(140px,1fr))] gap-x-3 gap-y-4">
              {entries.map((e) => (
                <ThemeCard
                  key={e.key}
                  name={e.name}
                  preview={e.preview}
                  tokens={e.tokens}
                  albumLight={e.albumLight}
                  on={false}
                  hint={e.hint}
                  onPick={() => {
                    onClose()
                    pick(e)
                  }}
                />
              ))}
            </div>
          ) : (
            <p className="rounded-xl border border-dashed border-[var(--edge-strong)] px-4 py-5 text-[13px] text-ink/60">You have every theme that comes with Venuta.</p>
          )}
        </div>
        <div className="flex flex-wrap items-center gap-x-4 gap-y-2 border-t border-[var(--edge)] px-6 py-4">
          <button className="btn btn-sm" onClick={() => void importFile()} disabled={!inApp()}>
            Import a theme file…
          </button>
          <span className="min-w-0 flex-1 text-[12px] leading-relaxed text-ink/50">
            Making one by hand or with an AI assistant? Every value is in the THEMES guide.{' '}
            <button className="text-accent-ink underline-offset-2 hover:underline" onClick={() => void copyThemePrompt()}>
              Copy a prompt
            </button>
          </span>
        </div>
      </motion.div>
    </motion.div>
  )
}

/* ------------------------------------------------------------------------------------------ */
/* Icons                                                                                        */
/* ------------------------------------------------------------------------------------------ */

function SearchIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" aria-hidden>
      <circle cx="11" cy="11" r="7" />
      <path d="m20 20-3.5-3.5" />
    </svg>
  )
}

function Compass() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden className="-ml-0.5">
      <circle cx="12" cy="12" r="9" />
      <path d="m15.5 8.5-2 5-5 2 2-5z" />
    </svg>
  )
}

function Plus() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" aria-hidden className="-ml-0.5">
      <path d="M12 5v14M5 12h14" />
    </svg>
  )
}

/* ------------------------------------------------------------------------------------------ */
/* A theme of yours: copy, export, remove                                                       */
/* ------------------------------------------------------------------------------------------ */

async function duplicate(t: Installed) {
  const to = freeId(`${t.id}-copy`)
  try {
    await duplicateTheme(t.id, to)
    await loadPacks()
    notify(`Made a copy: ${to}.`, { label: 'Edit', run: () => openStudio({ id: to }) })
  } catch (e) {
    notify(`Couldn't copy the theme: ${String(e)}`)
  }
}

async function exportOne(id: string, name: string) {
  const dest = await saveDialog({ defaultPath: `${name}.venuta-theme`, filters: [{ name: 'Venuta theme', extensions: ['venuta-theme'] }] })
  if (!dest) return
  try {
    await exportTheme(id, dest)
    notify(`Saved ${name} as a theme file you can share.`)
  } catch (e) {
    notify(`Couldn't save the theme file: ${String(e)}`)
  }
}

async function remove(t: Installed) {
  const name = t.pack?.name ?? t.id
  const ok = await confirmAsk({
    title: `Remove ${name}?`,
    text: 'It moves to Set aside in your library, with everything it holds, so nothing is lost.',
    confirm: 'Remove',
    danger: true,
  })
  if (!ok) return
  try {
    if (getPrefs().pack === t.id) setPrefs({ pack: '' })
    await setThemeAside(t.id)
    await loadPacks()
    notify(`Moved ${name} to Set aside.`)
  } catch (e) {
    notify(`Couldn't remove the theme: ${String(e)}`)
  }
}
