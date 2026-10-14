// node --test tests/   (Node runs the TypeScript as is)
import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  cleanValue,
  contrast,
  derive,
  idFrom,
  packCss,
  packInfo,
  parseColor,
  readability,
  readPack,
  scopeOf,
  scopeSelector,
  SIMPLE_DEFAULT,
  simpleToJson,
  splitSelectors,
  tokensIn,
  TOKENS,
  type Supports,
} from '../src/lib/packFormat.ts'
import { STYLES } from '../src/lib/themes.ts'

// The browser's CSS.supports stands in as: colours are colours, the word "bogus" is never valid.
const supports: Supports = (prop, v) => {
  if (/bogus/.test(v)) return false
  if (prop === 'color') return !!parseColor(v) || /^var\(--[a-z-]+\)$/.test(v) || /^[a-z]+$/.test(v) || /^(oklch|color-mix|hsl)\(/.test(v)
  return true
}

const json = (o: unknown) => JSON.stringify(o)

test('a small theme reads, keeps only what it writes, and inherits the rest', () => {
  const { pack, problems } = readPack(
    json({ format: 1, id: 'dusk', name: 'Dusk', extends: 'neo-tokyo', tokens: { 'app-bg': '#140c1c', '--ink': '#f4e9f2' } }),
    'dusk',
    supports,
  )
  assert.deepEqual(problems, [])
  assert.ok(pack)
  assert.equal(pack.extends, 'neo-tokyo')
  assert.deepEqual(pack.tokens, { 'app-bg': '#140c1c', ink: '#f4e9f2' })
  // It behaves like Neo-Tokyo: its seek bar, its scene.
  const info = packInfo(pack)
  assert.equal(info.seekBar, 'trail')
  assert.equal(info.fx, 'neo-tokyo')
  assert.equal(info.name, 'Dusk')
  assert.equal(info.surfaces, false)
})

test('a broken theme never loads half-understood: only fatal problems refuse it', () => {
  assert.equal(readPack('{not json', 'x', supports).pack, null)
  assert.equal(readPack('[]', 'x', supports).pack, null)
  const newer = readPack(json({ format: 2 }), 'x', supports)
  assert.equal(newer.pack, null)
  assert.match(newer.problems[0].message, /newer Venuta/)
  const nobase = readPack(json({ extends: 'winamp' }), 'x', supports)
  assert.equal(nobase.pack, null)
  assert.ok(nobase.problems.some((p) => p.fatal && p.at === 'extends'))
})

test('bad values are left out and reported, the good ones kept', () => {
  const { pack, problems } = readPack(
    json({
      extends: 'venuta',
      tokens: {
        ink: 'bogus',
        glass: 'rgb(1 2 3 / .5)',
        nope: '#fff',
        'app-image': 'url(https://evil.example/x.png)',
        edge: 'red; } body { display: none',
        'title-case': 'shouting',
        'backdrop-opacity': '7',
        popover: 'var(--secret)',
        bar: 'var(--glass)',
      },
      wat: 1,
    }),
    'x',
    supports,
  )
  assert.ok(pack)
  assert.deepEqual(pack.tokens, { glass: 'rgb(1 2 3 / .5)', bar: 'var(--glass)' })
  const at = problems.map((p) => p.at).sort()
  assert.deepEqual(at, ['tokens.app-image', 'tokens.backdrop-opacity', 'tokens.edge', 'tokens.ink', 'tokens.nope', 'tokens.popover', 'tokens.title-case', 'wat'])
  assert.ok(problems.every((p) => !p.fatal))
})

test('files a theme names stay inside it', () => {
  const { pack, problems } = readPack(
    json({
      fonts: { ui: '../../Windows/Fonts/x.ttf', display: 'builtin:Inter Variable', readout: 'fonts/Mono.woff2', title: 'x.woff2' },
      background: { image: 'C:/Users/x.jpg' },
      css: '../x.css',
    }),
    'x',
    supports,
  )
  assert.ok(pack)
  assert.deepEqual(pack.fonts, { display: 'builtin:Inter Variable', readout: 'fonts/Mono.woff2' })
  assert.equal(pack.background, null)
  assert.equal(pack.css, '')
  assert.deepEqual(problems.map((p) => p.at).sort(), ['background.image', 'css', 'fonts.title', 'fonts.ui'])
})

test("a pack's rules outrank every built-in theme rule and stay under its own name", () => {
  const { pack } = readPack(json({ tokens: { ink: '#fff' }, light: { tokens: { ink: '#111' } }, modes: ['dark', 'light'] }), 'x', supports)
  const css = packCss(pack!)
  assert.match(css, /:root:root:root:root:root\[data-pack="x"\] \{\n  --ink: #fff;/)
  assert.match(css, /\[data-pack="x"\]\[data-theme="light"\] \{\n  --ink: #111;/)
  assert.doesNotMatch(css, /data-theme="dark"/, 'no empty blocks')
})

test('a fixed accent brings readable text on it', () => {
  const { pack } = readPack(json({ accent: { color: '#ffe066' } }), 'x', supports)
  const t = tokensIn(pack!, 'dark')
  assert.equal(t.accent, '#ffe066')
  assert.equal(t['on-accent'], '#0b0c10')
  assert.equal(packInfo(pack!).accent, 'own')
  const album = readPack(json({ extends: 'neo-tokyo', accent: { from: 'album' } }), 'x', supports).pack!
  assert.equal(packInfo(album).accent, 'cover')
  const lumen = readPack(json({ accent: { from: 'album' } }), 'x', supports).pack!
  assert.equal(packInfo(lumen).accent, 'yours', 'on Lumen, the album is already what "yours" follows')
})

test('expert CSS: selectors are scoped, outside files and code removed', () => {
  assert.deepEqual(splitSelectors('.a, :is(.b, .c) .d,[x="1,2"]'), ['.a', ':is(.b, .c) .d', '[x="1,2"]'])
  const s = scopeOf('dusk')
  assert.equal(scopeSelector(':root', s), s)
  assert.equal(scopeSelector(':root .rail', s), `${s} .rail`)
  assert.equal(scopeSelector('html[data-theme="light"] .np', s), `${s}[data-theme="light"] .np`)
  assert.equal(scopeSelector('.rail', s), `${s} .rail`)
  assert.equal(scopeSelector(':root-ish', s), `${s} :root-ish`)
  const files = (n: string) => (n === 'art/a.png' ? 'blob:venuta/1' : null)
  assert.equal(cleanValue('url("art/a.png") center / cover', files), 'url("blob:venuta/1") center / cover')
  assert.equal(cleanValue("url('https://evil.example/a.png')", files), null)
  assert.equal(cleanValue('url(../../a.png)', files), null)
  assert.equal(cleanValue('url(art/missing.png)', files), null)
  assert.equal(cleanValue('expression(alert(1))', files), null)
  assert.equal(cleanValue('red', files), 'red')
})

test('the Studio makes every value from a few choices, readable', () => {
  const { tokens } = derive(SIMPLE_DEFAULT)
  for (const k of ['app-bg', 'glass', 'np-bg', 'rail-bg', 'popover', 'ink', 'accent', 'on-accent', 'r-rail', 'font-ui']) assert.ok(tokens[k], k)
  // Everything it makes passes the theme checks.
  const spec = new Map(TOKENS.map((t) => [t.name, t]))
  for (const k of Object.keys(tokens)) assert.ok(spec.has(k), `${k} is a real value`)
  assert.deepEqual(readability(tokens), [])

  // Grey text on grey: moved until it reads.
  const dull = derive({ ...SIMPLE_DEFAULT, background: '#555555', panels: '#5a5a5a', text: '#777777' })
  assert.equal(dull.fixedText, true)
  assert.ok(contrast(parseColor(dull.tokens.ink)!, parseColor('#555555')!) >= 4.5)

  // Light mode with a pale accent: the accent as text is darkened to read.
  const pale = derive({ ...SIMPLE_DEFAULT, mode: 'light', background: '#fbf8f2', panels: '#ffffff', text: '#1b1b1f', accent: '#ffd84d' })
  assert.deepEqual(readability(pale.tokens), [])
})

test('a Studio theme round-trips through theme.json', () => {
  const j = simpleToJson({ ...SIMPLE_DEFAULT, extends: 'layer-zero', accent: 'album' }, { id: 'calm', name: 'Calm', author: '', description: '' })
  const { pack, problems } = readPack(JSON.stringify(j), 'calm', supports)
  assert.deepEqual(problems, [])
  assert.equal(pack!.extends, 'layer-zero')
  assert.deepEqual(pack!.accent, { from: 'album' })
  assert.deepEqual((pack!.studio as { simple: { extends: string } }).simple.extends, 'layer-zero')
})

test('names become ids that are safe everywhere', () => {
  assert.equal(idFrom('Midnight Sakura'), 'midnight-sakura')
  assert.equal(idFrom('  Café — Noir!! '), 'cafe-noir')
  assert.equal(idFrom('???'), 'theme')
})

test('every built-in theme can be a base', () => {
  for (const s of STYLES) assert.ok(readPack(json({ extends: s.id }), 'x', supports).pack, s.id)
})

test('the themes that come with Venuta load cleanly and read well', async () => {
  const { readFileSync } = await import('node:fs')
  const dir = new URL('../public/theme-packs/', import.meta.url)
  const ids = JSON.parse(readFileSync(new URL('index.json', dir), 'utf8')) as string[]
  assert.ok(ids.length >= 7)
  for (const id of ids) {
    const { pack, problems } = readPack(readFileSync(new URL(`${id}/theme.json`, dir), 'utf8'), id, supports)
    assert.ok(pack, id)
    assert.deepEqual(problems, [], id)
    for (const mode of pack.modes) {
      const base = STYLES.find((s) => s.id === pack.extends)!
      assert.ok(base, id)
      assert.deepEqual(readability(tokensIn(pack, mode)), [], `${id} (${mode})`)
    }
  }
})

test('a theme can start from an album colour or from the theme on screen', async () => {
  const { simpleFromColour, simpleFromTokens } = await import('../src/lib/packFormat.ts')
  const s = simpleFromColour('#2f8fd8', 'dark', 'venuta', '#e0a030')
  assert.equal(s.accent, '#2f8fd8')
  assert.equal(s.gradient, '#e0a030')
  assert.ok(luminanceOf(s.background) < 0.03, 'a deep page')
  assert.deepEqual(readability(derive(s).tokens), [])
  const l = simpleFromColour('#2f8fd8', 'light', 'orchard')
  assert.deepEqual(readability(derive(l).tokens), [])

  const read = simpleFromTokens(
    { 'app-bg': 'rgb(11 12 16 / 0.35)', 'np-bg': 'rgb(22 24 31 / 0.66)', 'np-blur': 'blur(32px) saturate(1.8)', ink: '#f3f4f7', accent: '#8185eb', 'r-np': '22px', 'font-ui': '"Sora Variable", "Segoe UI Variable", system-ui', 'title-case': 'none' },
    'venuta',
    'dark',
  )
  assert.equal(read.panels, '#16181f')
  assert.equal(read.glass, 0.34)
  assert.equal(read.blur, 27)
  assert.equal(read.fontUi, 'Sora Variable')
  assert.equal(read.capitals, false)
})

function luminanceOf(c: string) {
  const p = parseColor(c)!
  const lin = (v: number) => {
    const x = v / 255
    return x <= 0.03928 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4
  }
  return 0.2126 * lin(p[0]) + 0.7152 * lin(p[1]) + 0.0722 * lin(p[2])
}
