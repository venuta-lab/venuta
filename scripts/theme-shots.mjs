// Every built-in theme on the main screens, captured from a test copy, so a change to how
// themes work can be proven to change nothing (or exactly what was meant).
//
//   node scripts/theme-shots.mjs take <folder>          capture every theme and screen
//   node scripts/theme-shots.mjs compare <before> <after>  count differing pixels per shot
//
// Needs a debug test copy running with its inspector on port 9333 (or VENUTA_CDP_PORT) and a
// song in its queue (the screens show it). Run it as CONTRIBUTING.md says, adding
//   WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333
// It mutes the player, pauses it, turns motion off for the capture (without saving any
// preference), and puts everything back at the end.

import { mkdirSync, readFileSync, readdirSync, writeFileSync, existsSync } from 'node:fs'
import { join } from 'node:path'

const PORT = process.env.VENUTA_CDP_PORT ?? '9333'

/** Fewer differing pixels than this is noise (random grain, a pixel of text smoothing), listed
 *  but not a failure: about 0.01% of the screen. */
const NOISE_PX = 200

/** Each theme, in the modes it has. */
const THEMES = [
  ['venuta', 'dark'], ['venuta', 'light'],
  ['green-room', 'dark'],
  ['orchard', 'dark'], ['orchard', 'light'],
  ['redline', 'dark'],
  ['glasshouse', 'dark'],
  ['neo-tokyo', 'dark'],
  ['thermoptic', 'dark'],
  ['sync-ratio', 'dark'],
  ['layer-zero', 'dark'],
  ['bounty', 'light'], ['bounty', 'dark'],
  ['skin99', 'dark'],
]

/** Each screen: how to get there from anywhere (a script run in the page). */
const SCREENS = {
  music: `await goTo('Music'); await tab('Home')`,
  album: `await openNowPlaying('Lyrics'); await __shots.sleep(400); await __shots.click('Go to album', () => document.querySelector('[aria-label="Now Playing"]') ?? document)`,
  playlists: `await goTo('Music'); await tab('Playlists')`,
  nowplaying: `await openNowPlaying('Sound')`,
  health: `await goTo('Health')`,
  settings: `await goTo('Settings'); await tab('Look and feel')`,
}

// Helpers the screen scripts use, defined in the page once. Each waits for what it clicks to be
// there (a screen mounts after the click that opens it), and a tab until it is the chosen one:
// a click that lands too early would leave the screen on whatever tab it remembered.
const HELPERS = `
  window.__shots = {
    sleep: (ms) => new Promise((r) => setTimeout(r, ms)),
    closeAll() { for (let i = 0; i < 4; i++) document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })) },
    find(text, scope = () => document) { return [...scope().querySelectorAll('button,[role=tab],[role=radio]')].find((e) => e.textContent.trim() === text || e.getAttribute('aria-label') === text) },
    async click(text, scope) {
      for (let i = 0; i < 40; i++) {
        const b = __shots.find(text, scope)
        if (b) { b.click(); return b }
        await __shots.sleep(100)
      }
      throw new Error('Not found: ' + text)
    },
  }
  const chosen = (b) => b.getAttribute('aria-selected') === 'true' || b.getAttribute('aria-checked') === 'true'
  window.goTo = async (place) => { __shots.closeAll(); await __shots.click(place, () => document.querySelector('nav') ?? document); await __shots.sleep(200) }
  window.tab = async (name) => {
    for (let i = 0; i < 10; i++) {
      const b = await __shots.click(name)
      await __shots.sleep(150)
      if (!b.hasAttribute('aria-selected') && !b.hasAttribute('aria-checked')) return
      if (chosen(__shots.find(name) ?? b)) return
    }
    throw new Error('Tab never chosen: ' + name)
  }
  window.openNowPlaying = async (panel) => { __shots.closeAll(); document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'l', bubbles: true })); await __shots.sleep(300); await tab(panel) }
`

async function connect() {
  const list = await (await fetch(`http://127.0.0.1:${PORT}/json`)).json()
  const page = list.find((t) => t.type === 'page' && !t.url.includes('#'))
  if (!page) throw new Error(`No Venuta window on port ${PORT}: start a debug test copy first.`)
  const ws = new WebSocket(page.webSocketDebuggerUrl)
  let id = 0
  const pending = new Map()
  ws.onmessage = (m) => {
    const d = JSON.parse(m.data)
    if (d.id && pending.has(d.id)) pending.get(d.id)(d)
  }
  await new Promise((r) => (ws.onopen = r))
  const send = (method, params = {}) =>
    new Promise((r) => {
      const i = ++id
      pending.set(i, r)
      ws.send(JSON.stringify({ id: i, method, params }))
    })
  const run = async (expression) => {
    const r = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true })
    if (r.result?.exceptionDetails) throw new Error(r.result.exceptionDetails.exception?.description ?? 'page error')
    return r.result?.result?.value
  }
  return { ws, send, run }
}

async function take(dir) {
  mkdirSync(dir, { recursive: true })
  const { ws, send, run } = await connect()
  await run(HELPERS)
  // Silent and still: muted, paused, and the look as it was remembered for the end.
  const before = await run(`(async () => {
    if (!window.__venutaPrefs) throw new Error('Not a debug build: the theme hook is missing.')
    const I = window.__TAURI_INTERNALS__.invoke
    await I('player_volume', { volume: 0 })
    if (document.querySelector('button[aria-label^="Pause ("]')) await I('player_toggle')
    const p = window.__venutaPrefs.get()
    return { style: p.style, theme: p.theme, motion: p.motion }
  })()`)
  let n = 0
  const unstable = []
  for (const [style, theme] of THEMES) {
    await run(`(async () => { window.__venutaPrefs.try({ style: '${style}', theme: '${theme}', motion: 'reduced' }); await __shots.sleep(700) })()`)
    for (const [name, go] of Object.entries(SCREENS)) {
      await run(`(async () => { ${go}; await __shots.sleep(1300);
        const b = [...document.querySelectorAll('span,div')].find((e) => e.textContent === 'Test copy' && e.children.length === 0); if (b) b.style.visibility = 'hidden' })()`)
      // Settled: two captures half a second apart are the same (covers loaded, fades done).
      let last = null
      let settled = false
      for (let tries = 0; tries < 16 && !settled; tries++) {
        const shot = (await send('Page.captureScreenshot', { format: 'png' })).result.data
        settled = shot === last
        last = shot
        if (!settled) await new Promise((r) => setTimeout(r, 500))
      }
      writeFileSync(join(dir, `${style}-${theme}-${name}.png`), Buffer.from(last, 'base64'))
      if (!settled) unstable.push(`${style}-${theme}-${name}`)
      n++
      process.stdout.write(`\r${n} shots`)
    }
  }
  await run(`(async () => { __shots.closeAll(); window.__venutaPrefs.try(${JSON.stringify(before)}) })()`)
  ws.close()
  console.log(`\nSaved ${n} shots in ${dir}`)
  if (unstable.length) console.log(`Never settled (something keeps moving): ${unstable.join(', ')}`)
}

// Compared in the page (it decodes PNGs; Node alone doesn't), shot by shot.
async function compare(a, b) {
  const { ws, run } = await connect()
  const names = readdirSync(a).filter((f) => f.endsWith('.png'))
  const out = join(b, 'diff')
  let changed = 0
  for (const name of names) {
    if (!existsSync(join(b, name))) {
      console.log(`missing  ${name}`)
      changed++
      continue
    }
    const pa = readFileSync(join(a, name))
    const pb = readFileSync(join(b, name))
    if (pa.equals(pb)) continue
    const r = await run(`(async () => {
      const load = (b64) => new Promise((res) => { const i = new Image(); i.onload = () => res(i); i.src = 'data:image/png;base64,' + b64 })
      const [x, y] = await Promise.all([load('${pa.toString('base64')}'), load('${pb.toString('base64')}')])
      if (x.width !== y.width || x.height !== y.height) return { size: true }
      const c = (img) => { const k = document.createElement('canvas'); k.width = img.width; k.height = img.height; const g = k.getContext('2d'); g.drawImage(img, 0, 0); return [k, g.getImageData(0, 0, k.width, k.height).data] }
      const [ka, da] = c(x), [, db] = c(y)
      const g = ka.getContext('2d'); g.globalAlpha = 0.25; g.fillStyle = '#000'; g.fillRect(0, 0, ka.width, ka.height); g.globalAlpha = 1
      let n = 0
      for (let i = 0; i < da.length; i += 4) {
        if (Math.abs(da[i] - db[i]) + Math.abs(da[i + 1] - db[i + 1]) + Math.abs(da[i + 2] - db[i + 2]) > 24) {
          n++; const p = i / 4; g.fillStyle = '#ff0050'; g.fillRect(p % ka.width, Math.floor(p / ka.width), 1, 1)
        }
      }
      return { n, total: da.length / 4, diff: n ? ka.toDataURL('image/png') : null }
    })()`)
    if (r.size) {
      console.log(`size     ${name}`)
      changed++
    } else if (r.n > 0 && r.n < NOISE_PX) {
      console.log(`noise    ${name}  ${r.n} px`)
    } else if (r.n > 0) {
      mkdirSync(out, { recursive: true })
      writeFileSync(join(out, name), Buffer.from(r.diff.split(',')[1], 'base64'))
      console.log(`changed  ${name}  ${r.n} px (${((r.n / r.total) * 100).toFixed(3)}%)`)
      changed++
    }
  }
  ws.close()
  console.log(changed ? `${changed} of ${names.length} shots differ (diffs in ${out})` : `No differences beyond noise in ${names.length} shots.`)
  process.exitCode = changed ? 1 : 0
}

const [cmd, a, b] = process.argv.slice(2)
if (cmd === 'take' && a) await take(a)
else if (cmd === 'compare' && a && b) await compare(a, b)
else console.log('Usage: node scripts/theme-shots.mjs take <folder> | compare <before> <after>')
