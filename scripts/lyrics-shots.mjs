// Every lyrics view with pronunciation and translation, captured in headless Chrome from the
// Vite dev server's preview page (app/lyrics-preview.html, fixture songs; never the app, never a
// window on screen), checked for clipping and overlap, and the lyrics clock measured.
//
//   pnpm --dir app exec vite --port 5199 --strictPort      (in another shell)
//   node scripts/lyrics-shots.mjs [--url http://localhost:5199] [--out docs/screenshots/lyrics-second-line] [--only bigscreen-]
//
// Chrome runs headless with a profile of its own in the temp folder, deleted at the end, and is
// driven over a pipe (no port opened). CHROME may name its exe.

import { spawn } from 'node:child_process'
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'

const arg = (name, or) => {
  const i = process.argv.indexOf(`--${name}`)
  return i > 0 ? process.argv[i + 1] : or
}
const BASE = arg('url', 'http://localhost:5199')
const OUT = resolve(arg('out', 'docs/screenshots/lyrics-second-line'))
// Only the shots whose name has this in it (the clock is measured on a full run only).
const ONLY = arg('only', '')
const CHROME = [process.env.CHROME, 'C:/Program Files/Google/Chrome/Application/chrome.exe', 'C:/Program Files (x86)/Google/Chrome/Application/chrome.exe', '/usr/bin/google-chrome'].find((p) => p && existsSync(p))
if (!CHROME) throw new Error('Chrome not found: set CHROME')

// --- The shots ------------------------------------------------------------------------------
// [name, query, width, height, { awake }]. All rows on unless said; the big screen and the strip
// at Huge. `awake`: the mouse has just moved (the big screen's song bar shows its buttons).
const SHOTS = []
for (const theme of ['dark', 'light']) {
  const t = `theme=${theme}`
  SHOTS.push(
    [`nowplaying-hindi-small-${theme}`, `view=nowplaying&song=hindi&${t}&line=2`, 1100, 700],
    [`nowplaying-hindi-large-${theme}`, `view=nowplaying&song=hindi&${t}&line=2`, 1920, 1080],
    [`bigscreen-hindi-small-${theme}`, `view=stage&song=hindi&${t}&line=2&size=xl`, 960, 540],
    [`bigscreen-hindi-large-${theme}`, `view=stage&song=hindi&${t}&line=2&size=xl`, 1920, 1080],
    // The strip as it grows for all rows from its first 920 x 200 (lib/lyricRows.ts: 296 high).
    [`desktop-hindi-small-${theme}`, `view=overlay&song=hindi&${t}&line=2&size=m`, 520, 296],
    [`desktop-hindi-large-${theme}`, `view=overlay&song=hindi&${t}&line=2&size=xl`, 1400, 296],
    [`mini-bar-hindi-${theme}`, `view=mini&song=hindi&${t}&line=2`, 380, 132],
    [`mini-card-hindi-${theme}`, `view=mini&song=hindi&${t}&line=2`, 300, 400],
    [`settings-lyrics-${theme}`, `view=settings&song=hindi&${t}`, 900, 1000],
  )
}
SHOTS.push(
  ['bigscreen-mixed-english-line-large-dark', 'view=stage&song=mixed&theme=dark&line=1&size=xl', 1920, 1080],
  ['bigscreen-japanese-small-dark', 'view=stage&song=japanese&theme=dark&line=3&size=xl', 960, 540],
  ['nowplaying-japanese-large-dark', 'view=nowplaying&song=japanese&theme=dark&line=1&dict=downloading', 1920, 1080],
  ['nowplaying-plain-small-light', 'view=nowplaying&song=plain&theme=light&line=1', 1100, 700],
  ['desktop-spanish-smallest-dark', 'view=overlay&song=spanish&theme=dark&line=2&size=xl', 360, 130],
  ['mini-card-mixed-light', 'view=mini&song=mixed&theme=light&line=0', 300, 400],
  ['bigscreen-hindi-outline-large-light', 'view=stage&song=hindi&theme=light&line=2&size=xl&look=outline', 1920, 1080],
)
// Lines with nowhere to wrap (fixture song unbroken: one long word, words joined by no-break
// spaces, a Thai line): the desktop strip makes them smaller until they fit across, in every look
// and every theme's fonts, and breaks inside a word only at its smallest size.
for (const look of ['karaoke', 'clean', 'glow', 'outline', 'serif', 'theme'])
  for (const line of [0, 1, 2]) SHOTS.push([`desktop-unbroken-${look}-line${line}-dark`, `view=overlay&song=unbroken&theme=dark&line=${line}&size=xl&look=${look}`, 520, 296])
for (const style of ['venuta', 'green-room', 'orchard', 'redline', 'glasshouse', 'neo-tokyo', 'thermoptic', 'sync-ratio', 'layer-zero', 'bounty', 'skin99'])
  SHOTS.push([`desktop-unbroken-theme-${style}-dark`, `view=overlay&song=unbroken&theme=dark&line=0&size=xl&look=theme&style=${style}`, 520, 296])
SHOTS.push(
  ['desktop-unbroken-smallest-dark', 'view=overlay&song=unbroken&theme=dark&line=0&size=xl', 360, 130],
  ['desktop-unbroken-joined-smallest-light', 'view=overlay&song=unbroken&theme=light&line=1&size=xl', 360, 130],
  ['desktop-unbroken-large-light', 'view=overlay&song=unbroken&theme=light&line=0&size=xl&look=theme', 1400, 296],
  ['bigscreen-unbroken-small-dark', 'view=stage&song=unbroken&theme=dark&line=0&size=xl', 960, 540],
  ['bigscreen-unbroken-joined-small-dark', 'view=stage&song=unbroken&theme=dark&line=1&size=xl', 960, 540],
)

// The big screen from 1280 x 720 up, with every row a song can have under its lines (a Japanese
// song translated by Google: pronunciation, translation, and the attribution beside it) and every
// line the song bar can have under the song (where the rows came from, the dictionary on its way;
// for the song not yet translated, the translation note with its button): the lines keep clear
// of the song bar at every size, in both grounds and every theme's fonts.
const SCREENS = [[1280, 720], [1366, 768], [1440, 900], [1600, 900], [1920, 1080], [2560, 1440], [3840, 2160]]
const FULL_BAR = 'view=stage&song=japanese-google&line=2&size=xl&dict=downloading&engine=google&key=1'
const ASK_BAR = 'view=stage&song=japanese&line=2&size=xl&dict=downloading&engine=google&key=1'
for (const theme of ['dark', 'light'])
  for (const [w, h] of SCREENS)
    SHOTS.push(
      [`bigscreen-japanese-google-${w}x${h}-${theme}`, `${FULL_BAR}&theme=${theme}`, w, h],
      [`bigscreen-japanese-ask-${w}x${h}-${theme}`, `${ASK_BAR}&theme=${theme}`, w, h],
    )
for (const style of ['venuta', 'green-room', 'orchard', 'redline', 'glasshouse', 'neo-tokyo', 'thermoptic', 'sync-ratio', 'layer-zero', 'bounty', 'skin99'])
  SHOTS.push([`bigscreen-japanese-google-theme-${style}-1280x720-dark`, `${FULL_BAR}&theme=dark&look=theme&style=${style}`, 1280, 720])
for (const look of ['karaoke', 'clean', 'glow', 'outline', 'serif'])
  SHOTS.push([`bigscreen-japanese-google-${look}-1280x720-light`, `${FULL_BAR}&theme=light&look=${look}`, 1280, 720])
SHOTS.push(
  ['bigscreen-japanese-google-awake-1280x720-dark', `${FULL_BAR}&theme=dark`, 1280, 720, { awake: true }],
  ['bigscreen-japanese-ask-awake-1280x720-dark', `${ASK_BAR}&theme=dark`, 1280, 720, { awake: true }],
  ['bigscreen-japanese-note-1280x720-light', 'view=stage&song=japanese&line=3&size=xl&theme=light', 1280, 720],
  ['bigscreen-hindi-1280x720-dark', 'view=stage&song=hindi&line=2&size=xl&theme=dark', 1280, 720],
  ['bigscreen-mixed-1280x720-light', 'view=stage&song=mixed&line=1&size=xl&theme=light&look=outline', 1280, 720],
)

// --- Chrome over a pipe ---------------------------------------------------------------------
const profile = mkdtempSync(join(tmpdir(), 'venuta-shots-'))
const chrome = spawn(
  CHROME,
  ['--headless=new', '--remote-debugging-pipe', `--user-data-dir=${profile}`, '--no-first-run', '--no-default-browser-check', '--disable-extensions', '--hide-scrollbars', '--mute-audio', '--force-device-scale-factor=1', 'about:blank'],
  { stdio: ['ignore', 'ignore', 'ignore', 'pipe', 'pipe'] },
)
const toChrome = chrome.stdio[3]
const fromChrome = chrome.stdio[4]
let nextId = 1
const waiting = new Map()
const events = []
let buffer = ''
fromChrome.on('data', (chunk) => {
  buffer += chunk.toString('utf8')
  let end
  while ((end = buffer.indexOf('\0')) >= 0) {
    const msg = JSON.parse(buffer.slice(0, end))
    buffer = buffer.slice(end + 1)
    if (msg.id && waiting.has(msg.id)) {
      const { ok, fail } = waiting.get(msg.id)
      waiting.delete(msg.id)
      msg.error ? fail(new Error(`${msg.error.message}`)) : ok(msg.result)
    } else events.push(msg)
  }
})
const send = (method, params = {}, sessionId) =>
  new Promise((ok, fail) => {
    const id = nextId++
    waiting.set(id, { ok, fail })
    toChrome.write(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }) + '\0')
  })
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

// Counts the lyrics window's frame loops (callbacks per frame) and the time they take.
const COUNT_FRAMES = `{
  const orig = window.requestAnimationFrame.bind(window)
  const s = (window.__raf = { calls: 0, frames: 0, busy: 0, worst: 0, last: -1, frame: 0 })
  window.requestAnimationFrame = (cb) => orig((t) => {
    if (t !== s.last) { s.last = t; s.frames++; s.worst = Math.max(s.worst, s.frame); s.frame = 0 }
    s.calls++
    const t0 = performance.now()
    cb(t)
    const d = performance.now() - t0
    s.busy += d
    s.frame += d
  })
}`

// What must hold: nothing clipped, nothing over the song bar, rows in order without overlap.
const CHECK = `(() => {
  const W = innerWidth, H = innerHeight, bad = []
  const nowPlaying = !!document.querySelector('[aria-label="Now Playing"]')
  // A view that failed to draw has nothing to clip: that is a failure of its own.
  if (!document.getElementById('root')?.textContent?.trim()) bad.push('nothing drawn')
  const name = (e) => (e.dataset.second ? 'row:' + e.dataset.second : e.className.toString().slice(0, 40)) + ' "' + e.textContent.slice(0, 30) + '"'
  for (const e of document.querySelectorAll('[data-second], [data-row], [data-stack]')) {
    const r = e.getBoundingClientRect()
    if (!r.width) continue
    if (r.left < -1 || r.right > W + 1) bad.push('off the side: ' + name(e))
    if (!nowPlaying && (r.top < -1 || r.bottom > H + 1)) bad.push('off the top or bottom: ' + name(e))
    if (e.scrollWidth > e.clientWidth + 1) bad.push('wider than its box: ' + name(e))
  }
  for (const b of document.querySelectorAll('[data-fit-box]')) {
    const i = b.firstElementChild
    if (!i) continue
    const br = b.getBoundingClientRect(), ir = i.getBoundingClientRect()
    if (ir.top < br.top - 1 || ir.bottom > br.bottom + 1 || i.scrollWidth > b.clientWidth + 1) bad.push('cut off in its box: "' + i.textContent.slice(0, 40) + '"')
  }
  // Nothing the lines draw (a line, its rows, the words of either) over the song bar: every box in
  // the stack, not only the stack's own, which what is in it can overflow.
  const stack = document.querySelector('[data-stack]'), bar = document.querySelector('[data-song-bar]')
  if (stack && bar) {
    const b = bar.getBoundingClientRect()
    const hits = (r) => r.width > 0 && r.height > 0 && r.bottom > b.top + 1 && r.top < b.bottom - 1 && r.right > b.left + 1 && r.left < b.right - 1
    if (hits(stack.getBoundingClientRect())) bad.push('the lines run into the song bar')
    const into = [...stack.querySelectorAll('*')].find((e) => hits(e.getBoundingClientRect()))
    if (into) bad.push('runs into the song bar: ' + name(into))
  }
  // Each row under its line, in order: pronunciation, then translation, below the line.
  for (const r of document.querySelectorAll('[data-second]')) {
    const prev = r.previousElementSibling
    if (prev && r.getBoundingClientRect().top < prev.getBoundingClientRect().bottom - 1) bad.push('overlaps the line above: ' + name(r))
  }
  const order = [...document.querySelectorAll('[data-second]')].map((e) => e.dataset.second)
  return { bad, rows: order.length }
})()`

async function shoot(session, [name, query, w, h, opts = {}]) {
  await send('Emulation.setDeviceMetricsOverride', { width: w, height: h, deviceScaleFactor: 1, mobile: false }, session)
  await send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-reduced-motion', value: 'no-preference' }] }, session)
  await send('Page.navigate', { url: `${BASE}/lyrics-preview.html?${query}` }, session)
  for (let i = 0; i < 100; i++) {
    const r = await send('Runtime.evaluate', { expression: 'window.__previewReady === true', returnByValue: true }, session)
    if (r.result.value) break
    await sleep(100)
  }
  // The mouse moving over the big screen wakes its buttons.
  if (opts.awake) for (const [x, y] of [[w / 2, h / 2], [w / 2 + 20, h / 2 + 10]]) await send('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y }, session)
  // The lines' entrances and the fit settle.
  await sleep(1400)
  const { result } = await send('Runtime.evaluate', { expression: CHECK, returnByValue: true }, session)
  const shot = await send('Page.captureScreenshot', { format: 'png' }, session)
  writeFileSync(join(OUT, `${name}.png`), Buffer.from(shot.data, 'base64'))
  return { name, size: `${w}x${h}`, ...result.value }
}

async function clock(session) {
  // The big screen playing: its frame loops per frame, and how long they take.
  await send('Emulation.setDeviceMetricsOverride', { width: 1920, height: 1080, deviceScaleFactor: 1, mobile: false }, session)
  await send('Page.navigate', { url: `${BASE}/lyrics-preview.html?view=stage&song=hindi&theme=dark&line=0&at=0.1&size=xl&play=1` }, session)
  for (let i = 0; i < 100; i++) {
    const r = await send('Runtime.evaluate', { expression: 'window.__previewReady === true', returnByValue: true }, session)
    if (r.result.value) break
    await sleep(100)
  }
  await sleep(1500)
  await send('Performance.enable', {}, session)
  const metric = async () => Object.fromEntries((await send('Performance.getMetrics', {}, session)).metrics.map((m) => [m.name, m.value]))
  await send('Runtime.evaluate', { expression: 'Object.assign(window.__raf, { calls: 0, frames: 0, busy: 0, worst: 0, frame: 0 })' }, session)
  const before = await metric()
  await sleep(5000)
  const after = await metric()
  const { result } = await send('Runtime.evaluate', { expression: '({ ...window.__raf })', returnByValue: true }, session)
  const s = result.value
  const frames = Math.max(1, s.frames)
  // Main-thread time (scripts, style, layout, paint) per frame drawn, over five seconds of the
  // song moving (lines changing, the karaoke fill on the line and its pronunciation).
  const task = (after.TaskDuration - before.TaskDuration) * 1000
  return {
    framesIn5s: s.frames,
    loopCallbacksPerFrame: +(s.calls / frames).toFixed(2),
    mainThreadMsPerFrame: +(task / frames).toFixed(2),
    worstLoopMsInAFrame: +s.worst.toFixed(2),
    note: 'headless Chrome draws without vsync, so it draws more frames than a 60 Hz screen; per-frame time is the measure',
  }
}

try {
  mkdirSync(OUT, { recursive: true })
  const { targetId } = await send('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await send('Target.attachToTarget', { targetId, flatten: true })
  await send('Page.enable', {}, sessionId)
  await send('Page.addScriptToEvaluateOnNewDocument', { source: COUNT_FRAMES }, sessionId)
  const results = []
  for (const s of SHOTS.filter(([name]) => name.includes(ONLY))) {
    const r = await shoot(sessionId, s)
    results.push(r)
    console.log(`${r.bad.length ? 'FAIL' : 'ok  '} ${r.name} (${r.size}, ${r.rows} rows)${r.bad.length ? '\n     ' + r.bad.join('\n     ') : ''}`)
  }
  if (!ONLY) {
    const frame = await clock(sessionId)
    console.log('lyrics clock, big screen playing:', JSON.stringify(frame))
    writeFileSync(join(OUT, 'checks.json'), JSON.stringify({ shots: results, clock: frame }, null, 2) + '\n')
  }
  process.exitCode = results.some((r) => r.bad.length) ? 1 : 0
} finally {
  // Chrome lets go of its profile only once it (and its helpers) have exited.
  const exited = new Promise((r) => (chrome.exitCode !== null ? r() : chrome.once('exit', r)))
  chrome.kill()
  await exited
  await sleep(500)
  try {
    rmSync(profile, { recursive: true, force: true, maxRetries: 20, retryDelay: 250 })
  } catch (e) {
    console.warn(`Chrome's profile was left in ${profile} (${e.code}): delete it by hand`)
  }
}
