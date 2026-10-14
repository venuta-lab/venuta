/*
  Theme packages, format 1: what a theme.json may say, how each value is checked, and the CSS
  it becomes. Pure (no page, no Tauri), so it is tested as is (tests/packs.test.ts); the page
  side (loading files, fonts, pictures, applying) is lib/packs.ts. Every value is documented in
  docs/THEMES.md, which scripts/theme-docs.mjs writes from TOKENS below.
*/
import { STYLES, styleInfo, type Fx, type Style, type StyleInfo } from './themes.ts'

export const FORMAT = 1

/** How a value is checked: which CSS property must accept it (the browser's own parser). */
export type TokenType = 'color' | 'paint' | 'image' | 'filter' | 'backdrop' | 'shadow' | 'length' | 'opacity' | 'weight' | 'tracking' | 'case' | 'font'

const PROPERTY: Record<TokenType, string> = {
  color: 'color',
  paint: 'background',
  image: 'background-image',
  filter: 'filter',
  backdrop: 'backdrop-filter',
  shadow: 'box-shadow',
  length: 'border-radius',
  opacity: 'opacity',
  weight: 'font-weight',
  tracking: 'letter-spacing',
  case: 'text-transform',
  font: 'font-family',
}

export interface TokenSpec {
  name: string
  group: TokenGroup
  type: TokenType
  /** What it changes, in a few words. */
  label: string
}

export type TokenGroup = 'Colours' | 'Page' | 'Panels' | 'Over content' | 'Bars' | 'Rail' | 'Player bar' | 'Accent material' | 'Shape' | 'Type' | 'Stage'

export const GROUPS: TokenGroup[] = ['Colours', 'Page', 'Panels', 'Over content', 'Bars', 'Rail', 'Player bar', 'Accent material', 'Shape', 'Type', 'Stage']

const T = (group: TokenGroup, type: TokenType, name: string, label: string): TokenSpec => ({ name, group, type, label })

/** Every value a theme can set, as the app's CSS names it (without the leading --). */
export const TOKENS: TokenSpec[] = [
  T('Colours', 'color', 'ink', 'Text and icons'),
  T('Colours', 'color', 'accent', 'The key colour: Play, the chosen tab, progress'),
  T('Colours', 'color', 'on-accent', 'Text and icons on the key colour'),
  T('Colours', 'color', 'accent-ink', 'The key colour as text (links, "In use")'),
  T('Colours', 'color', 'accent-2', 'A second colour some themes use for highlights'),
  T('Colours', 'color', 'problem', 'Problems (Health, errors)'),
  T('Colours', 'color', 'fix', 'Things to fix (warnings)'),
  T('Colours', 'color', 'good', 'Good news (done, healthy)'),
  T('Colours', 'color', 'note', 'Notes and hints'),
  T('Colours', 'color', 'block', 'The colour block (themes built on Bounty Session)'),

  T('Page', 'color', 'app-bg', 'The window behind everything'),
  T('Page', 'image', 'app-image', 'A gradient over the window colour'),
  T('Page', 'color', 'solid', 'Solid fill where nothing may show through'),
  T('Page', 'opacity', 'backdrop-opacity', 'How much of the album background shows (0 to 1)'),
  T('Page', 'filter', 'backdrop-filter', 'How the album background is treated (a filter)'),
  T('Page', 'filter', 'art-tint', 'How album art inside pages is tinted (a filter)'),
  T('Page', 'color', 'scrim', 'The shade behind dialogs'),
  T('Page', 'image', 'vignette', 'Darkening at the window edges'),

  T('Panels', 'color', 'glass', 'Panels and cards on a page'),
  T('Panels', 'color', 'edge', 'Panel outlines'),
  T('Panels', 'color', 'edge-strong', 'Outlines that need to stand out'),
  T('Panels', 'color', 'highlight', 'The lit top edge of a panel'),
  T('Panels', 'shadow', 'shadow', 'Panel shadow'),
  T('Panels', 'shadow', 'shadow-e3', 'Shadow of things that float (dialogs)'),
  T('Panels', 'backdrop', 'blur', 'Blur behind panels that have one'),

  T('Over content', 'color', 'popover', 'Menus, the palette, drawers and dialogs'),
  T('Over content', 'backdrop', 'blur-over', 'Blur behind them'),

  T('Bars', 'color', 'bar', 'Bars pinned over scrolling content'),
  T('Bars', 'backdrop', 'bar-blur', 'Blur behind them'),
  T('Bars', 'color', 'bar-edge', 'Their outline'),

  T('Rail', 'color', 'rail-bg', 'The side rail'),
  T('Rail', 'backdrop', 'rail-blur', 'Blur behind the rail'),
  T('Rail', 'color', 'rail-edge', 'The rail outline'),
  T('Rail', 'shadow', 'rail-shadow', 'The rail shadow'),
  T('Rail', 'color', 'pill-bg', 'The chosen place on the rail'),
  T('Rail', 'color', 'pill-ink', 'Its text and icon'),
  T('Rail', 'shadow', 'pill-shadow', 'Its edge or glow'),

  T('Player bar', 'color', 'np-bg', 'The player bar'),
  T('Player bar', 'backdrop', 'np-blur', 'Blur behind it'),
  T('Player bar', 'color', 'np-edge', 'Its outline'),
  T('Player bar', 'shadow', 'np-shadow', 'Its shadow'),

  T('Accent material', 'paint', 'accent-fill', 'What buttons in the key colour are made of'),
  T('Accent material', 'shadow', 'accent-sheen', 'Their light (an inset shadow)'),
  T('Accent material', 'backdrop', 'accent-blur', 'Blur behind see-through ones'),

  T('Shape', 'length', 'r-rail', 'Corner of the rail'),
  T('Shape', 'length', 'r-np', 'Corner of the player bar and covers'),

  T('Type', 'font', 'font-ui', 'Everyday text'),
  T('Type', 'font', 'font-display', 'Titles'),
  T('Type', 'font', 'font-readout', 'Times and numbers'),
  T('Type', 'weight', 'title-weight', 'Title weight (100 to 900)'),
  T('Type', 'tracking', 'title-tracking', 'Title letter spacing'),
  T('Type', 'case', 'title-case', 'Titles in capitals or not'),
  T('Type', 'case', 'label-case', 'Small labels in capitals or not'),

  T('Stage', 'color', 'stage-bg', 'The inset stage (themes built on Glasshouse)'),
  T('Stage', 'color', 'stage-edge', 'Its outline'),
  T('Stage', 'length', 'stage-radius', 'Its corner'),
  T('Stage', 'length', 'stage-margin', 'Its distance from the window edge'),
]

export const TOKEN = new Map(TOKENS.map((t) => [t.name, t]))

/** The bundled typefaces a theme can name as "builtin:<family>". */
export const BUILTIN_FONTS = [
  'Sora Variable',
  'Inter Variable',
  'Figtree Variable',
  'Roboto Variable',
  'Dela Gothic One',
  'Barlow Condensed',
  'Chakra Petch',
  'Share Tech Mono',
  'Shippori Mincho B1',
  'DSEG7 Classic',
  'IBM Plex Mono',
  'Major Mono Display',
  'Big Shoulders Display',
  'Courier Prime',
  'Silkscreen',
  'Segoe UI Variable',
  'Georgia',
]

const FALLBACK: Record<FontSlot, string> = {
  ui: 'system-ui, sans-serif',
  display: 'system-ui, sans-serif',
  readout: 'ui-monospace, monospace',
}

export type FontSlot = 'ui' | 'display' | 'readout'
export const FONT_SLOTS: FontSlot[] = ['ui', 'display', 'readout']

/** The effects menu: layers a theme can add, besides the scene of the theme it extends. */
export interface Effects {
  /** The base theme's scene ("base"), another theme's, or none. */
  scene: 'base' | Fx
  grain: number
  scanlines: number
  halftone: number
  hex: number
  wires: boolean
  rain: boolean
  trails: boolean
  split: boolean
  wipe: boolean
  lcd: boolean
}

export const NO_EFFECTS: Effects = {
  scene: 'base',
  grain: 0,
  scanlines: 0,
  halftone: 0,
  hex: 0,
  wires: false,
  rain: false,
  trails: false,
  split: false,
  wipe: false,
  lcd: false,
}

export const SCENES: Fx[] = ['none', 'neo-tokyo', 'thermoptic', 'sync-ratio', 'layer-zero', 'bounty', 'skin99']

export type Mode = 'dark' | 'light'
export type AccentFrom = { from: 'base' | 'album' } | { color: string }

/** A theme package as Venuta uses it, after checking. */
export interface Pack {
  id: string
  name: string
  author: string
  description: string
  extends: Style
  modes: Mode[]
  accent: AccentFrom
  tokens: Record<string, string>
  dark: Record<string, string>
  light: Record<string, string>
  fonts: Partial<Record<FontSlot, string>>
  background: { image: string; dim: number; blur: number } | null
  effects: Effects
  mark: { color: string; glow: number } | null
  /** The expert stylesheet's file name in the package. */
  css: string
  /** The Studio's own notes (its simple choices), kept as written. */
  studio: unknown
}

export interface Problem {
  /** Where: "tokens.glass", "fonts.ui", "extra.css line 4"… */
  at: string
  message: string
  /** A problem that stops the theme loading (the others are left out and reported). */
  fatal?: boolean
}

/** Whether a CSS property accepts a value: the browser's CSS.supports, or a stand-in in tests. */
export type Supports = (property: string, value: string) => boolean

const KNOWN_KEYS = new Set(['format', 'id', 'name', 'author', 'description', 'extends', 'modes', 'accent', 'tokens', 'dark', 'light', 'fonts', 'background', 'effects', 'mark', 'css', 'studio', '$schema'])

export const ID_RE = /^[a-z0-9][a-z0-9-]{0,63}$/

/** A theme id from a name: "Midnight Sakura" → "midnight-sakura". */
export function idFrom(name: string): string {
  const id = name
    .normalize('NFKD')
    .replace(/[̀-ͯ]/g, '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 48)
  return id || 'theme'
}

/** Words a value may never hold: outside addresses, code, or a way out of its declaration. */
const UNSAFE = /url\s*\(|image-set|expression\s*\(|javascript:|@import|[;{}<>\\]|!important|\/\*/i

/**
 * Is `value` a safe, valid value for the token? Values may refer to other tokens with
 * var(--name) (only names Venuta has).
 */
export function checkValue(spec: TokenSpec, value: unknown, supports: Supports): string | null {
  if (typeof value !== 'string') return 'should be text'
  const v = value.trim()
  if (!v) return 'is empty'
  if (v.length > 400) return 'is too long'
  if (UNSAFE.test(v)) return 'has something a theme value may not contain'
  for (const m of v.matchAll(/var\(\s*--([a-z0-9-]+)/g)) {
    if (!TOKEN.has(m[1]) && !m[1].startsWith('color-') && !m[1].startsWith('accent')) return `refers to --${m[1]}, which Venuta doesn't have`
  }
  if (spec.type === 'case' && !['none', 'uppercase', 'lowercase', 'capitalize'].includes(v)) return 'should be none, uppercase, lowercase or capitalize'
  if (spec.type === 'opacity') {
    const n = Number(v)
    if (!(n >= 0 && n <= 1)) return 'should be a number from 0 to 1'
    return null
  }
  if (spec.type === 'weight' && /^\d+$/.test(v) && !(Number(v) >= 100 && Number(v) <= 900)) return 'should be from 100 to 900'
  // "none" switches a blur or filter off.
  if ((spec.type === 'backdrop' || spec.type === 'filter' || spec.type === 'image' || spec.type === 'shadow') && v === 'none') return null
  return supports(PROPERTY[spec.type], v) ? null : `isn't a valid ${describe(spec.type)}`
}

function describe(t: TokenType): string {
  return {
    color: 'colour',
    paint: 'colour or gradient',
    image: 'gradient',
    filter: 'filter',
    backdrop: 'blur',
    shadow: 'shadow',
    length: 'size',
    opacity: 'number',
    weight: 'weight',
    tracking: 'letter spacing',
    case: 'case',
    font: 'font list',
  }[t]
}

const isObj = (v: unknown): v is Record<string, unknown> => !!v && typeof v === 'object' && !Array.isArray(v)
const num = (v: unknown, lo: number, hi: number, d: number) => (typeof v === 'number' && Number.isFinite(v) ? Math.min(hi, Math.max(lo, v)) : d)

/** A file name inside the package (the engine checks again before reading). */
export const safeFile = (f: unknown): f is string =>
  typeof f === 'string' && f.length <= 200 && /^[A-Za-z0-9 _.()-]+(\/[A-Za-z0-9 _.()-]+)*$/.test(f) && !f.split('/').some((p) => p.startsWith('.'))

function tokenSet(raw: unknown, at: string, supports: Supports, problems: Problem[]): Record<string, string> {
  const out: Record<string, string> = {}
  if (raw === undefined) return out
  if (!isObj(raw)) {
    problems.push({ at, message: 'should be a list of values' })
    return out
  }
  for (const [k, v] of Object.entries(raw)) {
    const name = k.replace(/^--/, '')
    const spec = TOKEN.get(name)
    if (!spec) {
      problems.push({ at: `${at}.${k}`, message: "isn't a value Venuta has (see docs/THEMES.md); left out" })
      continue
    }
    const why = checkValue(spec, v, supports)
    if (why) {
      problems.push({ at: `${at}.${k}`, message: `${why}; left out` })
      continue
    }
    out[name] = (v as string).trim()
  }
  return out
}

/**
 * Check a theme.json. A theme with problems still loads, minus what's wrong; only a theme that
 * can't work at all (not JSON, a newer format, no base) is refused, with `fatal` problems.
 */
export function readPack(text: string, folderId: string, supports: Supports): { pack: Pack | null; problems: Problem[] } {
  const problems: Problem[] = []
  let raw: unknown
  try {
    raw = JSON.parse(text)
  } catch (e) {
    return { pack: null, problems: [{ at: 'theme.json', message: `isn't valid JSON (${(e as Error).message})`, fatal: true }] }
  }
  if (!isObj(raw)) return { pack: null, problems: [{ at: 'theme.json', message: 'should be an object', fatal: true }] }
  const format = raw.format ?? FORMAT
  if (typeof format !== 'number' || format > FORMAT) {
    return { pack: null, problems: [{ at: 'format', message: `is ${String(format)}: made for a newer Venuta. Update Venuta to use it.`, fatal: true }] }
  }
  for (const k of Object.keys(raw)) if (!KNOWN_KEYS.has(k)) problems.push({ at: k, message: "isn't something a theme says; kept, but not used" })

  const base = typeof raw.extends === 'string' ? raw.extends : 'venuta'
  if (!STYLES.some((s) => s.id === base)) {
    return { pack: null, problems: [...problems, { at: 'extends', message: `"${base}" isn't one of Venuta's themes`, fatal: true }] }
  }
  const info = styleInfo(base)
  const str = (v: unknown, max: number) => (typeof v === 'string' ? v.trim().slice(0, max) : '')

  let modes: Mode[] = info.modes ? ['dark', 'light'] : ['dark']
  if (raw.modes !== undefined) {
    const m = Array.isArray(raw.modes) ? raw.modes.filter((x): x is Mode => x === 'dark' || x === 'light') : []
    if (m.length) modes = [...new Set(m)]
    else problems.push({ at: 'modes', message: 'should be ["dark"], ["light"] or both; using the base theme\'s' })
  }

  let accent: AccentFrom = { from: 'base' }
  if (isObj(raw.accent)) {
    if (typeof raw.accent.color === 'string') {
      if (checkValue(TOKEN.get('accent')!, raw.accent.color, supports)) problems.push({ at: 'accent.color', message: "isn't a colour; using the base theme's accent" })
      else accent = { color: raw.accent.color.trim() }
    } else if (raw.accent.from === 'album' || raw.accent.from === 'base') accent = { from: raw.accent.from }
    else problems.push({ at: 'accent', message: 'should be {"from": "album"}, {"from": "base"} or {"color": "#…"}' })
  } else if (raw.accent !== undefined) problems.push({ at: 'accent', message: 'should be {"from": "album"}, {"from": "base"} or {"color": "#…"}' })

  const tokens = tokenSet(raw.tokens, 'tokens', supports, problems)
  const dark = tokenSet(isObj(raw.dark) ? raw.dark.tokens : undefined, 'dark.tokens', supports, problems)
  const light = tokenSet(isObj(raw.light) ? raw.light.tokens : undefined, 'light.tokens', supports, problems)

  const fonts: Partial<Record<FontSlot, string>> = {}
  if (isObj(raw.fonts)) {
    for (const [k, v] of Object.entries(raw.fonts)) {
      if (!FONT_SLOTS.includes(k as FontSlot)) {
        problems.push({ at: `fonts.${k}`, message: 'should be ui, display or readout' })
        continue
      }
      if (typeof v === 'string' && v.startsWith('builtin:') && BUILTIN_FONTS.includes(v.slice(8))) fonts[k as FontSlot] = v
      else if (safeFile(v) && /\.(woff2?|ttf|otf)$/i.test(v)) fonts[k as FontSlot] = v
      else problems.push({ at: `fonts.${k}`, message: `should be "builtin:<a bundled font>" or a font file in the theme (.woff2)` })
    }
  }

  let background: Pack['background'] = null
  if (isObj(raw.background)) {
    if (safeFile(raw.background.image) && /\.(png|jpe?g|webp)$/i.test(raw.background.image)) {
      background = { image: raw.background.image, dim: num(raw.background.dim, 0, 0.95, 0.3), blur: num(raw.background.blur, 0, 60, 0) }
    } else problems.push({ at: 'background.image', message: 'should be a picture in the theme (.jpg, .png or .webp)' })
  }

  const effects: Effects = { ...NO_EFFECTS }
  if (isObj(raw.effects)) {
    const e = raw.effects
    if (e.scene !== undefined) {
      if (e.scene === 'base' || SCENES.includes(e.scene as Fx)) effects.scene = e.scene as Effects['scene']
      else problems.push({ at: 'effects.scene', message: `should be "base" or one of ${SCENES.join(', ')}` })
    }
    for (const k of ['grain', 'scanlines', 'halftone', 'hex'] as const) effects[k] = num(e[k], 0, 1, 0)
    for (const k of ['wires', 'rain', 'trails', 'split', 'wipe', 'lcd'] as const) effects[k] = e[k] === true
    for (const k of Object.keys(e)) if (!(k in NO_EFFECTS)) problems.push({ at: `effects.${k}`, message: "isn't one of the effects (see docs/THEMES.md)" })
  }

  let mark: Pack['mark'] = null
  if (isObj(raw.mark)) {
    const c = raw.mark.color
    const color = c === 'accent' || c === 'ink' ? c : typeof c === 'string' && !checkValue(TOKEN.get('accent')!, c, supports) ? c.trim() : 'accent'
    if (c !== undefined && color === 'accent' && c !== 'accent') problems.push({ at: 'mark.color', message: 'should be "accent", "ink" or a colour' })
    mark = { color, glow: num(raw.mark.glow, 0, 1, 0) }
  }

  let css = ''
  if (raw.css !== undefined) {
    if (safeFile(raw.css) && raw.css.endsWith('.css')) css = raw.css
    else problems.push({ at: 'css', message: 'should name a .css file in the theme' })
  }

  const id = ID_RE.test(folderId) ? folderId : idFrom(folderId)
  if (typeof raw.id === 'string' && raw.id !== id) problems.push({ at: 'id', message: `says "${raw.id}" but the theme is "${id}"; using "${id}"` })
  return {
    pack: {
      id,
      name: str(raw.name, 60) || id,
      author: str(raw.author, 80),
      description: str(raw.description, 240),
      extends: base as Style,
      modes,
      accent,
      tokens,
      dark,
      light,
      fonts,
      background,
      effects,
      mark,
      css,
      studio: raw.studio,
    },
    problems,
  }
}

/** What a pack does, for code that asks (lib/themes StyleInfo): its base's, with its changes. */
export function packInfo(pack: Pack): StyleInfo {
  const base = styleInfo(pack.extends)
  const accent: StyleInfo['accent'] = 'color' in pack.accent ? 'own' : pack.accent.from === 'album' ? (base.accent === 'yours' ? 'yours' : 'cover') : base.accent
  const fx = pack.effects.scene === 'base' ? base.fx : pack.effects.scene
  return {
    ...base,
    name: pack.name,
    line: pack.description || `Made from ${base.name}.`,
    modes: pack.modes.length > 1,
    accent,
    // A theme's own surfaces replace the glass/frosted/solid choice.
    surfaces: false,
    albumBackground: !pack.background && base.albumBackground,
    lcd: base.lcd || pack.effects.lcd,
    fx,
  }
}

/** The tokens a pack sets in a mode, everything it writes merged (for previews and checks). */
export function tokensIn(pack: Pack, mode: Mode): Record<string, string> {
  const t = { ...pack.tokens, ...(mode === 'light' ? pack.light : pack.dark) }
  if ('color' in pack.accent) {
    t.accent ??= pack.accent.color
    t['accent-ink'] ??= pack.accent.color
    t['on-accent'] ??= readableOn(pack.accent.color) ?? '#ffffff'
  }
  return t
}

/** The font list for a slot: its typeface, then what the system has. */
export function fontStack(pack: Pack, slot: FontSlot): string | null {
  const f = pack.fonts[slot]
  if (!f) return null
  const family = f.startsWith('builtin:') ? f.slice(8) : faceName(pack.id, slot)
  return `"${family}", ${FALLBACK[slot]}`
}

/** The family name a pack's own font file is registered under. */
export const faceName = (id: string, slot: FontSlot) => `venuta-pack-${id}-${slot}`

/** The selector a pack's rules sit under: above every built-in theme rule (they go to five). */
export const scopeOf = (id: string) => `:root:root:root:root:root[data-pack="${id}"]`

const block = (sel: string, t: Record<string, string>) => {
  const body = Object.entries(t)
    .map(([k, v]) => `  --${k}: ${v};`)
    .join('\n')
  return body ? `${sel} {\n${body}\n}\n` : ''
}

/**
 * The CSS for a pack's values, its fonts, its mark: everything but its expert stylesheet and its
 * background picture (lib/packs.ts adds those, with the files' addresses).
 */
export function packCss(pack: Pack): string {
  const s = scopeOf(pack.id)
  const common = { ...pack.tokens }
  for (const slot of FONT_SLOTS) {
    const stack = fontStack(pack, slot)
    if (stack) common[`font-${slot}`] = stack
  }
  const accent: Record<string, string> = 'color' in pack.accent ? { accent: pack.accent.color, 'accent-ink': pack.accent.color, 'on-accent': readableOn(pack.accent.color) ?? '#ffffff' } : {}
  let css = block(s, { ...accent, ...common })
  css += block(`${s}[data-theme="dark"]`, pack.dark)
  css += block(`${s}[data-theme="light"]`, pack.light)
  if (pack.mark) {
    const c = pack.mark.color === 'accent' ? 'var(--accent)' : pack.mark.color === 'ink' ? 'var(--color-ink)' : pack.mark.color
    const glow = pack.mark.glow > 0 ? `drop-shadow(0 0 ${(0.1 + pack.mark.glow * 0.3).toFixed(2)}em color-mix(in oklab, ${c} ${Math.round(40 + pack.mark.glow * 50)}%, transparent))` : 'none'
    css += `${s} .brand-mark {\n  color: ${c};\n  filter: ${glow};\n}\n`
  }
  return css
}

/* ------------------------------------------------------------------------------------------ */
/* Expert stylesheets: every rule kept under its theme, nothing loaded from outside.           */
/* ------------------------------------------------------------------------------------------ */

/** Split a selector list at its top-level commas (not those inside :is(), :not()…). */
export function splitSelectors(list: string): string[] {
  const out: string[] = []
  let depth = 0
  let quote = ''
  let cur = ''
  for (const ch of list) {
    if (quote) {
      if (ch === quote) quote = ''
    } else if (ch === '"' || ch === "'") quote = ch
    else if (ch === '(' || ch === '[') depth++
    else if (ch === ')' || ch === ']') depth--
    else if (ch === ',' && depth === 0) {
      out.push(cur.trim())
      cur = ''
      continue
    }
    cur += ch
  }
  if (cur.trim()) out.push(cur.trim())
  return out
}

/**
 * A selector put under the theme's scope: `.rail` → `<scope> .rail`; `:root` or `html` at its
 * start become the scope itself (so `:root { --x }` sets values for this theme only).
 */
export function scopeSelector(sel: string, scope: string): string {
  const m = sel.match(/^(?::root|html)(?![\w-])/)
  if (m) return scope + sel.slice(m[0].length)
  if (/^(body|#root|main)\b/.test(sel)) return `${scope} ${sel}`
  return `${scope} ${sel}`
}

/**
 * A declaration's value made safe: url(…) only to the theme's own files (as `files` maps
 * them); anything else that could load or run something removes the declaration (null).
 */
export function cleanValue(value: string, files: (name: string) => string | null): string | null {
  if (/expression\s*\(|javascript:|@import|image-set\s*\(|-moz-binding|behavior\s*:/i.test(value)) return null
  let ok = true
  const out = value.replace(/url\(\s*(['"]?)([^'")]*)\1\s*\)/gi, (_all, _q, name: string) => {
    const mapped = safeFile(name) ? files(name) : null
    if (!mapped) ok = false
    return `url("${mapped}")`
  })
  return ok ? out : null
}

/** A keyframes name kept to its theme. */
export const keyframesName = (id: string, name: string) => `p-${id}-${name}`

/* ------------------------------------------------------------------------------------------ */
/* Colour: reading, contrast, and making a whole theme from a few choices (the Studio).        */
/* ------------------------------------------------------------------------------------------ */

export type RGBA = [number, number, number, number]

/** #rgb, #rrggbb, #rrggbbaa, rgb()/rgba() in either syntax; null for anything else. */
export function parseColor(c: string): RGBA | null {
  const s = c.trim().toLowerCase()
  let m = s.match(/^#([0-9a-f]{3,8})$/)
  if (m) {
    const h = m[1]
    if (h.length === 3 || h.length === 4) {
      const v = [...h].map((x) => parseInt(x + x, 16))
      return [v[0], v[1], v[2], h.length === 4 ? v[3] / 255 : 1]
    }
    if (h.length === 6 || h.length === 8) {
      const v = [0, 2, 4, 6].map((i) => parseInt(h.slice(i, i + 2), 16))
      return [v[0], v[1], v[2], h.length === 8 ? v[3] / 255 : 1]
    }
    return null
  }
  m = s.match(/^rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)\s*(?:[,/]\s*([\d.]+%?)\s*)?\)$/)
  if (m) {
    const a = m[4] === undefined ? 1 : m[4].endsWith('%') ? parseFloat(m[4]) / 100 : parseFloat(m[4])
    return [+m[1], +m[2], +m[3], a]
  }
  if (s === 'transparent') return [0, 0, 0, 0]
  return null
}

const lin = (v: number) => {
  const c = v / 255
  return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4
}
export const luminance = ([r, g, b]: RGBA) => 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)

export function contrast(a: RGBA, b: RGBA): number {
  const [l1, l2] = [luminance(a), luminance(b)].sort((x, y) => y - x)
  return (l1 + 0.05) / (l2 + 0.05)
}

/** `top` laid over `under` (the colour you see through a see-through panel). */
export function over(top: RGBA, under: RGBA): RGBA {
  const a = top[3]
  return [top[0] * a + under[0] * (1 - a), top[1] * a + under[1] * (1 - a), top[2] * a + under[2] * (1 - a), 1]
}

export const hex = ([r, g, b]: RGBA) => '#' + [r, g, b].map((v) => Math.round(Math.min(255, Math.max(0, v))).toString(16).padStart(2, '0')).join('')
export const rgba = ([r, g, b]: RGBA, a: number) => `rgb(${Math.round(r)} ${Math.round(g)} ${Math.round(b)} / ${+a.toFixed(3)})`

/** Black or white, whichever reads better on `c`. */
export function readableOn(c: string): string | null {
  const p = parseColor(c)
  if (!p) return null
  return contrast(p, [255, 255, 255, 1]) >= contrast(p, [0, 0, 0, 1]) ? '#ffffff' : '#0b0c10'
}

/** Mix two colours: t = 0 is a, 1 is b. */
export const mix = (a: RGBA, b: RGBA, t: number): RGBA => [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t, 1]

/** The Studio's simple choices: from these few, every value is made. */
export interface Simple {
  extends: Style
  mode: Mode
  background: string
  panels: string
  text: string
  /** A colour, or "album" for the music's. */
  accent: string
  /** How see-through panels are, 0 (solid) to 1 (clear). */
  glass: number
  /** Blur behind see-through things, in px. */
  blur: number
  /** Corners, 0 (sharp) to 1 (round). */
  round: number
  fontUi: string
  fontDisplay: string
  capitals: boolean
  /** A picture in the theme behind everything (its file name), or ''. */
  image: string
  imageDim: number
  /** A second gradient colour over the window, or ''. */
  gradient: string
  effects: { grain: number; scanlines: number; halftone: boolean; trails: boolean }
  markColor: 'accent' | 'ink'
  markGlow: number
}

export const SIMPLE_DEFAULT: Simple = {
  extends: 'venuta',
  mode: 'dark',
  background: '#14101c',
  panels: '#241c30',
  text: '#f2ecf6',
  accent: '#ff7aa8',
  glass: 0.55,
  blur: 24,
  round: 0.6,
  fontUi: 'Sora Variable',
  fontDisplay: 'Sora Variable',
  capitals: false,
  image: '',
  imageDim: 0.35,
  gradient: '',
  effects: { grain: 0, scanlines: 0, halftone: false, trails: false },
  markColor: 'accent',
  markGlow: 0.4,
}

/**
 * Every value of a theme from the simple choices. Panels, bars, the rail and the player are
 * the panel colour at different strengths over the background; outlines and highlights are the
 * text colour, faint; shadows deepen in the dark and soften in the light; text is moved toward
 * black or white only as far as it needs to read on everything it sits on.
 */
export function derive(s: Simple): { tokens: Record<string, string>; fixedText: boolean } {
  const bg = parseColor(s.background) ?? [20, 16, 28, 1]
  const panel = parseColor(s.panels) ?? mix(bg, [255, 255, 255, 1], 0.08)
  let ink = parseColor(s.text) ?? [242, 236, 246, 1]
  const light = s.mode === 'light'
  const solidity = 1 - Math.min(1, Math.max(0, s.glass))
  const a = (base: number) => Math.min(1, Math.max(0.04, base + solidity * (1 - base)))

  // Text that reads on the page, the panels, the bars and the player bar.
  const surfaces = [bg, over([...panel.slice(0, 3), a(0.5)] as RGBA, bg), over([...panel.slice(0, 3), a(0.66)] as RGBA, bg)]
  let fixedText = false
  const target = light ? [0, 0, 0, 1] as RGBA : [255, 255, 255, 1] as RGBA
  for (let i = 0; i < 20 && Math.min(...surfaces.map((x) => contrast(ink, x))) < 4.5; i++) {
    ink = mix(ink, target, 0.15)
    fixedText = true
  }
  const inkHex = hex(ink)
  const faint = (k: number) => rgba(ink, k)
  const blur = (px: number) => (px <= 0 ? 'none' : `blur(${Math.round(px)}px) saturate(1.5)`)
  const shadowK = light ? 0.12 : 0.5
  const radius = (max: number) => `${Math.round(2 + s.round * (max - 2))}px`
  const display = BUILTIN_FONTS.includes(s.fontDisplay) ? s.fontDisplay : 'Sora Variable'
  const ui = BUILTIN_FONTS.includes(s.fontUi) ? s.fontUi : 'Sora Variable'

  const tokens: Record<string, string> = {
    ink: inkHex,
    'app-bg': hex(bg),
    solid: hex(bg),
    'app-image': s.gradient && parseColor(s.gradient) ? `radial-gradient(120% 90% at 85% 0%, ${rgba(parseColor(s.gradient)!, 0.55)}, transparent 65%)` : 'none',
    scrim: rgba(bg, 0.5),
    vignette: light ? 'none' : 'radial-gradient(120% 90% at 50% 35%, transparent 55%, rgb(0 0 0 / 0.45))',
    glass: rgba(panel, a(0.3) * 0.6),
    edge: faint(light ? 0.12 : 0.09),
    'edge-strong': faint(light ? 0.2 : 0.15),
    highlight: faint(light ? 0.5 : 0.08),
    shadow: `0 8px 24px -6px rgb(0 0 0 / ${shadowK})`,
    'shadow-e3': `0 24px 64px -12px rgb(0 0 0 / ${Math.min(0.7, shadowK * 1.3)})`,
    blur: blur(s.blur),
    popover: rgba(mix(panel, bg, 0.2), a(0.82)),
    'blur-over': blur(s.blur * 1.4),
    bar: rgba(bg, a(0.6)),
    'bar-blur': blur(s.blur * 0.8),
    'bar-edge': faint(0.07),
    'rail-bg': rgba(panel, a(0.5)),
    'rail-blur': blur(s.blur),
    'rail-edge': faint(0.08),
    'rail-shadow': `0 8px 24px -6px rgb(0 0 0 / ${shadowK * 0.9})`,
    'pill-bg': faint(light ? 0.08 : 0.1),
    'pill-ink': inkHex,
    'pill-shadow': `inset 0 1px 0 ${faint(0.12)}`,
    'np-bg': rgba(panel, a(0.66)),
    'np-blur': blur(s.blur * 1.2),
    'np-edge': faint(0.09),
    'np-shadow': `0 8px 24px -6px rgb(0 0 0 / ${shadowK})`,
    'r-rail': radius(24),
    'r-np': radius(24),
    'font-ui': `"${ui}", system-ui, sans-serif`,
    'font-display': `"${display}", system-ui, sans-serif`,
    'title-case': s.capitals ? 'uppercase' : 'none',
    'label-case': s.capitals ? 'uppercase' : 'none',
    'title-tracking': s.capitals ? '0.02em' : '-0.03em',
    'backdrop-opacity': s.image ? '0' : String(Math.round((0.25 + s.glass * 0.6) * 100) / 100),
  }
  if (s.accent !== 'album') {
    const acc = parseColor(s.accent)
    if (acc) {
      tokens.accent = hex(acc)
      tokens['on-accent'] = readableOn(hex(acc)) ?? '#ffffff'
      // As text, the accent moves toward the ink until it reads on the page.
      let ai = acc
      for (let i = 0; i < 12 && contrast(ai, bg) < 3; i++) ai = mix(ai, ink, 0.12)
      tokens['accent-ink'] = hex(ai)
      tokens['accent-2'] = hex(acc)
    }
  }
  return { tokens, fixedText }
}

/** A theme.json made from the Studio's simple choices (the choices kept in it, to edit later). */
export function simpleToJson(s: Simple, meta: { id: string; name: string; author: string; description: string }): Record<string, unknown> {
  const { tokens } = derive(s)
  const { accent: _a, 'accent-ink': _ai, 'on-accent': _oa, ...rest } = tokens
  return {
    format: FORMAT,
    id: meta.id,
    name: meta.name,
    ...(meta.author ? { author: meta.author } : {}),
    ...(meta.description ? { description: meta.description } : {}),
    extends: s.extends,
    modes: [s.mode],
    accent: s.accent === 'album' ? { from: 'album' } : { color: hex(parseColor(s.accent) ?? [255, 122, 168, 1]) },
    tokens: s.accent === 'album' ? rest : { ...rest, 'accent-ink': tokens['accent-ink'], 'on-accent': tokens['on-accent'], 'accent-2': tokens['accent-2'] },
    ...(s.image ? { background: { image: s.image, dim: s.imageDim, blur: 0 } } : {}),
    effects: {
      scene: 'base',
      grain: s.effects.grain,
      scanlines: s.effects.scanlines,
      halftone: s.effects.halftone ? 0.2 : 0,
      trails: s.effects.trails,
    },
    mark: { color: s.markColor, glow: s.markGlow },
    studio: { simple: s },
  }
}

/** Readability, the way the Studio checks it: what reads badly, and how badly. */
export interface Readability {
  what: string
  ratio: number
  needs: number
}

export function readability(t: Record<string, string>, fallback: Record<string, string> = {}): Readability[] {
  const get = (k: string) => parseColor(t[k] ?? fallback[k] ?? '')
  const bg = get('app-bg')
  const ink = get('ink')
  if (!bg || !ink) return []
  const out: Readability[] = []
  const check = (what: string, fg: RGBA | null, under: RGBA | null, needs: number) => {
    if (!fg || !under) return
    const r = contrast(fg, under[3] < 1 ? over(under, bg) : under)
    if (r < needs) out.push({ what, ratio: Math.round(r * 10) / 10, needs })
  }
  check('Text on the page', ink, bg, 4.5)
  check('Text on panels', ink, get('glass'), 4.5)
  check('Text on the player bar', ink, get('np-bg'), 4.5)
  check('Text on the rail', ink, get('rail-bg'), 4.5)
  check('Text in menus', ink, get('popover'), 4.5)
  check('Text on the key colour', get('on-accent'), get('accent'), 3)
  check('Key colour as text', get('accent-ink') ?? get('accent'), bg, 3)
  return out
}

/** Simple choices from one colour (an album's): its hue deep for the page, lighter for panels. */
export function simpleFromColour(accent: string, mode: Mode, base: Style, second = ''): Simple {
  const a = parseColor(accent) ?? [255, 122, 168, 1]
  const black: RGBA = [8, 8, 12, 1]
  const white: RGBA = [250, 250, 252, 1]
  const dark = mode === 'dark'
  const s2 = parseColor(second)
  return {
    ...SIMPLE_DEFAULT,
    extends: base,
    mode,
    background: hex(mix(a, dark ? black : white, dark ? 0.86 : 0.9)),
    panels: hex(mix(a, dark ? black : white, dark ? 0.74 : 0.8)),
    text: hex(mix(a, dark ? white : black, 0.9)),
    accent: hex(a),
    gradient: s2 ? hex(s2) : '',
  }
}

/** Simple choices that come close to a theme as it shows (its values read from the page). */
export function simpleFromTokens(t: Record<string, string>, base: Style, mode: Mode): Simple {
  const c = (k: string, d: string) => {
    const p = parseColor(t[k] ?? '')
    return p ? hex(p) : d
  }
  const np = parseColor(t['np-bg'] ?? '')
  const blur = /blur\(\s*([\d.]+)px/.exec(t['np-blur'] ?? '')
  const r = /([\d.]+)px/.exec(t['r-np'] ?? '')
  const family = (k: string) => {
    const f = /^\s*"?([^",]+)"?/.exec(t[k] ?? '')?.[1]?.trim() ?? ''
    return BUILTIN_FONTS.includes(f) ? f : SIMPLE_DEFAULT.fontUi
  }
  return {
    ...SIMPLE_DEFAULT,
    extends: base,
    mode,
    background: c('app-bg', SIMPLE_DEFAULT.background),
    panels: np ? hex(np) : SIMPLE_DEFAULT.panels,
    text: c('ink', SIMPLE_DEFAULT.text),
    accent: c('accent', SIMPLE_DEFAULT.accent),
    glass: np ? Math.round((1 - np[3]) * 100) / 100 : SIMPLE_DEFAULT.glass,
    blur: blur ? Math.round(Number(blur[1]) / 1.2) : 0,
    round: r ? Math.min(1, Math.max(0, (Number(r[1]) - 2) / 22)) : SIMPLE_DEFAULT.round,
    fontUi: family('font-ui'),
    fontDisplay: family('font-display'),
    capitals: (t['title-case'] ?? '').trim() === 'uppercase',
  }
}
