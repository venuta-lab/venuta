import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useMemo, useRef, useState, useSyncExternalStore, type ReactNode } from 'react'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { useCovers } from '../../lib/covers'
import { useEscape } from '../../lib/layers'
import { notify } from '../../lib/notify'
import { current, usePlayerValue } from '../../lib/player'
import {
  addThemeFile,
  check,
  duplicateTheme,
  freeId,
  getPacks,
  loadPacks,
  saveTheme,
  scopeSheet,
  showDraft,
  themeHistory,
  usePacks,
  writeThemeText,
} from '../../lib/packs'
import {
  BUILTIN_FONTS,
  checkValue,
  contrast,
  FONT_SLOTS,
  GROUPS,
  hex,
  idFrom,
  mix,
  NO_EFFECTS,
  parseColor,
  readability,
  readableOn,
  rgba,
  SCENES,
  SIMPLE_DEFAULT,
  simpleFromColour,
  simpleFromTokens,
  simpleToJson,
  TOKENS,
  type FontSlot,
  type Mode,
  type Readability,
  type RGBA,
  type Simple,
  type TokenSpec,
} from '../../lib/packFormat'
import { resolvedTheme, setPrefs, usePrefs } from '../../lib/prefs'
import { STYLES, styleInfo, type Style } from '../../lib/themes'
import { confirmAsk } from '../Confirm'
import { Segmented, Switch } from '../ui'

/*
  The Theme Studio: a panel beside the app, which is the preview. Simple makes every value from a
  few choices; Advanced edits every value, the effects, fonts, background and the expert
  stylesheet. Nothing is kept until Save (each save keeps the version before it).
*/

type Request = { create: true } | { id: string }
let request: Request | null = null
const listeners = new Set<() => void>()
const emit = () => listeners.forEach((l) => l())

export function openStudio(r: Request) {
  request = r
  emit()
}
function closeStudio() {
  request = null
  showDraft(null)
  emit()
}

export function StudioHost() {
  const r = useSyncExternalStore(
    (l) => {
      listeners.add(l)
      return () => {
        listeners.delete(l)
      }
    },
    () => request,
  )
  return <AnimatePresence>{r && <Studio key={'id' in r ? r.id : 'new'} request={r} />}</AnimatePresence>
}

const supports = (p: string, v: string) => {
  try {
    return CSS.supports(p, v)
  } catch {
    return false
  }
}

/** The values the page shows now, for every token (the draft's, or what it inherits). */
function pageValues(): Record<string, string> {
  const cs = getComputedStyle(document.documentElement)
  const out: Record<string, string> = {}
  for (const t of TOKENS) out[t.name] = cs.getPropertyValue(`--${t.name}`).trim()
  return out
}

function Studio({ request: r }: { request: Request }) {
  const [id, setId] = useState('create' in r ? '' : r.id)
  return (
    <motion.aside
      role="dialog"
      aria-label="Theme Studio"
      initial={{ opacity: 0, x: 24 }}
      animate={{ opacity: 1, x: 0 }}
      exit={{ opacity: 0, x: 24 }}
      transition={{ type: 'spring', stiffness: 420, damping: 36 }}
      className="popover fixed bottom-3 right-3 top-[52px] z-[85] flex w-[400px] max-w-[calc(100vw-24px)] flex-col overflow-hidden rounded-2xl shadow-[var(--shadow-e3)]"
    >
      {id ? <Editor id={id} /> : <Start onMade={setId} />}
    </motion.aside>
  )
}

function Head(props: { title: string; sub?: string; onClose: () => void; children?: ReactNode }) {
  // Escape closes the Studio as its × does (the editor asks about unsaved changes first).
  useEscape(true, props.onClose)
  return (
    <div className="shrink-0 border-b border-[var(--edge)] px-5 pb-3 pt-4">
      <div className="flex items-start gap-3">
        <div className="min-w-0 flex-1">
          <h2 className="title truncate text-[18px]">{props.title}</h2>
          {props.sub && <p className="mt-0.5 truncate text-[12px] text-ink/55">{props.sub}</p>}
        </div>
        <button aria-label="Close the Theme Studio" title="Close" onClick={props.onClose} className="btn-ghost -mr-2 -mt-1 h-8 w-8 rounded-full p-0 text-[18px] leading-none">
          ×
        </button>
      </div>
      {props.children}
    </div>
  )
}

/* ------------------------------------------------------------------------------------------ */
/* A new theme                                                                                 */
/* ------------------------------------------------------------------------------------------ */

type From = 'current' | 'album' | 'colours'

function Start({ onMade }: { onMade: (id: string) => void }) {
  const p = usePrefs()
  const dir = usePlayerValue((s) => current(s)?.dir ?? '')
  const cover = useCovers(320)
  const art = dir ? cover(dir) : undefined
  const inUse = p.pack ? getPacks().find((t) => t.id === p.pack)?.pack?.name : styleInfo(p.style).name
  const [name, setName] = useState('')
  const [from, setFrom] = useState<From>('current')
  const [busy, setBusy] = useState(false)

  async function make() {
    setBusy(true)
    try {
      const shown = name.trim() || 'My theme'
      const id = freeId(idFrom(shown))
      const mode = resolvedTheme()
      const meta = { id, name: shown, author: '', description: '' }
      if (from === 'current' && p.pack && getPacks().some((t) => t.id === p.pack && t.pack)) {
        // A theme of yours: a copy of it, files and all, under the new name.
        await duplicateTheme(p.pack, id)
        const j = JSON.parse(getPacks().find((t) => t.id === p.pack)!.json) as Record<string, unknown>
        await saveTheme(id, JSON.stringify({ ...j, id, name: shown }, null, 2))
      } else {
        let json: Record<string, unknown>
        if (from === 'current') {
          // Exactly the built-in theme to start with; the simple choices read from how it shows.
          json = { format: 1, id, name: shown, extends: p.style, studio: { simple: simpleFromTokens(pageValues(), p.style, mode) } }
        } else if (from === 'album' && art) {
          const second = await coverSecondColour(art.src, art.accent).catch(() => '')
          json = simpleToJson(simpleFromColour(art.accent ?? '#8185eb', mode, 'venuta', second), meta)
        } else {
          json = simpleToJson({ ...SIMPLE_DEFAULT }, meta)
        }
        await saveTheme(id, JSON.stringify(json, null, 2))
      }
      await loadPacks()
      onMade(id)
    } catch (e) {
      notify(`Couldn't start the theme: ${String(e)}`)
      setBusy(false)
    }
  }

  const choices: { id: From; title: string; text: string; off?: boolean }[] = [
    { id: 'current', title: `From ${inUse}`, text: 'Starts exactly as the theme in use looks.' },
    { id: 'album', title: 'From the album playing', text: art ? "Its cover's colours, deep for the page and lit for the key." : 'Play something first.', off: !art },
    { id: 'colours', title: 'From a few colours', text: 'Plum and blossom pink to begin with; change any of them.' },
  ]
  return (
    <>
      <Head title="New theme" sub="Kept with your library, in your themes." onClose={closeStudio} />
      <div className="flex-1 space-y-5 overflow-y-auto px-5 py-4">
        <label className="block">
          <span className="text-[12.5px] text-ink/70">Name</span>
          <input autoFocus className="field mt-1.5 w-full" value={name} placeholder="My theme" maxLength={60} onChange={(e) => setName(e.target.value)} onKeyDown={(e) => e.key === 'Enter' && !busy && void make()} />
        </label>
        <div role="radiogroup" aria-label="Start from" className="space-y-2">
          {choices.map((c) => (
            <button
              key={c.id}
              role="radio"
              aria-checked={from === c.id}
              disabled={c.off}
              onClick={() => setFrom(c.id)}
              className="glass block w-full rounded-xl px-4 py-3 text-left transition-shadow disabled:opacity-45"
              style={{ boxShadow: from === c.id ? '0 0 0 2px var(--color-accent)' : 'inset 0 0 0 1px var(--edge)' }}
            >
              <span className="block text-[13.5px] font-medium text-ink">{c.title}</span>
              <span className="mt-0.5 block text-[12px] leading-snug text-ink/60">{c.text}</span>
            </button>
          ))}
        </div>
      </div>
      <div className="flex shrink-0 justify-end gap-2 border-t border-[var(--edge)] px-5 py-3">
        <button className="btn-ghost" onClick={closeStudio}>
          Cancel
        </button>
        <button className="btn-primary" disabled={busy} onClick={() => void make()}>
          Start
        </button>
      </div>
    </>
  )
}

/** A second colour from the cover, far in hue from its accent ('' if there isn't one). */
async function coverSecondColour(src: string, accent: string | null): Promise<string> {
  const img = new Image()
  img.crossOrigin = 'anonymous'
  img.src = src
  await img.decode()
  const c = document.createElement('canvas')
  c.width = c.height = 32
  const g = c.getContext('2d')!
  g.drawImage(img, 0, 0, 32, 32)
  const d = g.getImageData(0, 0, 32, 32).data
  const hue = (r: number, gg: number, b: number) => {
    const mx = Math.max(r, gg, b)
    const mn = Math.min(r, gg, b)
    if (mx - mn < 40) return -1
    const h = mx === r ? (gg - b) / (mx - mn) : mx === gg ? 2 + (b - r) / (mx - mn) : 4 + (r - gg) / (mx - mn)
    return ((h * 60) + 360) % 360
  }
  const a = parseColor(accent ?? '')
  const ah = a ? hue(a[0], a[1], a[2]) : -1
  const bins = new Map<number, { n: number; rgb: [number, number, number] }>()
  for (let i = 0; i < d.length; i += 4) {
    const h = hue(d[i], d[i + 1], d[i + 2])
    if (h < 0 || (ah >= 0 && Math.min(Math.abs(h - ah), 360 - Math.abs(h - ah)) < 45)) continue
    const k = Math.floor(h / 30)
    const b = bins.get(k) ?? { n: 0, rgb: [0, 0, 0] }
    b.n++
    b.rgb = [b.rgb[0] + d[i], b.rgb[1] + d[i + 1], b.rgb[2] + d[i + 2]]
    bins.set(k, b)
  }
  const best = [...bins.values()].sort((x, y) => y.n - x.n)[0]
  if (!best || best.n < 20) return ''
  return hex([best.rgb[0] / best.n, best.rgb[1] / best.n, best.rgb[2] / best.n, 1])
}

/* ------------------------------------------------------------------------------------------ */
/* Editing                                                                                     */
/* ------------------------------------------------------------------------------------------ */

type Json = Record<string, unknown>
interface Doc {
  json: Json
  css: string
}

const clone = (d: Doc): Doc => ({ json: structuredClone(d.json), css: d.css })
const same = (a: Doc, b: Doc) => a.css === b.css && JSON.stringify(a.json) === JSON.stringify(b.json)
const obj = (v: unknown): Json => (v && typeof v === 'object' && !Array.isArray(v) ? (v as Json) : {})

function Editor({ id }: { id: string }) {
  const packs = usePacks()
  const t = packs.find((x) => x.id === id)
  const [doc, setDoc] = useState<Doc | null>(null)
  const saved = useRef<Doc | null>(null)
  const past = useRef<Doc[]>([])
  const future = useRef<Doc[]>([])
  const last = useRef({ key: '', at: 0 })
  const [tab, setTab] = useState<'simple' | 'advanced'>('simple')
  const [values, setValues] = useState<Record<string, string>>({})
  const [busy, setBusy] = useState(false)

  // The theme as saved, once.
  useEffect(() => {
    if (doc || !t) return
    void (async () => {
      let json: Json = {}
      try {
        json = obj(JSON.parse(t.json))
      } catch {
        json = { format: 1, id }
      }
      let css = ''
      if (typeof json.css === 'string') {
        try {
          const { invoke } = await import('@tauri-apps/api/core')
          css = new TextDecoder().decode(await invoke<ArrayBuffer>('theme_file', { id, path: json.css }))
        } catch {
          // Not there: an empty stylesheet.
        }
      }
      const d = { json, css }
      saved.current = clone(d)
      setDoc(d)
      if (!obj(json.studio).simple) setTab('advanced')
    })()
  }, [doc, t, id])

  // The app shows the theme as it is edited.
  const checked = useMemo(() => (doc ? check(id, JSON.stringify(doc.json), t?.files ?? []) : null), [doc, id, t?.files])
  useEffect(() => {
    if (checked?.pack && doc) showDraft({ id, pack: checked.pack, css: doc.css, files: t?.files ?? [] })
  }, [checked, doc, id, t?.files])
  useEffect(() => () => showDraft(null), [])
  // And the values the page shows (what it inherits too), once it has drawn them.
  useEffect(() => {
    const f = requestAnimationFrame(() => setValues(pageValues()))
    return () => cancelAnimationFrame(f)
  }, [checked])

  if (!t || !doc || !checked) {
    return (
      <>
        <Head title="Theme Studio" onClose={closeStudio} />
        <p className="px-5 py-4 text-[13px] text-ink/60">{t ? 'Reading the theme…' : "That theme isn't in your themes any more."}</p>
      </>
    )
  }

  const pack = checked.pack
  const dirty = !!saved.current && !same(doc, saved.current)

  /** A change; small ones to the same thing in a row (a slider dragged) undo as one. */
  function change(key: string, edit: (d: Doc) => void) {
    const next = clone(doc!)
    edit(next)
    const now = Date.now()
    if (!(key === last.current.key && now - last.current.at < 900)) {
      past.current.push(doc!)
      if (past.current.length > 200) past.current.shift()
    }
    last.current = { key, at: now }
    future.current = []
    setDoc(next)
  }
  const undo = () => {
    const prev = past.current.pop()
    if (!prev) return
    future.current.push(doc!)
    last.current = { key: '', at: 0 }
    setDoc(prev)
  }
  const redo = () => {
    const next = future.current.pop()
    if (!next) return
    past.current.push(doc!)
    last.current = { key: '', at: 0 }
    setDoc(next)
  }

  async function save() {
    if (!pack) return
    setBusy(true)
    try {
      const json: Json = { ...doc!.json, format: 1, id }
      if (doc!.css.trim() && typeof json.css !== 'string') json.css = 'extra.css'
      await saveTheme(id, JSON.stringify(json, null, 2))
      if (typeof json.css === 'string' && doc!.css !== saved.current?.css) await writeThemeText(id, json.css, doc!.css)
      saved.current = clone({ json, css: doc!.css })
      setDoc({ json, css: doc!.css })
      await loadPacks()
      setPrefs({ style: pack.extends, pack: id })
      notify(`Saved ${pack.name}. It's the theme in use.`)
    } catch (e) {
      notify(`Couldn't save: ${String(e)}`)
    } finally {
      setBusy(false)
    }
  }

  async function close() {
    if (dirty && !(await confirmAsk({ title: 'Leave without saving?', text: `Your changes to ${pack?.name ?? id} aren't saved.`, confirm: 'Leave', danger: true }))) return
    closeStudio()
  }

  const onKey = (e: React.KeyboardEvent) => {
    const typing = (e.target as HTMLElement).closest('input:not([type=range]):not([type=color]), textarea')
    if (typing || !(e.ctrlKey || e.metaKey)) return
    if (e.key.toLowerCase() === 'z') {
      e.preventDefault()
      if (e.shiftKey) redo()
      else undo()
    } else if (e.key.toLowerCase() === 'y') {
      e.preventDefault()
      redo()
    } else if (e.key.toLowerCase() === 's') {
      e.preventDefault()
      void save()
    }
  }

  const fatal = checked.problems.find((x) => x.fatal)
  return (
    <div className="flex min-h-0 flex-1 flex-col" onKeyDown={onKey}>
      <Head title={pack?.name ?? id} sub={`Theme Studio · built on ${styleInfo(pack?.extends ?? 'venuta').name}`} onClose={() => void close()}>
        <div className="mt-3">
          <Segmented
            value={tab}
            onChange={setTab}
            label="How to edit"
            tabs
            options={[
              { value: 'simple', label: 'Simple' },
              { value: 'advanced', label: 'Advanced' },
            ]}
          />
        </div>
      </Head>
      <div className="min-h-0 flex-1 space-y-6 overflow-y-auto overflow-x-hidden px-5 py-4">
        {fatal && <p className="rounded-xl bg-problem/10 px-3 py-2 text-[12.5px] text-problem">{fatal.message}</p>}
        {tab === 'simple' ? <SimpleTab doc={doc} id={id} change={change} values={values} /> : <AdvancedTab doc={doc} id={id} change={change} values={values} />}
        <ReadsWell values={values} change={change} />
        {checked.problems.filter((x) => !x.fatal).length > 0 && (
          <Section title="Left out">
            <ul className="space-y-1 text-[12px] leading-snug text-ink/65">
              {checked.problems
                .filter((x) => !x.fatal)
                .map((x, i) => (
                  <li key={i}>
                    <code className="text-ink/80">{x.at}</code> {x.message}
                  </li>
                ))}
            </ul>
          </Section>
        )}
        <Versions id={id} onLoad={(json) => change('history', (d) => (d.json = json))} />
      </div>
      <div className="flex shrink-0 items-center gap-2 border-t border-[var(--edge)] px-5 py-3">
        <button className="btn-ghost px-2" title="Undo (Ctrl+Z)" aria-label="Undo" disabled={!past.current.length} onClick={undo}>
          ↶
        </button>
        <button className="btn-ghost px-2" title="Redo (Ctrl+Shift+Z)" aria-label="Redo" disabled={!future.current.length} onClick={redo}>
          ↷
        </button>
        <span className="min-w-0 flex-1 truncate text-[12px] text-ink/55">{dirty ? 'Not saved' : 'Saved'}</span>
        <button className="btn-ghost" disabled={!dirty} onClick={() => saved.current && change('revert', (d) => Object.assign(d, clone(saved.current!)))}>
          Revert
        </button>
        <button className="btn-primary" disabled={!dirty || busy || !pack} onClick={() => void save()}>
          Save
        </button>
      </div>
    </div>
  )
}

type Change = (key: string, edit: (d: Doc) => void) => void

function Section(props: { title: string; hint?: string; children: ReactNode }) {
  return (
    <section>
      <h3 className="text-[12px] font-medium uppercase tracking-[0.06em] text-ink/50">{props.title}</h3>
      {props.hint && <p className="mt-1 text-[12px] leading-snug text-ink/55">{props.hint}</p>}
      <div className="mt-2.5 space-y-3">{props.children}</div>
    </section>
  )
}

function Field(props: { label: string; children: ReactNode; wide?: boolean }) {
  return (
    <label className={`flex items-center gap-3 ${props.wide ? 'flex-wrap' : ''}`}>
      <span className="w-[108px] shrink-0 text-[12.5px] text-ink/70">{props.label}</span>
      <span className="flex min-w-0 flex-1 items-center gap-2">{props.children}</span>
    </label>
  )
}

function Slider(props: { label: string; value: number; min: number; max: number; step: number; onChange: (v: number) => void; shown?: string }) {
  return (
    <Field label={props.label}>
      <input
        type="range"
        min={props.min}
        max={props.max}
        step={props.step}
        value={props.value}
        aria-label={props.label}
        onChange={(e) => props.onChange(Number(e.target.value))}
        className="h-1 min-w-0 flex-1 accent-[var(--accent)]"
      />
      <span className="w-11 text-right text-[11.5px] tabular-nums text-ink/55">{props.shown ?? props.value}</span>
    </Field>
  )
}

/** A colour: the picker for its colour, the text for anything CSS takes (alpha kept). */
function ColourInput(props: { label: string; value: string; placeholder?: string; onChange: (v: string) => void; error?: string | null }) {
  const p = parseColor(props.value || props.placeholder || '')
  return (
    <div>
      <Field label={props.label}>
        <input
          type="color"
          aria-label={`${props.label}: pick`}
          value={p ? hex(p) : '#000000'}
          onChange={(e) => {
            const n = parseColor(e.target.value)!
            props.onChange(p && p[3] < 1 ? rgba(n, p[3]) : e.target.value)
          }}
          className="h-7 w-9 shrink-0 cursor-pointer rounded-md border border-[var(--edge)] bg-transparent p-0.5"
        />
        <input className="field h-8 min-w-0 flex-1 py-1 font-mono text-[11.5px]" value={props.value} placeholder={props.placeholder} spellCheck={false} onChange={(e) => props.onChange(e.target.value)} />
      </Field>
      {props.error && <p className="ml-[120px] mt-1 text-[11.5px] text-problem">{props.error}</p>}
    </div>
  )
}

function Select<T extends string>(props: { label: string; value: T; options: { value: T; label: string }[]; onChange: (v: T) => void }) {
  return (
    <Field label={props.label}>
      <select className="field h-8 min-w-0 flex-1 py-1 text-[12.5px]" value={props.value} onChange={(e) => props.onChange(e.target.value as T)}>
        {props.options.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
    </Field>
  )
}

const BASES = STYLES.map((s) => ({ value: s.id, label: s.name }))
const FONTS = BUILTIN_FONTS.map((f) => ({ value: f, label: f.replace(' Variable', '') }))

/* ------------------------------------------------------------------------------------------ */
/* Simple                                                                                      */
/* ------------------------------------------------------------------------------------------ */

function SimpleTab({ doc, id, change, values }: { doc: Doc; id: string; change: Change; values: Record<string, string> }) {
  const s = obj(doc.json.studio).simple as Simple | undefined
  const mode: Mode = resolvedTheme()
  if (!s) {
    return (
      <div className="glass rounded-xl px-4 py-3 text-[12.5px] leading-relaxed text-ink/70">
        This theme was changed value by value (in Advanced, or by hand), so the simple choices aren't its own. Starting again from them makes all its values anew.
        <button
          className="btn mt-3 block"
          onClick={() =>
            change('simple-start', (d) => {
              const base = (typeof d.json.extends === 'string' ? d.json.extends : 'venuta') as Style
              const next = simpleFromTokens(values, base, mode)
              d.json = { ...fromSimple(next, d.json), studio: { simple: next } }
            })
          }
        >
          Start again from simple choices
        </button>
      </div>
    )
  }
  const set = (key: string, patch: Partial<Simple>) => change(`simple:${key}`, (d) => (d.json = fromSimple({ ...s, ...patch }, d.json)))
  const albumKey = s.accent === 'album'
  return (
    <>
      <Section title="Built on" hint="Keeps what that theme does: its player bar, seek bar, shapes and effects.">
        <Select label="Theme" value={s.extends} options={BASES} onChange={(v) => set('extends', { extends: v })} />
        <Field label="Mode">
          <Segmented
            size="sm"
            value={s.mode}
            onChange={(v) => set('mode', { mode: v })}
            label="Mode"
            options={[
              { value: 'dark', label: 'Dark' },
              { value: 'light', label: 'Light' },
            ]}
          />
        </Field>
      </Section>
      <Section title="Colours" hint="Text is kept readable on everything it sits on.">
        <ColourInput label="Background" value={s.background} onChange={(v) => set('background', { background: v })} />
        <ColourInput label="Panels" value={s.panels} onChange={(v) => set('panels', { panels: v })} />
        <ColourInput label="Text" value={s.text} onChange={(v) => set('text', { text: v })} />
        {albumKey ? (
          <Field label="Key colour">
            <span className="text-[12.5px] text-ink/70">From the album playing</span>
          </Field>
        ) : (
          <ColourInput label="Key colour" value={s.accent} onChange={(v) => set('accent', { accent: v })} />
        )}
        <Field label="">
          <Switch label="Key colour from the album playing" on={albumKey} onChange={(on) => set('accent-from', { accent: on ? 'album' : (values.accent && parseColor(values.accent) ? hex(parseColor(values.accent)!) : '#ff7aa8') })} />
          <span className="text-[12px] text-ink/60">From the album playing</span>
        </Field>
      </Section>
      <Section title="Glass">
        <Slider label="See-through" value={s.glass} min={0} max={1} step={0.01} onChange={(v) => set('glass', { glass: v })} shown={`${Math.round(s.glass * 100)}%`} />
        <Slider label="Blur" value={s.blur} min={0} max={48} step={1} onChange={(v) => set('blur', { blur: v })} shown={`${s.blur}px`} />
        <Slider label="Corners" value={s.round} min={0} max={1} step={0.01} onChange={(v) => set('round', { round: v })} shown={s.round < 0.15 ? 'Sharp' : s.round > 0.85 ? 'Round' : `${Math.round(2 + s.round * 22)}px`} />
      </Section>
      <Section title="Type">
        <Select label="Text" value={s.fontUi} options={FONTS} onChange={(v) => set('fontUi', { fontUi: v })} />
        <Select label="Titles" value={s.fontDisplay} options={FONTS} onChange={(v) => set('fontDisplay', { fontDisplay: v })} />
        <Field label="Capitals">
          <Switch label="Titles in capitals" on={s.capitals} onChange={(on) => set('capitals', { capitals: on })} />
        </Field>
      </Section>
      <Section title="Behind everything">
        <ColourInput label="Glow colour" value={s.gradient} placeholder="none" onChange={(v) => set('gradient', { gradient: v })} />
        <PictureField
          id={id}
          image={s.image}
          onPick={(image) => set('image', { image })}
          onClear={() => set('image', { image: '' })}
        />
        {s.image && <Slider label="Dim" value={s.imageDim} min={0} max={0.95} step={0.01} onChange={(v) => set('imageDim', { imageDim: v })} shown={`${Math.round(s.imageDim * 100)}%`} />}
      </Section>
      <Section title="Effects">
        <Slider label="Grain" value={s.effects.grain} min={0} max={1} step={0.01} onChange={(v) => set('grain', { effects: { ...s.effects, grain: v } })} shown={`${Math.round(s.effects.grain * 100)}%`} />
        <Slider label="Scanlines" value={s.effects.scanlines} min={0} max={1} step={0.01} onChange={(v) => set('scanlines', { effects: { ...s.effects, scanlines: v } })} shown={`${Math.round(s.effects.scanlines * 100)}%`} />
        <Field label="Halftone">
          <Switch label="Halftone dots" on={s.effects.halftone} onChange={(on) => set('halftone', { effects: { ...s.effects, halftone: on } })} />
        </Field>
        <Field label="Light trails">
          <Switch label="Light trails when the song changes" on={s.effects.trails} onChange={(on) => set('trails', { effects: { ...s.effects, trails: on } })} />
        </Field>
      </Section>
      <Section title="Logo">
        <Field label="Colour">
          <Segmented
            size="sm"
            value={s.markColor}
            onChange={(v) => set('markColor', { markColor: v })}
            label="Logo colour"
            options={[
              { value: 'accent', label: 'Key colour' },
              { value: 'ink', label: 'Text' },
            ]}
          />
        </Field>
        <Slider label="Glow" value={s.markGlow} min={0} max={1} step={0.01} onChange={(v) => set('markGlow', { markGlow: v })} shown={`${Math.round(s.markGlow * 100)}%`} />
      </Section>
    </>
  )
}

/** The theme.json for simple choices, keeping what the choices don't cover (name, stylesheet). */
function fromSimple(s: Simple, j: Json): Json {
  const meta = {
    id: typeof j.id === 'string' ? j.id : 'theme',
    name: typeof j.name === 'string' ? j.name : 'Theme',
    author: typeof j.author === 'string' ? j.author : '',
    description: typeof j.description === 'string' ? j.description : '',
  }
  const out = simpleToJson(s, meta)
  if (typeof j.css === 'string') out.css = j.css
  return out
}

/** Choose a picture from this computer: copied into the theme as art/background.<ext>. */
function PictureField(props: { id: string; image: string; onPick: (name: string) => void; onClear: () => void }) {
  async function pick() {
    const from = await openDialog({ multiple: false, filters: [{ name: 'Picture', extensions: ['jpg', 'jpeg', 'png', 'webp'] }] })
    if (typeof from !== 'string') return
    const ext = from.split('.').pop()!.toLowerCase().replace('jpeg', 'jpg')
    const name = `art/background.${ext}`
    try {
      await addThemeFile(props.id, name, from)
      await loadPacks()
      props.onPick(name)
    } catch (e) {
      notify(`Couldn't use that picture: ${String(e)}`)
    }
  }
  return (
    <Field label="Picture">
      <span className="min-w-0 flex-1 truncate text-[12px] text-ink/60">{props.image || 'None'}</span>
      <button className="btn h-8 px-3 text-[12px]" onClick={() => void pick()}>
        Choose…
      </button>
      {props.image && (
        <button className="btn-ghost h-8 px-2 text-[12px]" onClick={props.onClear}>
          Remove
        </button>
      )}
    </Field>
  )
}

/* ------------------------------------------------------------------------------------------ */
/* Readability                                                                                 */
/* ------------------------------------------------------------------------------------------ */

function ReadsWell({ values, change }: { values: Record<string, string>; change: Change }) {
  const issues = readability(values)
  if (!values['app-bg']) return null
  return (
    <Section title="Reads well">
      {issues.length === 0 ? (
        <p className="text-[12.5px] text-good">Everything reads well.</p>
      ) : (
        <ul className="space-y-2">
          {issues.map((r) => (
            <li key={r.what} className="flex items-center gap-3 text-[12.5px]">
              <span className="min-w-0 flex-1 text-ink/75">
                {r.what}: <span className="text-fix">{r.ratio}:1</span>, needs {r.needs}:1
              </span>
              <button className="btn h-7 px-3 text-[12px]" onClick={() => change(`fix:${r.what}`, (d) => fixReadability(d, r, values))}>
                Fix
              </button>
            </li>
          ))}
        </ul>
      )}
    </Section>
  )
}

/** Change the fewest values that make `r` read: the text, the text on the key colour, or the key colour as text. */
function fixReadability(d: Doc, r: Readability, v: Record<string, string>) {
  const bg = parseColor(v['app-bg']) ?? [0, 0, 0, 1]
  const setToken = (name: string, value: string) => {
    const simple = obj(d.json.studio).simple as Simple | undefined
    if (simple && name === 'ink') {
      d.json = fromSimple({ ...simple, text: value }, d.json)
      return
    }
    d.json.tokens = { ...obj(d.json.tokens), [name]: value }
    delete d.json.studio
  }
  if (r.what === 'Text on the key colour') {
    setToken('on-accent', readableOn(v.accent) ?? '#ffffff')
  } else if (r.what === 'Key colour as text') {
    let a = parseColor(v['accent-ink'] || v.accent) ?? [255, 255, 255, 1]
    const ink = parseColor(v.ink) ?? [255, 255, 255, 1]
    for (let i = 0; i < 16 && contrast(a, bg) < 3; i++) a = mix(a, ink, 0.12)
    setToken('accent-ink', hex(a))
  } else {
    let ink = parseColor(v.ink) ?? [255, 255, 255, 1]
    const target: RGBA = (parseColor(v['app-bg']) && contrast([255, 255, 255, 1], bg) > contrast([0, 0, 0, 1], bg)) ? [255, 255, 255, 1] : [0, 0, 0, 1]
    const tried = { ...v }
    for (let i = 0; i < 24 && readability({ ...tried, ink: hex(ink) }).some((x) => x.what.startsWith('Text on') && x.what !== 'Text on the key colour'); i++) ink = mix(ink, target, 0.12)
    setToken('ink', hex(ink))
  }
}

/* ------------------------------------------------------------------------------------------ */
/* Advanced                                                                                    */
/* ------------------------------------------------------------------------------------------ */

type Where = 'tokens' | 'dark' | 'light'

function AdvancedTab({ doc, id, change, values }: { doc: Doc; id: string; change: Change; values: Record<string, string> }) {
  const j = doc.json
  const base = (typeof j.extends === 'string' ? j.extends : 'venuta') as Style
  const modes = Array.isArray(j.modes) ? (j.modes as Mode[]) : styleInfo(base).modes ? (['dark', 'light'] as Mode[]) : (['dark'] as Mode[])
  const [where, setWhere] = useState<Where>('tokens')
  const [open, setOpen] = useState<string>('Colours')
  const accent = obj(j.accent)
  const effects = { ...NO_EFFECTS, ...obj(j.effects) }
  const mark = obj(j.mark)
  const bg = obj(j.background)
  const fonts = obj(j.fonts)
  const set = (key: string, edit: (j: Json) => void) =>
    change(key, (d) => {
      edit(d.json)
      // Changed value by value: the simple choices no longer describe it.
      delete d.json.studio
    })
  const tokensOf = (w: Where): Json => (w === 'tokens' ? obj(j.tokens) : obj(obj(j[w]).tokens))
  const setToken = (spec: TokenSpec, value: string) =>
    set(`tok:${where}:${spec.name}`, (x) => {
      const t = { ...(where === 'tokens' ? obj(x.tokens) : obj(obj(x[where]).tokens)) }
      if (value === '') delete t[spec.name]
      else t[spec.name] = value
      if (where === 'tokens') x.tokens = t
      else x[where] = { ...obj(x[where]), tokens: t }
    })
  const text = (k: string, v: string) => set(`meta:${k}`, (x) => (v ? (x[k] = v) : delete x[k]))

  return (
    <>
      <Section title="About">
        <Field label="Name">
          <input className="field h-8 min-w-0 flex-1 py-1 text-[12.5px]" value={String(j.name ?? '')} maxLength={60} onChange={(e) => text('name', e.target.value)} />
        </Field>
        <Field label="Made by">
          <input className="field h-8 min-w-0 flex-1 py-1 text-[12.5px]" value={String(j.author ?? '')} maxLength={80} onChange={(e) => text('author', e.target.value)} />
        </Field>
        <Field label="Description">
          <input className="field h-8 min-w-0 flex-1 py-1 text-[12.5px]" value={String(j.description ?? '')} maxLength={240} onChange={(e) => text('description', e.target.value)} />
        </Field>
        <Select label="Built on" value={base} options={BASES} onChange={(v) => set('extends', (x) => (x.extends = v))} />
        <Field label="Modes">
          <Segmented
            size="sm"
            value={modes.length > 1 ? 'both' : modes[0]}
            onChange={(v) => set('modes', (x) => (x.modes = v === 'both' ? ['dark', 'light'] : [v]))}
            label="Modes"
            options={[
              { value: 'dark', label: 'Dark' },
              { value: 'light', label: 'Light' },
              { value: 'both', label: 'Both' },
            ]}
          />
        </Field>
        <Field label="Key colour">
          <Segmented
            size="sm"
            value={typeof accent.color === 'string' ? 'color' : accent.from === 'album' ? 'album' : 'base'}
            onChange={(v) => set('accent', (x) => (x.accent = v === 'color' ? { color: values.accent && parseColor(values.accent) ? hex(parseColor(values.accent)!) : '#ff7aa8' } : { from: v }))}
            label="Key colour"
            options={[
              { value: 'base', label: "The base's" },
              { value: 'album', label: 'The album' },
              { value: 'color', label: 'One colour' },
            ]}
          />
        </Field>
        {typeof accent.color === 'string' && (
          <ColourInput label="Colour" value={accent.color} onChange={(v) => set('accent:color', (x) => (x.accent = { color: v }))} error={checkValue(TOKENS.find((t) => t.name === 'accent')!, accent.color, supports)} />
        )}
      </Section>

      <Section title="Values" hint="Empty takes the base theme's value (shown faintly). Every value is in docs/THEMES.md.">
        {modes.length > 1 && (
          <Segmented
            size="sm"
            value={where}
            onChange={setWhere}
            label="Which mode"
            options={[
              { value: 'tokens', label: 'Both modes' },
              { value: 'dark', label: 'Dark only' },
              { value: 'light', label: 'Light only' },
            ]}
          />
        )}
        {GROUPS.map((g) => (
          <div key={g} className="rounded-xl shadow-[inset_0_0_0_1px_var(--edge)]">
            <button className="flex w-full items-center justify-between px-3 py-2 text-left text-[12.5px] font-medium text-ink/85" aria-expanded={open === g} onClick={() => setOpen(open === g ? '' : g)}>
              {g}
              <span className="text-ink/45">{TOKENS.filter((t) => t.group === g && tokensOf(where)[t.name] !== undefined).length || ''}</span>
            </button>
            {open === g && (
              <div className="space-y-3 px-3 pb-3">
                {TOKENS.filter((t) => t.group === g).map((spec) => (
                  <TokenRow key={spec.name} spec={spec} value={String(tokensOf(where)[spec.name] ?? '')} shown={values[spec.name] ?? ''} onChange={(v) => setToken(spec, v)} />
                ))}
              </div>
            )}
          </div>
        ))}
      </Section>

      <Section title="Fonts" hint="A bundled font, or a font file of your own (.woff2, .ttf or .otf, up to 2 MB).">
        {FONT_SLOTS.map((slot) => (
          <FontRow key={slot} id={id} slot={slot} value={typeof fonts[slot] === 'string' ? (fonts[slot] as string) : ''} onChange={(v) => set(`font:${slot}`, (x) => {
            const f = { ...obj(x.fonts) }
            if (v) f[slot] = v
            else delete f[slot]
            x.fonts = f
          })} />
        ))}
      </Section>

      <Section title="Behind everything">
        <PictureField
          id={id}
          image={typeof bg.image === 'string' ? bg.image : ''}
          onPick={(image) => set('bg', (x) => (x.background = { dim: 0.35, blur: 0, ...obj(x.background), image }))}
          onClear={() => set('bg', (x) => delete x.background)}
        />
        {typeof bg.image === 'string' && (
          <>
            <Slider label="Dim" value={Number(bg.dim ?? 0.3)} min={0} max={0.95} step={0.01} onChange={(v) => set('bg:dim', (x) => (x.background = { ...obj(x.background), dim: v }))} shown={`${Math.round(Number(bg.dim ?? 0.3) * 100)}%`} />
            <Slider label="Blur" value={Number(bg.blur ?? 0)} min={0} max={60} step={1} onChange={(v) => set('bg:blur', (x) => (x.background = { ...obj(x.background), blur: v }))} shown={`${Number(bg.blur ?? 0)}px`} />
          </>
        )}
      </Section>

      <Section title="Effects" hint="Moving effects rest when motion is reduced.">
        <Select
          label="Scene"
          value={String(effects.scene)}
          options={[{ value: 'base', label: `${styleInfo(base).name}'s` }, ...SCENES.map((s) => ({ value: s, label: s === 'none' ? 'None' : styleInfo(s === 'bounty' ? 'bounty' : s).name }))]}
          onChange={(v) => set('fx:scene', (x) => (x.effects = { ...obj(x.effects), scene: v }))}
        />
        {(['grain', 'scanlines', 'halftone', 'hex'] as const).map((k) => (
          <Slider key={k} label={{ grain: 'Grain', scanlines: 'Scanlines', halftone: 'Halftone', hex: 'Hex grid' }[k]} value={Number(effects[k]) || 0} min={0} max={1} step={0.01} onChange={(v) => set(`fx:${k}`, (x) => (x.effects = { ...obj(x.effects), [k]: v }))} shown={`${Math.round((Number(effects[k]) || 0) * 100)}%`} />
        ))}
        {(
          [
            ['wires', 'Wires'],
            ['rain', 'Data rain'],
            ['trails', 'Light trails'],
            ['split', 'Colour split'],
            ['wipe', 'Colour wipe'],
            ['lcd', 'LCD spectrum'],
          ] as const
        ).map(([k, label]) => (
          <Field key={k} label={label}>
            <Switch label={label} on={effects[k] === true} onChange={(on) => set(`fx:${k}`, (x) => (x.effects = { ...obj(x.effects), [k]: on }))} />
          </Field>
        ))}
      </Section>

      <Section title="Logo">
        <Field label="Colour">
          <Segmented
            size="sm"
            value={mark.color === 'ink' ? 'ink' : typeof mark.color === 'string' && mark.color !== 'accent' ? 'own' : 'accent'}
            onChange={(v) => set('mark:color', (x) => (x.mark = { ...obj(x.mark), color: v === 'own' ? (values.accent || '#ffffff') : v }))}
            label="Logo colour"
            options={[
              { value: 'accent', label: 'Key colour' },
              { value: 'ink', label: 'Text' },
              { value: 'own', label: 'Its own' },
            ]}
          />
        </Field>
        {typeof mark.color === 'string' && mark.color !== 'accent' && mark.color !== 'ink' && (
          <ColourInput label="Its colour" value={mark.color} onChange={(v) => set('mark:own', (x) => (x.mark = { ...obj(x.mark), color: v }))} />
        )}
        <Slider label="Glow" value={Number(mark.glow ?? 0)} min={0} max={1} step={0.01} onChange={(v) => set('mark:glow', (x) => (x.mark = { color: 'accent', ...obj(x.mark), glow: v }))} shown={`${Math.round(Number(mark.glow ?? 0) * 100)}%`} />
      </Section>

      <Stylesheet doc={doc} change={change} />
    </>
  )
}

function TokenRow({ spec, value, shown, onChange }: { spec: TokenSpec; value: string; shown: string; onChange: (v: string) => void }) {
  const error = value ? checkValue(spec, value, supports) : null
  if (spec.type === 'color') return <ColourInput label={spec.label} value={value} placeholder={shown} onChange={onChange} error={error} />
  return (
    <div>
      <label className="block">
        <span className="block text-[12px] leading-snug text-ink/70">
          {spec.label} <code className="text-[11px] text-ink/40">--{spec.name}</code>
        </span>
        <input className="field mt-1 h-8 w-full py-1 font-mono text-[11.5px]" value={value} placeholder={shown} spellCheck={false} onChange={(e) => onChange(e.target.value)} />
      </label>
      {error && <p className="mt-1 text-[11.5px] text-problem">{error}</p>}
    </div>
  )
}

function FontRow({ id, slot, value, onChange }: { id: string; slot: FontSlot; value: string; onChange: (v: string) => void }) {
  const label = { ui: 'Text', display: 'Titles', readout: 'Numbers' }[slot]
  const file = value && !value.startsWith('builtin:')
  async function pick() {
    const from = await openDialog({ multiple: false, filters: [{ name: 'Font', extensions: ['woff2', 'woff', 'ttf', 'otf'] }] })
    if (typeof from !== 'string') return
    const name = `fonts/${slot}.${from.split('.').pop()!.toLowerCase()}`
    try {
      await addThemeFile(id, name, from)
      await loadPacks()
      onChange(name)
    } catch (e) {
      notify(`Couldn't use that font: ${String(e)}`)
    }
  }
  return (
    <Field label={label}>
      <select className="field h-8 min-w-0 flex-1 py-1 text-[12.5px]" value={file ? 'file' : value} onChange={(e) => (e.target.value === 'file' ? void pick() : onChange(e.target.value))}>
        <option value="">The base theme's</option>
        {FONTS.map((f) => (
          <option key={f.value} value={`builtin:${f.value}`}>
            {f.label}
          </option>
        ))}
        <option value="file">{file ? `Your file: ${value}` : 'A font file…'}</option>
      </select>
    </Field>
  )
}

function Stylesheet({ doc, change }: { doc: Doc; change: Change }) {
  const pack = check('preview', JSON.stringify(doc.json)).pack
  const problems = pack ? scopeSheet(doc.css, pack, () => 'blob:').problems : []
  return (
    <Section title="Stylesheet (for experts)" hint="CSS for this theme only: every rule is kept under it, and nothing can load from outside. Pictures in the theme can be used with url(art/…).">
      <textarea
        className="field min-h-[140px] w-full resize-y py-2 font-mono text-[11.5px] leading-relaxed"
        spellCheck={false}
        placeholder={':root {\n  --r-np: 4px;\n}\n.rail {\n  border-right: 1px solid var(--edge-strong);\n}'}
        value={doc.css}
        onChange={(e) => {
          const css = e.target.value
          change('css', (d) => {
            d.css = css
            if (css.trim() && typeof d.json.css !== 'string') d.json.css = 'extra.css'
          })
        }}
      />
      {problems.length > 0 && (
        <ul className="space-y-1 text-[11.5px] leading-snug text-problem">
          {problems.map((x, i) => (
            <li key={i}>{x.message}</li>
          ))}
        </ul>
      )}
    </Section>
  )
}

function Versions({ id, onLoad }: { id: string; onLoad: (json: Json) => void }) {
  const [list, setList] = useState<{ when: number; json: string }[] | null>(null)
  return (
    <Section title="Earlier versions" hint="Every save keeps the one before. Loading one changes nothing until you save.">
      {list === null ? (
        <button className="btn h-8 px-3 text-[12px]" onClick={() => void themeHistory(id).then(setList).catch(() => setList([]))}>
          Show earlier versions
        </button>
      ) : list.length === 0 ? (
        <p className="text-[12.5px] text-ink/55">None yet.</p>
      ) : (
        <ul className="max-h-[180px] space-y-1 overflow-y-auto">
          {list.map((v) => (
            <li key={v.when} className="flex items-center gap-3 text-[12.5px]">
              <span className="min-w-0 flex-1 text-ink/70">{new Date(v.when).toLocaleString()}</span>
              <button
                className="btn-ghost h-7 px-2 text-[12px]"
                onClick={() => {
                  try {
                    onLoad(obj(JSON.parse(v.json)))
                  } catch {
                    notify("That version couldn't be read.")
                  }
                }}
              >
                Load
              </button>
            </li>
          ))}
        </ul>
      )}
    </Section>
  )
}

