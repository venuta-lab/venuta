/*
  Your themes in the page: the packages in the library's .venuta/themes (lib/packFormat.ts says
  what they may hold), loaded, checked and put on <html> as data-pack="<id>" with a stylesheet
  of their own. The theme they extend stays underneath (data-style), so everything it does keeps
  working. A theme that can't load never breaks anything: the base shows, and Settings says why.
*/
import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { inApp } from './engine'
import {
  cleanValue,
  faceName,
  FONT_SLOTS,
  keyframesName,
  packCss,
  packInfo,
  readPack,
  scopeOf,
  scopeSelector,
  splitSelectors,
  type Mode,
  type Pack,
  type Problem,
  type Supports,
} from './packFormat'
import type { StyleInfo } from './themes'

export interface PackFile {
  path: string
  size: number
}

/** A package as the engine lists it. */
interface Listed {
  id: string
  json: string
  files: PackFile[]
  error: string | null
}

/** One of your themes, checked. */
export interface Installed {
  id: string
  json: string
  files: PackFile[]
  pack: Pack | null
  problems: Problem[]
}

const supports: Supports = (property, value) => {
  try {
    return CSS.supports(property, value)
  } catch {
    return false
  }
}

export function check(id: string, json: string, files: PackFile[] = [], error: string | null = null): Installed {
  if (error) return { id, json, files, pack: null, problems: [{ at: id, message: error, fatal: true }] }
  const { pack, problems } = readPack(json, id, supports)
  if (pack) {
    // Files it names that aren't there.
    const has = new Set(files.map((f) => f.path))
    for (const slot of FONT_SLOTS) {
      const f = pack.fonts[slot]
      if (f && !f.startsWith('builtin:') && !has.has(f)) {
        problems.push({ at: `fonts.${slot}`, message: `names "${f}", which isn't in the theme; left out` })
        delete pack.fonts[slot]
      }
    }
    if (pack.background && !has.has(pack.background.image)) {
      problems.push({ at: 'background.image', message: `names "${pack.background.image}", which isn't in the theme; left out` })
      pack.background = null
    }
    if (pack.css && !has.has(pack.css)) {
      problems.push({ at: 'css', message: `names "${pack.css}", which isn't in the theme; left out` })
      pack.css = ''
    }
  }
  return { id, json, files, pack, problems }
}

/* ------------------------------------------------------------------------------------------ */
/* The list of your themes                                                                     */
/* ------------------------------------------------------------------------------------------ */

let installed: Installed[] = []
let loaded = false
const listeners = new Set<() => void>()
const changed = () => listeners.forEach((l) => l())

/** Your themes, read again from the library. */
export async function loadPacks(): Promise<Installed[]> {
  const list = inApp() ? await invoke<Listed[]>('themes_list').catch(() => []) : []
  installed = list.map((p) => check(p.id, p.json, p.files, p.error))
  loaded = true
  changed()
  return installed
}

export const getPacks = () => installed

export function usePacks(): Installed[] {
  const [v, set] = useState(installed)
  useEffect(() => {
    const l = () => set(installed)
    listeners.add(l)
    if (!loaded) void loadPacks()
    return () => {
      listeners.delete(l)
    }
  }, [])
  return v
}

/* ------------------------------------------------------------------------------------------ */
/* The theme on screen                                                                         */
/* ------------------------------------------------------------------------------------------ */

/** A theme on the page: one of yours, or the Studio's draft. */
export interface Shown {
  id: string
  pack: Pack
  info: StyleInfo
  /** Its expert stylesheet, as written. */
  css: string
  files: PackFile[]
  /** The background picture's address, once read ('' until then). */
  background: string
}

let active: Shown | null = null
/** The Studio's theme while it is open: shown instead of the chosen one, never saved by showing. */
let draft: Shown | null = null
/** Why the chosen theme isn't showing (it couldn't load), or ''. */
let failure = ''
/** The theme asked for, and one that couldn't load (not tried again until its files change). */
let wanted = ''
let failedId = ''
/** Set up from what was kept on this computer, but not yet from its files. */
let fromCache = false
const activeListeners = new Set<() => void>()
const activeChanged = () => activeListeners.forEach((l) => l())

/** Run when the theme on screen changes (lib/prefs applies the look again). */
export function onPackChange(l: () => void) {
  activeListeners.add(l)
  return () => activeListeners.delete(l)
}

export const packFailure = () => failure

/** The theme of yours on screen for this choice: the Studio's draft, or the chosen one. */
export function onScreen(id: string): Shown | null {
  return draft ?? (id && active?.id === id ? active : null)
}

/** What the theme on screen does, if it's a theme of yours (or the Studio's). */
export const packInfoFor = (id: string): StyleInfo | null => onScreen(id)?.info ?? null

/** The modes the theme on screen has (null: not a theme of yours, or not loaded). */
export const packModes = (id: string): Mode[] | null => onScreen(id)?.pack.modes ?? null

/** The built-in theme under the Studio's draft, while it is open. */
export const draftBase = () => draft?.pack.extends ?? null

const CACHE = 'theme-pack:'
interface Cached {
  json: string
  css: string
}

function styleEl(): HTMLStyleElement {
  let el = document.getElementById('venuta-pack') as HTMLStyleElement | null
  if (!el) {
    el = document.createElement('style')
    el.id = 'venuta-pack'
    document.head.appendChild(el)
  }
  return el
}

/* Fonts and pictures, read once per file version and kept while the theme is in use. */

const MIME: Record<string, string> = { png: 'image/png', jpg: 'image/jpeg', jpeg: 'image/jpeg', webp: 'image/webp' }
const pictures = new Map<string, string>()
const reading = new Set<string>()
/** The font registered for each theme and slot, and which file version it is. */
const faces = new Map<string, { key: string; face: FontFace }>()

const fileKey = (id: string, f: PackFile) => `${id}/${f.path}#${f.size}`

/** Read a file of a theme. */
const fileBytes = (id: string, path: string) => invoke<ArrayBuffer>('theme_file', { id, path })

/** A picture of the theme on screen as an address, or null while it's read (it shows then). */
function pictureFor(t: Shown, name: string): string | null {
  const f = t.files.find((x) => x.path === name)
  const ext = name.split('.').pop()!.toLowerCase()
  if (!f || !MIME[ext]) return null
  const key = fileKey(t.id, f)
  const have = pictures.get(key)
  if (have) return have
  if (!reading.has(key)) {
    reading.add(key)
    fileBytes(t.id, name)
      .then((b) => {
        pictures.set(key, URL.createObjectURL(new Blob([b], { type: MIME[ext] })))
        render()
      })
      .catch(() => {})
      .finally(() => reading.delete(key))
  }
  return null
}

/** Register the theme's own font files (the text re-draws in them once they are read). */
function fontsFor(t: Shown) {
  for (const slot of FONT_SLOTS) {
    const name = t.pack.fonts[slot]
    const slotKey = `${t.id}/${slot}`
    const f = name && !name.startsWith('builtin:') ? t.files.find((x) => x.path === name) : undefined
    const had = faces.get(slotKey)
    if (!f) {
      if (had) document.fonts.delete(had.face)
      faces.delete(slotKey)
      continue
    }
    const key = fileKey(t.id, f)
    if (had?.key === key || reading.has(key)) continue
    reading.add(key)
    fileBytes(t.id, f.path)
      .then(async (b) => {
        const face = new FontFace(faceName(t.id, slot), b)
        await face.load()
        const old = faces.get(slotKey)
        if (old) document.fonts.delete(old.face)
        document.fonts.add(face)
        faces.set(slotKey, { key, face })
      })
      .catch(() => {
        // Not a font this computer reads: the theme's fallback shows.
      })
      .finally(() => reading.delete(key))
  }
}

/** Let go of the files of themes no longer on screen. */
function forgetOthers(keep: string[]) {
  for (const [k, u] of pictures) {
    if (!keep.some((id) => k.startsWith(`${id}/`))) {
      URL.revokeObjectURL(u)
      pictures.delete(k)
    }
  }
  for (const [k, v] of faces) {
    if (!keep.some((id) => k.startsWith(`${id}/`))) {
      document.fonts.delete(v.face)
      faces.delete(k)
    }
  }
}

/** Put the theme on screen (the draft, else the chosen one) into the page's stylesheet. */
function render() {
  const t = draft ?? active
  if (!t) return
  fontsFor(t)
  const scoped = scopeSheet(t.css, t.pack, (name) => pictureFor(t, name))
  const bg = t.pack.background ? (pictureFor(t, t.pack.background.image) ?? '') : ''
  const text = packCss(t.pack) + scoped.css + backdropCss(t.pack)
  const el = styleEl()
  if (el.textContent !== text) el.textContent = text
  document.documentElement.dataset.pack = t.id
  if (bg !== t.background) {
    t.background = bg
    activeChanged()
  }
}

/** With a picture behind everything, the album background steps aside. */
function backdropCss(pack: Pack): string {
  return pack.background ? `${scopeOf(pack.id)} {\n  --backdrop-opacity: 0;\n}\n` : ''
}

/** Take the chosen theme off the page: the base theme shows. */
function unset(why = '') {
  const had = active !== null || failure !== why
  active = null
  failure = why
  fromCache = false
  if (!draft) {
    forgetOthers([])
    document.getElementById('venuta-pack')?.remove()
    delete document.documentElement.dataset.pack
  }
  if (had) activeChanged()
}

/**
 * Put a theme of yours on the page (called by lib/prefs whenever the look is applied; '' takes
 * it off). The first frame comes from what was kept on this computer, so it never flashes the
 * base; the files are then read and the theme set up again in full.
 */
export function showPack(id: string) {
  wanted = id
  if (!id) {
    failedId = ''
    if (active || failure || (!draft && document.documentElement.dataset.pack)) unset()
    return
  }
  if (failedId === id) return
  if (active?.id === id) return
  try {
    const c = JSON.parse(localStorage.getItem(CACHE + id) ?? 'null') as Cached | null
    const r = c ? check(id, c.json) : null
    if (c && r?.pack) {
      active = { id, pack: r.pack, info: packInfo(r.pack), css: c.css, files: [], background: '' }
      failure = ''
      fromCache = true
      render()
      activeChanged()
    }
  } catch {
    // Nothing kept: the base shows until the theme is read.
  }
  void loadInFull(id)
}

let loading: Promise<void> | null = null
let loadingId = ''

async function loadInFull(id: string) {
  if (loading && loadingId === id) return loading
  loadingId = id
  loading = (async () => {
    const list = await loadPacks()
    const found = list.find((p) => p.id === id)
    if (wanted !== id) return
    if (!found?.pack) {
      const why = !found ? "It isn't in your themes any more." : (found.problems.find((p) => p.fatal)?.message ?? "It couldn't be read.")
      try {
        localStorage.removeItem(CACHE + id)
      } catch {
        // Nothing kept anyway.
      }
      failedId = id
      unset(why)
      return
    }
    let css = ''
    if (found.pack.css) {
      try {
        css = new TextDecoder().decode(await fileBytes(id, found.pack.css))
      } catch (e) {
        found.problems.push({ at: found.pack.css, message: `couldn't be read (${String(e)})` })
      }
    }
    if (wanted !== id) return
    found.problems.push(...scopeSheet(css, found.pack, (n) => (found.files.some((f) => f.path === n) ? 'x' : null)).problems)
    active = { id, pack: found.pack, info: packInfo(found.pack), css, files: found.files, background: active?.id === id ? active.background : '' }
    failure = ''
    fromCache = false
    forgetOthers([id, ...(draft ? [draft.id] : [])])
    render()
    try {
      localStorage.setItem(CACHE + id, JSON.stringify({ json: found.json, css } satisfies Cached))
    } catch {
      // Not kept: the next start reads the theme before showing it.
    }
    changed()
    activeChanged()
  })().finally(() => {
    loading = null
  })
  return loading
}

/**
 * The Studio's theme, shown live while it is edited (null: the chosen theme again). Only what
 * is saved is ever kept; this changes the screen, not your themes.
 */
export function showDraft(d: { id: string; pack: Pack; css: string; files: PackFile[] } | null) {
  if (d) {
    draft = { ...d, info: packInfo(d.pack), background: draft?.id === d.id ? draft.background : '' }
    render()
  } else {
    draft = null
    if (active) render()
    else {
      document.getElementById('venuta-pack')?.remove()
      delete document.documentElement.dataset.pack
    }
  }
  activeChanged()
}

/** Whether the chosen theme still waits for its files (for tests and the Studio). */
export const settling = () => fromCache || loading !== null

/* ------------------------------------------------------------------------------------------ */
/* Expert stylesheets                                                                          */
/* ------------------------------------------------------------------------------------------ */

/**
 * A theme's own stylesheet, read by the browser's parser and rewritten: every selector under
 * the theme, keyframes renamed to the theme, url() only to the theme's files, and nothing that
 * reaches outside (@import, @namespace…). What's removed is reported.
 */
export function scopeSheet(text: string, pack: Pack, files: (name: string) => string | null): { css: string; problems: Problem[] } {
  const problems: Problem[] = []
  if (!text.trim()) return { css: '', problems }
  const at = pack.css || 'extra.css'
  if (/@import/i.test(text)) problems.push({ at, message: '@import is not allowed (a theme loads nothing from outside); removed' })
  let sheet: CSSStyleSheet
  try {
    sheet = new CSSStyleSheet()
    sheet.replaceSync(text.replace(/@import[^;]*;/gi, ''))
  } catch (e) {
    problems.push({ at, message: `couldn't be read (${(e as Error).message})` })
    return { css: '', problems }
  }
  const scope = scopeOf(pack.id)
  const names = new Map<string, string>()
  const collect = (rules: CSSRuleList) => {
    for (const r of Array.from(rules)) {
      if (r instanceof CSSKeyframesRule) names.set(r.name, keyframesName(pack.id, r.name))
      else if ('cssRules' in r && r.cssRules) collect((r as CSSGroupingRule).cssRules)
    }
  }
  collect(sheet.cssRules)

  const cleanStyle = (style: CSSStyleDeclaration, where: string) => {
    for (const prop of Array.from(style)) {
      const value = style.getPropertyValue(prop)
      let v = cleanValue(value, files)
      if (v === null) {
        problems.push({ at, message: `${where}: "${prop}" loads something from outside or runs code; removed` })
        style.removeProperty(prop)
        continue
      }
      if (/^animation(-name)?$/.test(prop) && names.size) {
        v = v.replace(/[\w-]+/g, (w) => names.get(w) ?? w)
      }
      if (v !== value) style.setProperty(prop, v, style.getPropertyPriority(prop))
    }
  }

  const walk = (list: CSSRuleList, owner: { deleteRule(i: number): void }, nested: boolean) => {
    for (let i = list.length - 1; i >= 0; i--) {
      const r = list[i]
      if (r instanceof CSSStyleRule) {
        if (!nested) r.selectorText = splitSelectors(r.selectorText).map((s) => scopeSelector(s, scope)).join(', ')
        cleanStyle(r.style, r.selectorText.replace(scope, ':root').slice(0, 60))
        if (r.cssRules?.length) walk(r.cssRules, r, true)
      } else if (r instanceof CSSKeyframesRule) {
        r.name = names.get(r.name) ?? r.name
        for (const k of Array.from(r.cssRules)) cleanStyle((k as CSSKeyframeRule).style, `@keyframes ${r.name}`)
      } else if (r instanceof CSSMediaRule || r instanceof CSSSupportsRule || (typeof CSSContainerRule !== 'undefined' && r instanceof CSSContainerRule)) {
        walk((r as CSSGroupingRule).cssRules, r as CSSGroupingRule, nested)
      } else if (r instanceof CSSFontFaceRule) {
        // A font from the theme's own files, under a name of its own.
        cleanStyle(r.style, '@font-face')
        if (!r.style.getPropertyValue('src')) owner.deleteRule(i)
      } else {
        problems.push({ at, message: `${r.cssText.split('{')[0].trim().slice(0, 40)} isn't allowed in a theme; removed` })
        owner.deleteRule(i)
      }
    }
  }
  walk(sheet.cssRules, sheet, false)
  return { css: Array.from(sheet.cssRules, (r) => r.cssText).join('\n') + '\n', problems }
}

/* ------------------------------------------------------------------------------------------ */
/* Editing by hand: the folder is checked while Venuta is in front                              */
/* ------------------------------------------------------------------------------------------ */

let stamp = -1
let watching = false

/** Reload your themes when their files change (a theme edited in another program). */
export function watchPacks() {
  if (watching || !inApp()) return
  watching = true
  // While Venuta is in front, every couple of seconds; behind another program, it rests until
  // it comes back to the front (and looks at once then: that is when a theme edited elsewhere
  // is wanted).
  let timer = 0
  window.addEventListener('focus', () => {
    window.clearTimeout(timer)
    void tick()
  })
  let busy = false
  const tick = async () => {
    if (busy) return
    if (!document.hasFocus()) return
    busy = true
    try {
      const s = await invoke<number>('themes_stamp').catch(() => stamp)
      if (stamp !== -1 && s !== stamp) {
        await loadPacks()
        // The theme on screen, or one that couldn't load before: read again.
        if (wanted) {
          const was = failedId
          failedId = ''
          active = null
          if (was) showPack(wanted)
          else await loadInFull(wanted)
        }
      }
      stamp = s
    } finally {
      busy = false
    }
    window.clearTimeout(timer)
    timer = window.setTimeout(tick, 2000)
  }
  void tick()
}

/* ------------------------------------------------------------------------------------------ */
/* Changing your themes                                                                        */
/* ------------------------------------------------------------------------------------------ */

export const saveTheme = (id: string, json: string) => invoke<void>('theme_save', { id, json })
export const writeThemeText = (id: string, path: string, text: string) => invoke<void>('theme_write_text', { id, path, text })
export const addThemeFile = (id: string, path: string, from: string) => invoke<void>('theme_add_file', { id, path, from })
export const themeHistory = (id: string) => invoke<{ when: number; json: string }[]>('theme_history', { id })
export const setThemeAside = (id: string) => invoke<string>('theme_set_aside', { id })
export const exportTheme = (id: string, dest: string) => invoke<void>('theme_export', { id, dest })
export const inspectTheme = (path: string) => invoke<{ json: string; files: PackFile[]; skipped: string[] }>('theme_inspect', { path })
export const installTheme = (path: string, id: string, replace: boolean) => invoke<void>('theme_install', { path, id, replace })

/** An id for a new theme that isn't taken: "dusk", "dusk-2"… */
export function freeId(base: string, taken = installed.map((p) => p.id)): string {
  if (!taken.includes(base)) return base
  for (let n = 2; ; n++) if (!taken.includes(`${base}-${n}`)) return `${base}-${n}`
}


/* ------------------------------------------------------------------------------------------ */
/* The themes that come with Venuta: packages too, in public/theme-packs                       */
/* ------------------------------------------------------------------------------------------ */

export interface Bundled {
  id: string
  json: string
  css: string
  pack: Pack
}

let bundled: Bundled[] | null = null

export async function bundledPacks(): Promise<Bundled[]> {
  if (bundled) return bundled
  try {
    const ids = (await (await fetch('/theme-packs/index.json')).json()) as string[]
    const out: Bundled[] = []
    for (const id of ids) {
      const json = await (await fetch(`/theme-packs/${id}/theme.json`)).text()
      const r = readPack(json, id, supports)
      if (!r.pack) continue
      const css = r.pack.css ? await (await fetch(`/theme-packs/${id}/${r.pack.css}`)).text() : ''
      out.push({ id, json, css, pack: r.pack })
    }
    bundled = out
  } catch {
    bundled = []
  }
  return bundled
}

/** Add a theme that comes with Venuta to your themes (so you can use and change it). */
export async function addBundled(b: Bundled): Promise<string> {
  await saveTheme(b.id, b.json)
  if (b.css && b.pack.css) await writeThemeText(b.id, b.pack.css, b.css)
  await loadPacks()
  return b.id
}

export const duplicateTheme = (from: string, to: string) => invoke<void>('theme_duplicate', { from, to })
export const showThemeFile = (id: string) => invoke<void>('theme_show', { id })
