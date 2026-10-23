// The output picker in every case (a Bluetooth device's twins, hands-free only, a USB DAC
// bit-perfect, the chosen device away, following Windows), in the quality badge's popover and in
// Settings, dark and light, captured in headless Chrome from the Vite dev server's preview page
// (app/devices-preview.html, fixture devices; never the app, never a window on screen), and
// checked: something drawn, nothing wider than its box or off the side.
//
//   pnpm --dir app exec vite --port 5199 --strictPort      (in another shell)
//   node scripts/devices-shots.mjs [--url http://localhost:5199] [--out docs/screenshots/devices]
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
const OUT = resolve(arg('out', 'docs/screenshots/devices'))
const CHROME = [process.env.CHROME, 'C:/Program Files/Google/Chrome/Application/chrome.exe', 'C:/Program Files (x86)/Google/Chrome/Application/chrome.exe', '/usr/bin/google-chrome'].find((p) => p && existsSync(p))
if (!CHROME) throw new Error('Chrome not found: set CHROME')

// [name, query, width, height]
const SHOTS = []
for (const theme of ['dark', 'light']) {
  for (const c of ['twin', 'handsfree', 'dac', 'away', 'follow']) SHOTS.push([`popover-${c}-${theme}`, `case=${c}&theme=${theme}&view=popover`, 400, 900])
  SHOTS.push([`settings-twin-${theme}`, `case=twin&theme=${theme}&view=settings`, 720, 1100])
}

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
    }
  }
})
const send = (method, params = {}, sessionId) =>
  new Promise((ok, fail) => {
    const id = nextId++
    waiting.set(id, { ok, fail })
    toChrome.write(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }) + '\0')
  })
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

const CHECK = `(() => {
  const W = innerWidth, bad = []
  if (!document.getElementById('root')?.textContent?.trim()) bad.push('nothing drawn')
  for (const e of document.querySelectorAll('#root *')) {
    const r = e.getBoundingClientRect()
    if (!r.width) continue
    if (r.right > W + 1) bad.push('off the side: ' + e.tagName + ' "' + (e.textContent || '').slice(0, 30) + '"')
    if (e.tagName !== 'INPUT' && getComputedStyle(e).overflowX === 'visible' && e.scrollWidth > e.clientWidth + 1 && e.children.length === 0) bad.push('wider than its box: "' + e.textContent.slice(0, 40) + '"')
  }
  const rows = document.querySelectorAll('button[aria-pressed]').length
  return { bad: [...new Set(bad)].slice(0, 10), rows }
})()`

async function shoot(session, [name, query, w, h]) {
  await send('Emulation.setDeviceMetricsOverride', { width: w, height: h, deviceScaleFactor: 1, mobile: false }, session)
  await send('Page.navigate', { url: `${BASE}/devices-preview.html?${query}` }, session)
  // Drawn (the first load waits for Vite to prepare the modules), then settled.
  for (let t = 0; t < 150; t++) {
    const { result } = await send('Runtime.evaluate', { expression: "!!document.getElementById('root')?.textContent?.trim()", returnByValue: true }, session)
    if (result.value) break
    await sleep(100)
  }
  await sleep(400)
  const { result } = await send('Runtime.evaluate', { expression: CHECK, returnByValue: true }, session)
  const shot = await send('Page.captureScreenshot', { format: 'png' }, session)
  writeFileSync(join(OUT, `${name}.png`), Buffer.from(shot.data, 'base64'))
  return { name, size: `${w}x${h}`, ...result.value }
}

try {
  mkdirSync(OUT, { recursive: true })
  const { targetId } = await send('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await send('Target.attachToTarget', { targetId, flatten: true })
  await send('Page.enable', {}, sessionId)
  await send('Runtime.enable', {}, sessionId)
  const results = []
  for (const s of SHOTS) {
    const r = await shoot(sessionId, s)
    results.push(r)
    console.log(`${r.bad.length ? 'FAIL' : 'ok  '} ${r.name} (${r.size}, ${r.rows} devices)${r.bad.length ? '\n     ' + r.bad.join('\n     ') : ''}`)
  }
  writeFileSync(join(OUT, 'checks.json'), JSON.stringify({ shots: results }, null, 2) + '\n')
  process.exitCode = results.some((r) => r.bad.length) ? 1 : 0
} finally {
  const gone = new Promise((r) => chrome.once('exit', r))
  chrome.kill()
  await Promise.race([gone, sleep(5000)])
  await sleep(300)
  rmSync(profile, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 })
}
