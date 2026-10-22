// Screenshots of the room panel v2 in every state of the window's fixture, dark and light, at the
// window's default and smallest sizes: headless Chrome (a temporary profile, no window on the
// desktop), driven over its DevTools pipe, against the Vite dev server's preview page
// (index.html here). The viewport is set exactly (Emulation), since headless Chrome's
// --window-size leaves the page smaller than the size asked.
//
//   pnpm vite --port 5291 --strictPort        (in app/, left running)
//   node src/components/together/preview/shoot.mjs http://localhost:5291 ../docs/screenshots/listen-together-v2 [part of a case name]
//
// CHROME names another Chrome or Edge. Each file is <case>[-<what is open>]-<theme>-<width>x<height>.png.

import { spawn } from 'node:child_process'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'

const [base = 'http://localhost:5291', out = '../docs/screenshots/listen-together-v2', only] = process.argv.slice(2)
const CHROME = process.env.CHROME ?? 'C:/Program Files/Google/Chrome/Application/chrome.exe'
/** The window's default size, and its smallest (src-tauri/tauri.conf.json). */
const SIZES = [
  [1280, 820],
  [960, 620],
]
const THEMES = ['dark', 'light']
/** What the panel shows only after a press: the host's two questions, and a person's numbers. */
const EXTRAS = [
  ['hosting, everyone in every state', 'show=end', 'end-for-everyone'],
  ['hosting, everyone in every state', 'show=hand', 'hand-over'],
  ['hosting, everyone in every state', 'open=1', 'numbers'],
  ['in, skipping', 'open=2', 'numbers'],
]

const { cases } = JSON.parse(readFileSync(new URL('../../../../tests/fixtures/together-views.json', import.meta.url), 'utf8'))
const slug = (s) =>
  s
    .toLowerCase()
    .replace(/'/g, '')
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-|-$/g, '')
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

/** Chrome over --remote-debugging-pipe: messages are JSON, each ended by a NUL. */
function open() {
  const profile = mkdtempSync(join(tmpdir(), 'venuta-panel-shots-'))
  const chrome = spawn(
    CHROME,
    ['--headless=new', `--user-data-dir=${profile}`, '--no-first-run', '--no-default-browser-check', '--disable-extensions', '--disable-gpu', '--hide-scrollbars', '--mute-audio', '--remote-debugging-pipe', 'about:blank'],
    { stdio: ['ignore', 'ignore', 'ignore', 'pipe', 'pipe'] },
  )
  const toChrome = chrome.stdio[3]
  const fromChrome = chrome.stdio[4]
  let id = 0
  const waiting = new Map()
  let buf = ''
  fromChrome.on('data', (d) => {
    buf += d.toString('utf8')
    let at
    while ((at = buf.indexOf('\0')) >= 0) {
      const msg = JSON.parse(buf.slice(0, at))
      buf = buf.slice(at + 1)
      const w = waiting.get(msg.id)
      if (!w) continue
      waiting.delete(msg.id)
      if (msg.error) w.reject(new Error(JSON.stringify(msg.error)))
      else w.resolve(msg.result)
    }
  })
  const send = (method, params = {}, sessionId) =>
    new Promise((resolve, reject) => {
      const n = ++id
      waiting.set(n, { resolve, reject })
      toChrome.write(JSON.stringify({ id: n, method, params, ...(sessionId ? { sessionId } : {}) }) + '\0')
    })
  const close = async () => {
    await send('Browser.close').catch(() => {})
    await new Promise((r) => (chrome.exitCode !== null ? r() : chrome.once('exit', r)))
    rmSync(profile, { recursive: true, force: true })
  }
  return { send, close }
}

const dir = resolve(out)
mkdirSync(dir, { recursive: true })
const cdp = open()
let made = 0
try {
  const { targetId } = await cdp.send('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await cdp.send('Target.attachToTarget', { targetId, flatten: true })
  const page = (m, p) => cdp.send(m, p, sessionId)
  await page('Page.enable')
  const shots = [...cases.map((c) => [c.name, '', '']), ...EXTRAS]
  for (const [name, extra, label] of shots) {
    if (only && !name.includes(only)) continue
    for (const theme of THEMES)
      for (const [width, height] of SIZES) {
        await page('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 1, mobile: false })
        await page('Page.navigate', { url: `${base}/src/components/together/preview/index.html?case=${encodeURIComponent(name)}&theme=${theme}${extra ? `&${extra}` : ''}` })
        // The page says when the panel is drawn (and its fonts are in).
        let ready = false
        for (let i = 0; i < 200 && !ready; i++) {
          await sleep(50)
          const r = await page('Runtime.evaluate', { expression: 'document.body?.dataset.ready === "1"', returnByValue: true }).catch(() => null)
          ready = r?.result?.value === true
        }
        if (!ready) {
          console.error(`not drawn: ${name} ${extra} ${theme} ${width}x${height}`)
          process.exitCode = 1
          continue
        }
        const { data } = await page('Page.captureScreenshot', { format: 'png' })
        writeFileSync(join(dir, `${slug(name)}${label ? `-${label}` : ''}-${theme}-${width}x${height}.png`), Buffer.from(data, 'base64'))
        made++
      }
  }
} finally {
  await cdp.close()
}
console.log(`${made} screenshots in ${dir}`)
