// A small driver for testing a Venuta test copy as a user would, over the inspector port of its
// WebView2: QA_PORT (default 9333). Start the copy with
// WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=<port>; see docs/qa/RELEASE.md.
//   import { session } from './qa.mjs'; const t = await session(); ... await t.close()
// Screenshots go to QA_SHOTS, or a folder in the system's temp.
import { writeFileSync, mkdirSync, appendFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

const DIR = process.env.QA_SHOTS ?? join(tmpdir(), 'venuta-qa')
mkdirSync(join(DIR, 'shots'), { recursive: true })

export async function session(target = (t) => t.type === 'page' && !t.url.includes('#')) {
  const list = await (await fetch(`http://127.0.0.1:${process.env.QA_PORT ?? 9333}/json`)).json()
  const page = list.find(target)
  if (!page) throw new Error('no page: ' + list.map((t) => t.url).join(', '))
  const ws = new WebSocket(page.webSocketDebuggerUrl)
  await new Promise((r) => (ws.onopen = r))
  let id = 0
  const console_ = []
  ws.addEventListener('message', (m) => {
    const d = JSON.parse(m.data)
    if (d.method === 'Runtime.consoleAPICalled' && ['error', 'warning'].includes(d.params.type)) {
      const text = d.params.args.map((a) => a.value ?? a.description).join(' ')
      if (!/Reduced Motion enabled|Couldn't find callback id/.test(text)) console_.push(`${d.params.type}: ${text.slice(0, 400)}`)
    }
    if (d.method === 'Runtime.exceptionThrown') console_.push(`exception: ${d.params.exceptionDetails.exception?.description?.slice(0, 400)}`)
  })
  const send = (method, params = {}) =>
    new Promise((r) => {
      const i = ++id
      ws.addEventListener('message', function on(m) {
        const d = JSON.parse(m.data)
        if (d.id === i) {
          ws.removeEventListener('message', on)
          r(d)
        }
      })
      ws.send(JSON.stringify({ id: i, method, params }))
    })
  await send('Runtime.enable')
  await send('Performance.enable')
  const run = async (expr) => {
    const r = await send('Runtime.evaluate', { expression: `(async () => { ${expr} })()`, awaitPromise: true, returnByValue: true })
    if (r.result?.exceptionDetails) throw new Error(r.result.exceptionDetails.exception?.description ?? 'page error')
    return r.result?.result?.value
  }
  // Long frames: anything that held the main thread over 50 ms, recorded in the page.
  await run(`if (!window.__qa) { window.__qa = { long: [] }; try { new PerformanceObserver((l) => { for (const e of l.getEntries()) window.__qa.long.push({ at: Math.round(e.startTime), ms: Math.round(e.duration), name: e.name }) }).observe({ type: 'longtask', buffered: false }) } catch {} }`)
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

  const t = {
    ws,
    run,
    send,
    sleep,
    console: console_,
    /** Click the element with this visible text or accessible name (first visible match). */
    async click(label, scope = 'document') {
      const ok = await run(`
        const want = ${JSON.stringify(label)}
        const els = [...${scope}.querySelectorAll('button,[role=tab],[role=menuitem],[role=radio],[role=option],a,[role=button],li,[role=row]')]
        const vis = (e) => { const r = e.getBoundingClientRect(); return r.width > 0 && r.height > 0 }
        const hit = els.find((e) => vis(e) && (e.getAttribute('aria-label') === want || e.textContent.trim() === want))
          ?? els.find((e) => vis(e) && (e.getAttribute('aria-label') ?? '').startsWith(want))
          ?? els.find((e) => vis(e) && e.textContent.trim().startsWith(want))
        if (!hit) return false
        hit.scrollIntoView({ block: 'center' })
        const r = hit.getBoundingClientRect()
        window.__qaAt = [r.left + r.width / 2, r.top + r.height / 2]
        return true`)
      if (!ok) throw new Error(`not found: ${label}`)
      const [x, y] = await run('return window.__qaAt')
      await this.mouse(x, y)
      return true
    },
    async mouse(x, y, button = 'left', clickCount = 1) {
      await send('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y })
      await send('Input.dispatchMouseEvent', { type: 'mousePressed', x, y, button, clickCount })
      await send('Input.dispatchMouseEvent', { type: 'mouseReleased', x, y, button, clickCount })
    },
    async rightClick(selectorExpr) {
      const at = await run(`const e = ${selectorExpr}; if (!e) return null; e.scrollIntoView({block:'center'}); const r = e.getBoundingClientRect(); return [r.left + r.width/2, r.top + r.height/2]`)
      if (!at) throw new Error('nothing to right-click')
      await this.mouse(at[0], at[1], 'right')
    },
    async clickAt(selectorExpr) {
      const at = await run(`const e = ${selectorExpr}; if (!e) return null; e.scrollIntoView({block:'center'}); const r = e.getBoundingClientRect(); return [r.left + r.width/2, r.top + r.height/2]`)
      if (!at) throw new Error('nothing to click: ' + selectorExpr)
      await this.mouse(at[0], at[1])
    },
    async dblClickAt(selectorExpr) {
      const at = await run(`const e = ${selectorExpr}; if (!e) return null; e.scrollIntoView({block:'center'}); const r = e.getBoundingClientRect(); return [r.left + r.width/2, r.top + r.height/2]`)
      if (!at) throw new Error('nothing to double-click: ' + selectorExpr)
      await this.mouse(at[0], at[1], 'left', 1)
      await send('Input.dispatchMouseEvent', { type: 'mousePressed', x: at[0], y: at[1], button: 'left', clickCount: 2 })
      await send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: at[0], y: at[1], button: 'left', clickCount: 2 })
    },
    /** Type as a keyboard does, into whatever has focus. */
    async type(text, delay = 25) {
      for (const ch of text) {
        await send('Input.dispatchKeyEvent', { type: 'keyDown', text: ch, key: ch, unmodifiedText: ch })
        await send('Input.dispatchKeyEvent', { type: 'keyUp', key: ch })
        if (delay) await sleep(delay)
      }
    },
    async key(key, modifiers = 0) {
      const codes = { Enter: 13, Escape: 27, Tab: 9, Backspace: 8, ArrowDown: 40, ArrowUp: 38, ArrowLeft: 37, ArrowRight: 39, ' ': 32, Space: 32, Delete: 46, Home: 36, End: 35, PageDown: 34 }
      const code = codes[key] ?? key.toUpperCase().charCodeAt(0)
      const k = key === 'Space' ? ' ' : key
      await send('Input.dispatchKeyEvent', { type: 'rawKeyDown', key: k, windowsVirtualKeyCode: code, modifiers, ...(k.length === 1 && !modifiers ? { text: k } : {}) })
      if (k.length === 1 && !modifiers) await send('Input.dispatchKeyEvent', { type: 'char', key: k, text: k })
      await send('Input.dispatchKeyEvent', { type: 'keyUp', key: k, windowsVirtualKeyCode: code, modifiers })
    },
    async shot(name) {
      const s = await send('Page.captureScreenshot', { format: 'png' })
      writeFileSync(join(DIR, 'shots', `${name}.png`), Buffer.from(s.result.data, 'base64'))
      return join(DIR, 'shots', `${name}.png`)
    },
    /** Wait until two captures 300 ms apart match (the screen settled), up to `max` ms. Returns ms taken. */
    async settle(max = 5000) {
      const t0 = Date.now()
      let last = ''
      while (Date.now() - t0 < max) {
        const s = (await send('Page.captureScreenshot', { format: 'jpeg', quality: 30 })).result.data
        if (s === last) return Date.now() - t0 - 300
        last = s
        await sleep(300)
      }
      return -1
    },
    async longFrames(clear = true) {
      return run(`const l = window.__qa.long.slice(); ${clear ? 'window.__qa.long = [];' : ''} return l`)
    },
    async metrics() {
      const m = (await send('Performance.getMetrics')).result.metrics
      const g = (n) => m.find((x) => x.name === n)?.value
      return { heapMB: Math.round(g('JSHeapUsedSize') / 1e6), nodes: g('Nodes'), listeners: g('JSEventListeners'), layouts: g('LayoutCount'), styleRecalcs: g('RecalcStyleCount'), scriptMs: Math.round(g('ScriptDuration') * 1000), layoutMs: Math.round(g('LayoutDuration') * 1000) }
    },
    text: (sel = 'main') => run(`return (document.querySelector(${JSON.stringify(sel)}) ?? document.body).innerText.slice(0, 4000)`),
    note(line) {
      appendFileSync(join(DIR, 'log.md'), line + '\n')
      console.log(line)
    },
    async close() {
      ws.close()
    },
  }
  return t
}
