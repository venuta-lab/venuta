// The release gates (docs/qa/RELEASE.md), checked in a running test copy as a
// person would use it. Each prints PASS or FAIL with its numbers; the run fails if any fails.
//
//   QA_PORT=9334 node scripts/qa/gates.mjs
//
// Options (environment): QA_BIG_PLAYLIST="Favourite Songs (Apple, whole)" adds the big-playlist
// gate; QA_ROUNDS=20 sets the memory gate's rounds (10 places a round). Measure a production
// build of the UI (vite build, then vite preview on the copy's dev port): the development build
// runs every render twice and is several times slower. Plays are silent (volume 0) and paused
// after. Nothing in the library is changed.
import { session } from './qa.mjs'

const t = await session()
const results = []
const gate = (name, pass, detail) => {
  results.push({ name, pass })
  console.log(`${pass ? 'PASS' : 'FAIL'}  ${name.padEnd(34)} ${detail}`)
}
const mute = () => t.run(`await window.__TAURI_INTERNALS__.invoke('player_volume', { volume: 0 })`)
const worst = async () => Math.max(0, ...(await t.longFrames()).map((l) => l.ms))
const place = async (key) => {
  const k = { key, code: 'Digit' + key, modifiers: 2, windowsVirtualKeyCode: key.charCodeAt(0) }
  await t.send('Input.dispatchKeyEvent', { type: 'keyDown', ...k })
  await t.send('Input.dispatchKeyEvent', { type: 'keyUp', ...k })
}
const tab = (label) => t.click(label, "document.querySelector('main')")
const back = async () => {
  for (let i = 0; i < 3; i++) {
    const b = await t.run(`const b = [...document.querySelectorAll('main button')].find(b => b.textContent.trim() === 'Back'); if (b) b.click(); return !!b`)
    if (!b) break
    await t.sleep(600)
  }
}
const scroll = async () => {
  for (let i = 0; i < 25; i++) {
    await t.send('Input.dispatchMouseEvent', { type: 'mouseWheel', x: 700, y: 450, deltaX: 0, deltaY: 1200 })
    await t.sleep(30)
  }
  await t.sleep(300)
  await t.run(`document.querySelector('main .thin-scroll')?.scrollTo({ top: 0 })`)
}

await mute()
await place('1')
await t.sleep(600)
await back()

// 1. Opening any place, and scrolling the long ones: no frame over 50 ms (three rounds).
{
  let w = 0
  for (let round = 0; round < 3; round++) {
    for (const k of ['2', '3', '4', '5', '1']) {
      await t.longFrames()
      await place(k)
      await t.sleep(900)
      w = Math.max(w, await worst())
    }
    for (const name of ['Albums', 'Artists', 'Songs', 'Playlists', 'Home']) {
      await t.longFrames()
      await tab(name).catch(() => {})
      await t.sleep(900)
      if (name !== 'Playlists') await scroll()
      w = Math.max(w, await worst())
    }
  }
  gate('places open and scroll', w <= 50, `worst frame ${w} ms (gate 50)`)
}

// 2. The queue of the whole library: opens under 150 ms, no frame over 50 ms, opening or scrolling.
{
  await tab('Songs')
  await t.sleep(900)
  await t.dblClickAt(`document.querySelector('main [role=listitem][data-index="3"]')`)
  await t.sleep(1500)
  await mute()
  await t.longFrames()
  const t0 = Date.now()
  await t.clickAt(`document.querySelector('[aria-label="Queue (Q)"]')`)
  let opened = -1
  for (let i = 0; i < 60; i++) {
    if (await t.run(`return !!document.querySelector('aside[aria-label="Queue"] [role=listitem]')`)) {
      opened = Date.now() - t0
      break
    }
    await t.sleep(10)
  }
  await t.sleep(600)
  let w = await worst()
  for (let i = 0; i < 30; i++) {
    await t.send('Input.dispatchMouseEvent', { type: 'mouseWheel', x: 1100, y: 400, deltaX: 0, deltaY: 3000 })
    await t.sleep(30)
  }
  await t.sleep(400)
  w = Math.max(w, await worst())
  const size = await t.run(`return document.querySelector('aside[aria-label="Queue"] header')?.innerText.match(/[\\d,]+ up next/)?.[0] ?? '?'`)
  gate('queue of the whole library', opened >= 0 && opened < 150 && w <= 50, `${size}: opened in ${opened} ms (gate 150), worst frame ${w} ms`)
  await t.key('Escape')
  await t.sleep(400)
}

// 3. The big playlist, if named: opens under 300 ms, no frame over 50 ms.
if (process.env.QA_BIG_PLAYLIST) {
  const name = process.env.QA_BIG_PLAYLIST
  await place('1')
  await t.sleep(500)
  await back()
  await tab('Playlists')
  await t.sleep(1200)
  await t.longFrames()
  const t0 = Date.now()
  await t.clickAt(`[...document.querySelectorAll('main .card')].find(c => c.querySelector('.font-medium')?.textContent === ${JSON.stringify(name)})?.querySelector('button')`)
  let opened = -1
  for (let i = 0; i < 100; i++) {
    if (await t.run(`return !!document.querySelector('main [role=list] [role=listitem]')`)) {
      opened = Date.now() - t0
      break
    }
    await t.sleep(10)
  }
  await t.sleep(700)
  await scroll()
  const w = await worst()
  gate('big playlist', opened >= 0 && opened < 300 && w <= 50, `"${name}" opened in ${opened} ms (gate 300), worst frame ${w} ms`)
  await back()
}

// 4. Typing in Music's search: no keystroke over 50 ms.
{
  await place('1')
  await t.sleep(500)
  await t.run(`const i = document.querySelector('main input[placeholder^="Search albums"]'); i.focus(); i.select()`)
  await t.longFrames()
  for (const ch of 'arijit singh') {
    await t.send('Input.insertText', { text: ch })
    await t.sleep(120)
  }
  await t.sleep(500)
  const w = await worst()
  gate('typing in search', w <= 50, `worst frame ${w} ms (gate 50)`)
  await t.key('a', 2)
  await t.key('Backspace')
  await t.key('Escape')
}

// 5. Right-click: never the browser's menu; things have their menu, empty space none.
{
  await t.run(`window.__qaMenus = []; window.addEventListener('contextmenu', (e) => window.__qaMenus.push(e.defaultPrevented))`)
  const menu = () => t.run(`return document.querySelectorAll('[role=menu] [role=menuitem]').length`)
  await t.rightClick(`document.querySelector('main h1')`)
  await t.sleep(300)
  const empty = await menu()
  await t.key('Escape')
  await tab('Albums')
  await t.sleep(800)
  await t.rightClick(`document.querySelector('main .card')`)
  await t.sleep(300)
  const album = await menu()
  await t.key('Escape')
  const all = await t.run(`return window.__qaMenus`)
  gate('right-click', all.every(Boolean) && empty === 0 && album > 0, `browser menu stopped ${all.filter(Boolean).length}/${all.length}; empty space ${empty} items; an album ${album} items`)
}

// 6. Memory after many page visits (after collecting): within 10 MB of the start.
{
  const heap = async () => {
    await t.send('HeapProfiler.enable')
    await t.send('HeapProfiler.collectGarbage')
    await t.sleep(300)
    await t.send('HeapProfiler.collectGarbage')
    return (await t.metrics()).heapMB
  }
  const round = async () => {
    for (const k of ['2', '3', '4', '5', '1']) {
      await place(k)
      await t.sleep(350)
    }
    for (const name of ['Albums', 'Songs', 'Artists', 'Playlists', 'Home']) {
      await tab(name).catch(() => {})
      await t.sleep(350)
    }
  }
  const rounds = Number(process.env.QA_ROUNDS ?? 20)
  await round()
  const start = await heap()
  for (let i = 0; i < rounds; i++) await round()
  // The shared-layout sweep runs every 8 s: give it a turn before measuring.
  await t.sleep(17000)
  const end = await heap()
  gate('memory over page visits', end - start <= 10, `${start} → ${end} MB over ${rounds * 10} visits (gate +10 MB)`)
}

// 7. Nothing went wrong on the way.
gate('no errors in the console', t.console.filter((c) => /error|exception/i.test(c)).length === 0, `${t.console.length} warnings or errors`)

if (await t.run(`return !!document.querySelector('[aria-label^="Pause ("]')`)) await t.click('Pause (Space)').catch(() => {})
const failed = results.filter((r) => !r.pass).length
console.log(failed ? `\n${failed} of ${results.length} gates failed.` : `\nAll ${results.length} gates pass.`)
process.exit(failed ? 1 : 0)
