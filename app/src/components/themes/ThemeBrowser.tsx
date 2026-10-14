import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useMemo, useState, type MouseEvent } from 'react'
import { open as openDialog, save as saveDialog } from '@tauri-apps/plugin-dialog'
import { inApp } from '../../lib/engine'
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
import { STYLES, styleInfo, type Style } from '../../lib/themes'
import { confirmAsk } from '../Confirm'
import { Chip, Segmented } from '../ui'
import { useContextMenu } from '../../screens/listen/parts'
import { CardWith, ThemeArt, ThemeCard, ThemeSwatch } from './ThemeCard'
import { openStudio } from './ThemeStudio'
import { openImport } from './ThemeImport'
import { copyThemePrompt } from './themePrompt'

/*
  The theme part of Look and feel: the theme in use and what you can do with it, the themes you
  used lately in one row, and every theme in a browser that stays closed until you want it
  (search, filters, and groups that show one row until opened).
*/

/** Put a theme of yours on screen (and keep the choice). */
export const chooseTheme = (pack: Pack) => setPrefs({ style: pack.extends, pack: pack.id })
const chooseBuiltIn = (style: Style) => setPrefs({ style, pack: '' })

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
  preview: Style
  tokens?: Record<string, string>
  albumLight: boolean
  modes: Mode[]
  installed?: Installed
  bundled?: Bundled
}

function useEntries(): Entry[] {
  const packs = usePacks()
  const [more, setMore] = useState<Bundled[]>([])
  useEffect(() => {
    void bundledPacks().then(setMore)
  }, [])
  return useMemo(() => {
    const yours: Entry[] = packs.map((t) => {
      const pk = t.pack
      return {
        key: `pack:${t.id}`,
        kind: 'yours',
        name: pk?.name ?? t.id,
        line: pk ? pk.description || `Built on ${styleInfo(pk.extends).name}.` : "Can't be used.",
        preview: pk?.extends ?? 'venuta',
        tokens: pk ? cardTokens(pk) : undefined,
        albumLight: pk ? packInfo(pk).accent === 'yours' : false,
        modes: pk?.modes ?? ['dark'],
        installed: t,
      }
    })
    const built: Entry[] = STYLES.map((s) => ({
      key: `style:${s.id}`,
      kind: 'built',
      name: s.name,
      line: s.line,
      preview: s.id,
      albumLight: s.accent === 'yours',
      modes: s.modes ? ['dark', 'light'] : ['dark'],
    }))
    const venuta: Entry[] = more
      .filter((b) => !packs.some((t) => t.id === b.id))
      .map((b) => ({
        key: `venuta:${b.id}`,
        kind: 'venuta',
        name: b.pack.name,
        line: b.pack.description,
        preview: b.pack.extends,
        tokens: cardTokens(b.pack),
        albumLight: packInfo(b.pack).accent === 'yours',
        modes: b.pack.modes,
        bundled: b,
      }))
    // Venuta's own themes lead, everywhere.
    return [...built, ...yours, ...venuta]
  }, [packs, more])
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

const OPEN = 'look-browse'
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

/** The theme in use, the recent row, and the browser. */
export function Themes() {
  const p = usePrefs()
  const entries = useEntries()
  const [browsing, setBrowsing] = useRemembered(OPEN, false)
  const key = themeKey(p.style, p.pack)
  const inUse = entries.find((e) => e.key === key) ?? entries.find((e) => e.key === `style:${p.style}`)
  const failure = p.pack ? packFailure() : ''
  return (
    <section aria-label="Theme">
      {inUse && <InUse entry={inUse} browsing={browsing} onBrowse={() => setBrowsing(!browsing)} />}
      {failure && (
        <div role="alert" className="mt-3 flex flex-wrap items-center gap-3 rounded-xl bg-problem/10 px-4 py-3 text-[12.5px] leading-relaxed">
          <span className="min-w-0 flex-1">
            Your theme couldn't load: {failure} {styleInfo(p.style).name}, the theme it's built on, is showing instead.
          </span>
          <button className="btn" onClick={() => setPrefs({ pack: '' })}>
            Use {styleInfo(p.style).name}
          </button>
        </div>
      )}
      <Recent entries={entries} current={inUse?.key ?? key} />
      <AnimatePresence initial={false}>
        {browsing && (
          <motion.div
            key="browser"
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: 'auto', opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{ duration: 0.28, ease: [0.2, 0.8, 0.2, 1] }}
            className="overflow-hidden"
          >
            <Browser entries={entries} current={inUse?.key ?? key} />
          </motion.div>
        )}
      </AnimatePresence>
    </section>
  )
}

/* ------------------------------------------------------------------------------------------ */
/* In use                                                                                       */
/* ------------------------------------------------------------------------------------------ */

function InUse({ entry, browsing, onBrowse }: { entry: Entry; browsing: boolean; onBrowse: () => void }) {
  const p = usePrefs()
  const info = themeInfo(p)
  const pk = entry.installed?.pack
  const { onContextMenu, menu } = useContextMenu(() =>
    entry.installed ? themeActions(entry.installed).filter(([label]) => label !== 'Use') : [['Make a copy to change', () => openStudio({ create: true })]],
  )
  const kind = entry.kind === 'yours' && pk ? `Yours, built on ${styleInfo(pk.extends).name}` : 'Built in'
  const modes = info.modes ? 'light and dark' : resolvedTheme(p) === 'light' ? 'light' : 'dark'
  return (
    <div className="flex flex-wrap items-center gap-5 rounded-2xl bg-ink/[0.035] p-4 shadow-[inset_0_0_0_1px_var(--edge)]">
      <div className="w-[200px] shrink-0 rounded-xl shadow-[0_0_0_1px_var(--edge)]">
        <ThemeArt preview={entry.preview} tokens={entry.tokens} albumLight={entry.albumLight} />
      </div>
      <div className="min-w-[240px] flex-1">
        <p className="text-[12px] text-ink/50">In use</p>
        <h3 className="title mt-0.5 text-[22px] leading-tight">{entry.name}</h3>
        <p className="mt-1 max-w-[56ch] text-[12.5px] leading-relaxed text-ink/60">{entry.line}</p>
        <p className="mt-1 text-[12px] text-ink/45">
          {kind}, {modes}
        </p>
        <div className="mt-3 flex flex-wrap items-center gap-2">
          {info.modes && (
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
          )}
          <span className="flex-1" />
          <button className="btn" aria-expanded={browsing} onClick={onBrowse}>
            {browsing ? 'Close themes' : 'Browse themes'}
            <Chevron open={browsing} />
          </button>
          <button className="btn" onClick={() => openStudio({ create: true })} disabled={!inApp()}>
            New theme
          </button>
          <button className="btn w-10 justify-center px-0" aria-label={`More for ${entry.name}`} title="More" onClick={onContextMenu}>
            <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor" aria-hidden>
              <circle cx="5" cy="12" r="1.9" />
              <circle cx="12" cy="12" r="1.9" />
              <circle cx="19" cy="12" r="1.9" />
            </svg>
          </button>
        </div>
      </div>
      {menu}
    </div>
  )
}

function Chevron({ open }: { open: boolean }) {
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden className={`transition-transform duration-200 ${open ? 'rotate-180' : ''}`}>
      <path d="m6 9 6 6 6-6" />
    </svg>
  )
}

/* ------------------------------------------------------------------------------------------ */
/* Recent                                                                                       */
/* ------------------------------------------------------------------------------------------ */

/** Five themes to switch to in one click: the ones used lately, or a few to try. */
function Recent({ entries, current }: { entries: Entry[]; current: string }) {
  const byKey = new Map(entries.map((e) => [e.key, e]))
  const used = recentThemes()
    .filter((k) => k !== current)
    .map((k) => byKey.get(k))
    .filter((e): e is Entry => !!e && (e.kind !== 'yours' || !!e.installed?.pack))
  const tried = used.length > 0
  const list = (tried ? used : entries.filter((e) => e.key !== current && e.kind === 'built')).slice(0, 5)
  if (!list.length) return null
  return (
    <div className="mt-3 flex flex-wrap items-center gap-2">
      <span className="mr-1 text-[12px] text-ink/50">{tried ? 'Recent' : 'Try'}</span>
      {list.map((e) => (
        <button
          key={e.key}
          onClick={() => pick(e)}
          title={e.kind === 'venuta' ? `${e.name}: adds it to your themes` : e.name}
          className="flex items-center gap-2 rounded-full bg-ink/[0.05] py-1 pl-1 pr-3 text-[12.5px] text-ink/80 shadow-[inset_0_0_0_1px_var(--edge)] transition-colors hover:bg-ink/[0.09] hover:text-ink"
        >
          <ThemeSwatch preview={e.preview} tokens={e.tokens} className="h-6 w-9 rounded-full" />
          {e.name}
        </button>
      ))}
    </div>
  )
}

/* ------------------------------------------------------------------------------------------ */
/* The browser                                                                                  */
/* ------------------------------------------------------------------------------------------ */

type Filter = 'all' | 'yours' | 'built' | 'venuta' | 'dark' | 'light'
const FILTERS: { id: Filter; label: string }[] = [
  { id: 'all', label: 'All' },
  { id: 'built', label: 'Built in' },
  { id: 'yours', label: 'Yours' },
  { id: 'venuta', label: 'From Venuta' },
  { id: 'dark', label: 'Dark' },
  { id: 'light', label: 'Light' },
]
const GROUPS: { kind: Entry['kind']; title: string; hint: string }[] = [
  { kind: 'built', title: 'Built in', hint: 'Every theme of yours is built on one of these and keeps what it does.' },
  { kind: 'yours', title: 'Yours', hint: 'Made here or given to you, kept with your library.' },
  { kind: 'venuta', title: 'More from Venuta', hint: 'Theme files like yours: choose one to add it, then use it or change it.' },
]
/** A group shows one row until it is opened. */
const ROW = 4

const fold = (s: string) => s.normalize('NFKD').replace(/[̀-ͯ]/g, '').toLowerCase()

function Browser({ entries, current }: { entries: Entry[]; current: string }) {
  const [q, setQ] = useState('')
  const [filter, setFilter] = useState<Filter>('all')
  const [opened, setOpened] = useState<Record<string, boolean>>({})
  const query = fold(q.trim())
  const matches = entries.filter((e) => {
    if (filter === 'yours' || filter === 'built' || filter === 'venuta') {
      if (e.kind !== filter) return false
    } else if (filter === 'dark' || filter === 'light') {
      if (!e.modes.includes(filter)) return false
    }
    return !query || fold(`${e.name} ${e.line} ${styleInfo(e.preview).name}`).includes(query)
  })
  const count = (f: Filter) =>
    f === 'all' ? entries.length : f === 'dark' || f === 'light' ? entries.filter((e) => e.modes.includes(f)).length : entries.filter((e) => e.kind === f).length
  const grouped = !query && filter === 'all'

  async function importFile() {
    const path = await openDialog({ multiple: false, filters: [{ name: 'Venuta theme', extensions: ['venuta-theme', 'json', 'zip'] }] })
    if (typeof path === 'string') void openImport(path)
  }

  return (
    <div className="pt-5">
      <div className="flex flex-wrap items-center gap-3">
        <label className="relative min-w-[200px] flex-1">
          <span className="sr-only">Find a theme</span>
          <svg className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-ink/45" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" aria-hidden>
            <circle cx="11" cy="11" r="7" />
            <path d="m20 20-3.5-3.5" />
          </svg>
          <input className="field h-9 w-full py-1 pl-9 text-[13px]" placeholder="Find a theme" value={q} onChange={(e) => setQ(e.target.value)} />
        </label>
        <button className="btn" onClick={() => void importFile()} disabled={!inApp()}>
          Import…
        </button>
      </div>
      <div className="mt-3 flex flex-wrap gap-1" role="group" aria-label="Show">
        {FILTERS.filter((f) => f.id === 'all' || f.id === filter || count(f.id) > 0).map((f) => (
          <Chip key={f.id} group="theme-filter" on={filter === f.id} label={f.label} count={count(f.id)} onClick={() => setFilter(f.id)} />
        ))}
      </div>

      {grouped ? (
        GROUPS.map((g) => {
          // The theme in use leads its group, so a folded group still shows it.
          const list = entries.filter((e) => e.kind === g.kind).sort((x, y) => Number(y.key === current) - Number(x.key === current))
          if (g.kind !== 'yours' && !list.length) return null
          const all = opened[g.kind] || list.length <= ROW + 1
          return (
            <section key={g.kind} className="mt-6" aria-label={g.title}>
              <div className="flex items-baseline gap-3">
                <h4 className="text-[14px] font-medium">{g.title}</h4>
                <span className="min-w-0 flex-1 truncate text-[12px] text-ink/50">{g.hint}</span>
                {!all && (
                  <button className="shrink-0 text-[12.5px] text-accent-ink underline-offset-2 hover:underline" onClick={() => setOpened({ ...opened, [g.kind]: true })}>
                    Show all {list.length}
                  </button>
                )}
                {opened[g.kind] && list.length > ROW + 1 && (
                  <button className="shrink-0 text-[12.5px] text-ink/55 hover:text-ink" onClick={() => setOpened({ ...opened, [g.kind]: false })}>
                    Show fewer
                  </button>
                )}
              </div>
              {list.length ? (
                <Grid entries={all ? list : list.slice(0, ROW)} current={current} />
              ) : (
                <p className="mt-3 rounded-xl border border-dashed border-[var(--edge-strong)] px-4 py-4 text-[12.5px] leading-relaxed text-ink/60">
                  None yet. <b className="font-medium text-ink/80">New theme</b> starts one from the theme in use, the album playing, or a few colours; or
                  add one from Venuta's and change it.
                </p>
              )}
            </section>
          )
        })
      ) : matches.length ? (
        <div className="mt-4">
          <Grid entries={matches} current={current} />
        </div>
      ) : (
        <p className="mt-6 text-[13px] text-ink/60">No theme matches{q ? ` "${q}"` : ''}.</p>
      )}

      <p className="mt-6 text-[12px] leading-relaxed text-ink/50">
        Making themes by hand or with an AI assistant: every value is in Venuta's THEMES guide.{' '}
        <button className="text-accent-ink underline-offset-2 hover:underline" onClick={() => void copyThemePrompt()}>
          Copy a prompt for an AI assistant
        </button>
      </p>
    </div>
  )
}

function Grid({ entries, current }: { entries: Entry[]; current: string }) {
  return (
    <div role="radiogroup" aria-label="Themes" className="mt-3 grid grid-cols-[repeat(auto-fill,minmax(176px,1fr))] gap-3">
      {entries.map((e) =>
        e.kind === 'yours' && e.installed ? (
          <PackCard key={e.key} t={e.installed} entry={e} on={e.key === current} />
        ) : (
          <ThemeCard key={e.key} name={e.name} preview={e.preview} tokens={e.tokens} albumLight={e.albumLight} on={e.key === current} onPick={() => pick(e)} tag={e.kind === 'venuta' ? 'Add' : undefined} />
        ),
      )}
    </div>
  )
}

/* ------------------------------------------------------------------------------------------ */
/* A theme of yours                                                                             */
/* ------------------------------------------------------------------------------------------ */

/** What can be done with a theme of yours (its … menu, here and in In use). */
function themeActions(t: Installed): [string, () => void][] {
  const pk = t.pack
  return [
    ...(pk ? ([['Use', () => chooseTheme(pk)], ['Edit in Theme Studio', () => openStudio({ id: t.id })]] as [string, () => void][]) : []),
    ['Duplicate', () => void duplicate(t)],
    ...(pk ? ([['Export…', () => void exportOne(t.id, pk.name)]] as [string, () => void][]) : []),
    ['Show the file', () => void showThemeFile(t.id).catch((e) => notify(String(e)))],
    ['Remove…', () => void remove(t)],
  ]
}

function PackCard({ t, entry, on }: { t: Installed; entry: Entry; on: boolean }) {
  const pk = t.pack
  const [notes, setNotes] = useState(false)
  const { onContextMenu, menu } = useContextMenu(() => themeActions(t))
  const problems = t.problems.filter((x) => !x.fatal)
  return (
    <CardWith
      card={
        pk ? (
          <ThemeCard name={entry.name} preview={entry.preview} tokens={entry.tokens} albumLight={entry.albumLight} on={on} onPick={() => chooseTheme(pk)} onContextMenu={onContextMenu} />
        ) : (
          <div onContextMenu={onContextMenu as (e: MouseEvent) => void} className="flex aspect-[16/10] flex-col justify-end rounded-xl border border-dashed border-[var(--edge-strong)] p-3 text-[12px] leading-snug text-ink/65">
            <span className="text-problem">Can't be used</span>
            {t.problems[0]?.message}
          </div>
        )
      }
    >
      <button
        aria-label={`More for ${entry.name}`}
        title="More"
        onClick={onContextMenu}
        className="glass absolute right-1.5 top-1.5 flex h-7 w-7 items-center justify-center rounded-full text-[15px] leading-none text-ink/75 opacity-0 shadow-[inset_0_0_0_1px_var(--edge)] transition-opacity hover:text-ink focus-visible:opacity-100 [div:hover>&]:opacity-100"
      >
        …
      </button>
      {!pk && <div className="mt-2 px-0.5 text-[13.5px] font-medium text-ink/80">{t.id}</div>}
      {problems.length > 0 && (
        <div className="mt-1 px-0.5">
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
