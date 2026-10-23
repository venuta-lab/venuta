// FEATURES.md walked through (docs/qa/RELEASE.md step 4), in a running test copy, as far as the
// page can drive it. Each line of docs/qa/FEATURES.md, and the release's new features, is a check
// that records pass, fail (with why and a screenshot) or "needs a person" (what the page can't
// reach: Windows, the tray, real windows' behaviour, writes to music files).
//
//   QA_PORT=9333 QA_SHOTS=<folder> node scripts/qa/smoke.mjs        everything
//   QA_ONLY=music,playing node scripts/qa/smoke.mjs                  some sections
//
// Sections: music, lyrics, windows, playing, playlists, library, settings, everywhere, updates,
// together, output, counts. The log is <QA_SHOTS>/smoke.md, the results <QA_SHOTS>/smoke.json.
//
// What it never does, because the copy's Music folder holds hard links to a real library: save
// Edit details or Fix details, Apply in Organize, save the lyrics maker, run the online or
// measuring tools, convert or write to devices. Those screens are opened, checked and closed.
// It is silent: the volume is held at 0 by a guard in the page for the whole run (the one volume
// check raises it to 5% only while paused, then mutes), and it pauses at the end. The big screen,
// desktop lyrics and mini player are each opened once and closed at once; if one doesn't close,
// the run stops. No sign-ins, rooms, online translation or imports from services. Playlists made
// here live in the copy's own Playlists folder and are set aside again at the end.
import { session } from './qa.mjs'
import { existsSync, readdirSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

const DIR = process.env.QA_SHOTS ?? join(tmpdir(), 'venuta-qa')
const PORT = process.env.QA_PORT ?? 9333
const ONLY = process.env.QA_ONLY ? process.env.QA_ONLY.split(',') : null
const LOG = join(DIR, 'smoke.md')
const TEST = 'QA smoke 040'

const t = await session()
const results = []
writeFileSync(LOG, `# FEATURES.md walk, ${new Date().toISOString()}\n\n`)
const line = (s) => {
  writeFileSync(LOG, s + '\n', { flag: 'a' })
  console.log(s)
}
const sleep = t.sleep
let n = 0

// --- The page --------------------------------------------------------------------------------

const PAGE = `
window.__sm = {
  vis(e) { const r = e.getBoundingClientRect(); return r.width > 0 && r.height > 0 && getComputedStyle(e).visibility !== 'hidden' },
  scope(s) { return typeof s === 'string' ? document.querySelector(s) : document },
  label(e) { return (e.getAttribute('aria-label') || e.textContent || '').trim().replace(/\\s+/g, ' ') },
  all(s) { const sc = __sm.scope(s); return sc ? [...sc.querySelectorAll('button,[role=tab],[role=menuitem],[role=menuitemradio],[role=radio],[role=switch],[role=option],a,[role=button]')].filter(__sm.vis) : [] },
  find(want, s, loose) {
    const els = __sm.all(s)
    const exact = els.find((e) => e.getAttribute('aria-label') === want || e.textContent.trim() === want)
    if (exact || !loose) return exact
    return els.find((e) => (e.getAttribute('aria-label') ?? '').startsWith(want)) ?? els.find((e) => e.textContent.trim().startsWith(want))
  },
  labels(s) { return __sm.all(s).map(__sm.label) },
}
if (!window.__qaGuard) window.__qaGuard = setInterval(() => {
  if (window.__qaVolOk) return
  const v = document.querySelector('input[aria-label=Volume]')
  if (v && +v.value > 0) window.__TAURI_INTERNALS__.invoke('player_volume', { volume: 0 })
}, 100)
return true`
const run = async (js) => {
  try {
    return await t.run(js)
  } catch (e) {
    if (/__sm/.test(e.message)) {
      await t.run(PAGE)
      return t.run(js)
    }
    throw e
  }
}
await run(PAGE)
const J = JSON.stringify
const mute = () => run(`await window.__TAURI_INTERNALS__.invoke('player_volume', { volume: 0 })`)
await mute()

/** Wait until the expression is true (or return false after `ms`). */
async function until(expr, ms = 4000) {
  const t0 = Date.now()
  while (Date.now() - t0 < ms) {
    if (await run(`return !!(${expr})`).catch(() => false)) return true
    await sleep(100)
  }
  return false
}
const has = (label, scope, loose = false) => run(`return !!__sm.find(${J(label)}, ${J(scope ?? null)}, ${loose})`)
const labels = (scope) => run(`return __sm.labels(${J(scope ?? null)})`)
const text = (sel = 'main') => run(`return (document.querySelector(${J(sel)})?.innerText ?? '')`)
/** Click what has this name, as the mouse does (it waits for it to be there). */
async function press(label, scope, loose = false) {
  const t0 = Date.now()
  let at = null
  while (Date.now() - t0 < 3000) {
    at = await run(`const e = __sm.find(${J(label)}, ${J(scope ?? null)}, ${loose}); if (!e) return null; e.scrollIntoView({ block: 'center' }); const r = e.getBoundingClientRect(); return [r.left + r.width / 2, r.top + r.height / 2]`)
    if (at) break
    await sleep(150)
  }
  if (!at) throw new Error(`no "${label}" to press${scope ? ` in ${scope}` : ''}`)
  await t.mouse(at[0], at[1])
  await sleep(250)
}
async function pressAt(expr) {
  const at = await run(`const e = ${expr}; if (!e) return null; e.scrollIntoView({ block: 'center' }); const r = e.getBoundingClientRect(); return [r.left + r.width / 2, r.top + r.height / 2]`)
  if (!at) throw new Error('nothing at ' + expr.slice(0, 80))
  await t.mouse(at[0], at[1])
  await sleep(250)
}
async function rightAt(expr) {
  const at = await run(`const e = ${expr}; if (!e) return null; e.scrollIntoView({ block: 'center' }); const r = e.getBoundingClientRect(); return [r.left + Math.min(40, r.width / 2), r.top + r.height / 2]`)
  if (!at) throw new Error('nothing to right-click at ' + expr.slice(0, 80))
  await t.mouse(at[0], at[1], 'right')
  await sleep(400)
}
const menuItems = () => run(`return [...document.querySelectorAll('[role=menu] [role=menuitem]')].filter(__sm.vis).map((e) => e.textContent.trim())`)
const esc = async (times = 1) => {
  for (let i = 0; i < times; i++) {
    await t.key('Escape')
    await sleep(250)
  }
}
/** Into the body, so the player's keys are the player's. */
const blur = () => run(`document.activeElement?.blur?.(); return true`)
const ctrl = async (key, shift = false) => {
  const k = { key, code: /^\d$/.test(key) ? 'Digit' + key : 'Key' + key.toUpperCase(), modifiers: 2 | (shift ? 8 : 0), windowsVirtualKeyCode: key.toUpperCase().charCodeAt(0) }
  if (key === ',') Object.assign(k, { code: 'Comma', windowsVirtualKeyCode: 188 })
  if (key.toLowerCase() === 'q') throw new Error('Ctrl+Q quits the app: never sent')
  await t.send('Input.dispatchKeyEvent', { type: 'rawKeyDown', ...k })
  await t.send('Input.dispatchKeyEvent', { type: 'keyUp', ...k })
  await sleep(300)
}
const ctrlArrow = async (dir) => {
  const k = { key: 'Arrow' + dir, code: 'Arrow' + dir, modifiers: 2, windowsVirtualKeyCode: dir === 'Right' ? 39 : 37 }
  await t.send('Input.dispatchKeyEvent', { type: 'rawKeyDown', ...k })
  await t.send('Input.dispatchKeyEvent', { type: 'keyUp', ...k })
  await sleep(300)
}
const altArrow = async (dir) => {
  const k = { key: 'Arrow' + dir, code: 'Arrow' + dir, modifiers: 1, windowsVirtualKeyCode: dir === 'Down' ? 40 : 38 }
  await t.send('Input.dispatchKeyEvent', { type: 'rawKeyDown', ...k })
  await t.send('Input.dispatchKeyEvent', { type: 'keyUp', ...k })
  await sleep(400)
}
/** Type into the focused field only (a stray key elsewhere is a shortcut: M unmutes). */
async function typeIn(textToType) {
  const field = await run(`const a = document.activeElement; return !!a && (a.tagName === 'INPUT' || a.tagName === 'TEXTAREA')`)
  if (!field) throw new Error('nothing to type into has focus')
  await t.send('Input.insertText', { text: textToType })
  await sleep(300)
}
async function focusField(sel) {
  const ok = await run(`const i = document.querySelector(${J(sel)}); if (!i) return false; i.focus(); i.select?.(); return document.activeElement === i`)
  if (!ok) throw new Error('no field ' + sel)
}
async function clearField() {
  await run(`const a = document.activeElement; if (a && (a.tagName === 'INPUT' || a.tagName === 'TEXTAREA')) a.select(); return true`)
  await t.key('Backspace')
  await sleep(200)
}

// Places: the rail, and Back until a place's own first screen.
async function rail(name) {
  await closeLayers()
  await press(name, 'nav')
  await sleep(700)
}
async function back() {
  for (let i = 0; i < 4; i++) {
    const b = await run(`const b = [...document.querySelectorAll('main button')].find((b) => b.textContent.trim() === 'Back' && __sm.vis(b)); if (b) b.click(); return !!b`)
    if (!b) break
    await sleep(600)
  }
}
const tab = async (name) => {
  await press(name, 'main')
  await sleep(700)
}
const nowOpen = () => run(`return !!document.querySelector('section[aria-label="Now Playing"]')`)
const queueOpen = () => run(`return !!document.querySelector('aside[aria-label=Queue]')`)
async function closeLayers() {
  for (let i = 0; i < 3; i++) {
    const layer = await run(`return !!document.querySelector('[role=menu],[role=dialog],[role=alertdialog],section[aria-label="Now Playing"],aside[aria-label=Queue]')`)
    if (!layer) return
    await esc()
  }
}
const playing = () => run(`return !!document.querySelector('[aria-label="Pause (Space)"]')`)
async function pause() {
  if (await playing()) await press('Pause (Space)')
  await until(`document.querySelector('[aria-label="Play (Space)"]')`, 3000)
}
const songNow = () => run(`return document.querySelector('[aria-label="Now playing"] .truncate, [aria-label="Now playing"] span')?.textContent?.trim() ?? ''`)
const barText = () => text('[aria-label="Now playing"]')
const position = () => run(`return +(document.querySelector('[aria-label="Position in the song"]')?.getAttribute('aria-valuenow') ?? -1)`)
async function targets() {
  return (await (await fetch(`http://127.0.0.1:${PORT}/json`)).json()).filter((x) => x.type === 'page')
}

// --- Results ---------------------------------------------------------------------------------

let section = ''
const want = (s) => !ONLY || ONLY.includes(s)
const need = (ok, why) => {
  if (!ok) throw new Error(why)
}
function record(area, name, result, why, shot) {
  results.push({ section, area, name, result, why, shot })
  line(`- ${result === 'pass' ? 'PASS' : result === 'fail' ? 'FAIL' : 'PERSON'} | ${area} | ${name} | ${why}${shot ? ` | ${shot}` : ''}`)
}
async function check(area, name, fn) {
  try {
    const why = await fn()
    record(area, name, 'pass', why ?? '')
  } catch (e) {
    const shot = await t.shot(`fail-${String(++n).padStart(2, '0')}-${name.replace(/[^a-z0-9]+/gi, '-').slice(0, 40)}`).catch(() => '')
    record(area, name, 'fail', e.message.split('\n')[0], shot)
    await closeLayers().catch(() => {})
  } finally {
    await mute().catch(() => {})
  }
}
const person = (area, name, why) => record(area, name, 'person', why)
const begin = (s) => {
  section = s
  line(`\n## ${s}\n`)
}

/** Double-click a song row: is it the song now, and still playing 1.5 s later? */
async function dblPlay(rowExpr, title) {
  await mute()
  await t.dblClickAt(rowExpr)
  const current = await until(`document.querySelector('[aria-label="Now playing"]')?.innerText.includes(${J(title)})`, 5000)
  await sleep(1500)
  const on = await playing()
  await mute()
  await pause()
  return { current, on }
}
const DBL = 'double-click: the song starts, then the second click of the double-click pauses it (one click on a row plays it, and a click on the playing row toggles)'

/** Play a song from the title-bar search: its own Play button among the results. */
async function playSearch(q, title) {
  await closeLayers()
  await mute()
  await t.key('/')
  await until(`document.activeElement?.getAttribute('aria-label') === 'Search your music'`, 3000)
  await clearField()
  await typeIn(q)
  await t.key('Enter')
  await sleep(1500)
  await mute()
  await press(`Play ${title}`, 'main')
  return until(`document.querySelector('[aria-label="Now playing"]')?.innerText.includes(${J(title)})`, 6000)
}

// ===============================================================================================
// Music
// ===============================================================================================

if (want('music')) {
  begin('music')

  await check('Music', 'Home: top card, Jump back in, Your playlists, shelves, See all', async () => {
    await rail('Home')
    await back()
    const m = await text()
    for (const w of ['Jump back in', 'Your playlists', 'Your music']) need(m.includes(w), `no "${w}" on Home`)
    const hero = (await has('Resume', 'main')) || (await has('Play', 'main'))
    need(hero, 'the top card has neither Resume nor Play')
    need(await has('Shuffle', 'main'), 'the top card has no Shuffle')
    const top = (await has('Resume', 'main')) ? 'Resume' : 'Play'
    const seeAll = await run(`return __sm.all('main').filter((b) => b.textContent.trim() === 'See all').length`)
    need(seeAll > 0, 'no See all')
    // See all of a category shelf (Hi-Res), and of Your playlists.
    const seeAllOf = (heading) => `[...document.querySelectorAll('main button')].find((b) => b.textContent.trim() === 'See all' && __sm.vis(b) && (b.closest('section')?.querySelector('h2,h3')?.textContent ?? '').startsWith(${J(heading)}))`
    await pressAt(seeAllOf('Hi-Res'))
    await sleep(1500)
    const hiRes = await text()
    await rail('Home')
    await back()
    await pressAt(seeAllOf('Your playlists'))
    await sleep(2000)
    // What the page shows besides a notice (notices live in main too).
    const lists = await run(`return document.querySelectorAll('main .card, main h1, main [role=tab]').length`)
    const where = await run(`return document.querySelector('nav [aria-current=page]')?.textContent?.trim()`)
    const blankShot = lists ? '' : await t.shot('home-your-playlists-see-all-blank')
    if (!lists) {
      await rail('Library')
      await tab('Playlists')
    }
    need(/Hi-Res \d+/.test(hiRes), 'Hi-Res See all did not open the Hi-Res albums')
    need(lists > 0, `Your playlists › See all: ${where} is selected but its page is blank (no heading, tabs or cards; ${blankShot}); the button passes its click event to openPlaylists as the place (PlaylistShelf onClick={props.onSeeAll})`)
    return `top card ${top} + Shuffle; ${seeAll} See all; Hi-Res and Your playlists See all open their lists`
  })

  await check('Music', 'Albums: filters, open an album, Play, Shuffle, Edit details', async () => {
    await rail('Library')
    await back()
    await tab('Albums')
    const all = await run(`return document.querySelectorAll('main .card').length`)
    await press('Hi-Res', 'main', true)
    await sleep(700)
    const hi = await run(`return [...document.querySelectorAll('main .card')].length`)
    const hiText = await text()
    await press('All', 'main', true)
    await sleep(700)
    need(all > 0, 'no album cards')
    need(/Hi-Res/.test(hiText), 'the Hi-Res filter shows no Hi-Res albums')
    const cats = (await labels('main')).filter((l) => /^[A-Z].* \d+$/.test(l))
    need(cats.length >= 3, 'fewer than 3 category filters')
    await pressAt(`document.querySelector('main .card button')`)
    await sleep(900)
    const title = await run(`return document.querySelector('main h1')?.textContent?.trim()`)
    for (const b of ['Play', 'Shuffle', 'Edit details']) need(await has(b, 'main'), `album page has no ${b}`)
    await press('Edit details', 'main')
    need(await until(`document.querySelector('[role=dialog][aria-label$=", details"]')`), 'Edit details opened no editor')
    const fields = await run(`return document.querySelectorAll('[role=dialog] input, [role=dialog] textarea').length + document.querySelectorAll('[role=dialog] [role=button],[role=dialog] button').length`)
    await esc()
    need(await until(`!document.querySelector('[role=dialog][aria-label$=", details"]')`, 2000), 'the editor did not close on Escape')
    await back()
    return `filters: ${cats.length} categories, Hi-Res ${hi} shown; opened "${title}"; editor opened (${fields} controls) and closed with nothing saved`
  })

  await check('Music', 'Album page: songs, Play, Shuffle, play one, right-click, Go to artist, back', async () => {
    await rail('Library')
    await back()
    await tab('Albums')
    await pressAt(`document.querySelector('main .card button')`)
    await sleep(900)
    const album = await run(`return document.querySelector('main h1')?.textContent?.trim()`)
    const rows = await run(`return document.querySelectorAll('main [role=listitem]').length`)
    need(rows > 0, 'no songs on the album page')
    const second = await run(`return document.querySelectorAll('main [role=listitem]')[1]?.querySelector('.font-medium')?.textContent?.trim() ?? ''`)
    await mute()
    await press('Play', 'main')
    need(await until(`document.querySelector('[aria-label="Pause (Space)"]')`, 5000), 'Play did not start the album')
    await pause()
    await press('Shuffle', 'main')
    need(await until(`document.querySelector('[aria-label="Pause (Space)"]')`, 5000), 'Shuffle did not start the album')
    await pause()
    // Shuffle from the album leaves shuffle on: put it back as it was.
    if (await run(`return !!document.querySelector('button[aria-pressed][aria-label="Shuffle is on"]')`)) await pressAt(`document.querySelector('button[aria-pressed][aria-label="Shuffle is on"]')`)
    const dbl = await dblPlay(`document.querySelectorAll('main [role=listitem]')[1]`, second)
    need(dbl.current, `double-click did not play "${second}"`)
    await rightAt(`document.querySelectorAll('main [role=listitem]')[0]`)
    const items = await menuItems()
    need(items.includes('Play next'), 'right-click shows no song menu: ' + items.join(', '))
    await esc()
    const artist = await run(`const h = document.querySelector('main h1'); const b = [...document.querySelectorAll('main button')].find((b) => __sm.vis(b) && h && (h.compareDocumentPosition(b) & 4) && b.textContent.trim() && !/^(Play|Shuffle|Edit details|Select|Back)$/.test(b.textContent.trim())); return b?.textContent.trim() ?? ''`)
    need(artist, 'no artist link on the album page')
    await press(artist, 'main')
    await sleep(900)
    const artistPage = await run(`return document.querySelector('main h1')?.textContent?.trim()`)
    need(artistPage && artistPage !== album, `Go to artist stayed on "${artistPage}"`)
    await back()
    need(dbl.on, `"${second}" on the album page: ${DBL}`)
    return `"${album}": ${rows} songs; Play, Shuffle, double-click played "${second}"; right-click ${items.length} items; artist "${artistPage}", Back`
  })

  await check('Music', 'Artists: list, an artist page with albums and songs', async () => {
    await rail('Library')
    await back()
    await tab('Artists')
    const first = await run(`return [...document.querySelectorAll('main button')].filter(__sm.vis).find((b) => /\\d+ albums?$/.test(b.textContent.trim()))?.textContent.trim() ?? ''`)
    need(first, 'no artists in the list')
    await press(first, 'main')
    await sleep(1200)
    const h = await run(`return document.querySelector('main h1')?.textContent?.trim()`)
    const m = await text()
    need(/\bAlbums\b/.test(m) && /\d+ songs/i.test(m), `the artist page "${h}" shows no albums and songs`)
    const songRows = await run(`return document.querySelectorAll('main [role=listitem]').length`)
    const cards = await run(`return document.querySelectorAll('main .card').length`)
    await back()
    return `"${first}" → "${h}" with ${cards} album cards, ${songRows} song rows`
  })

  await check('Music', 'Songs: whole library, sorting by column, quality badges, double-click/Enter to play', async () => {
    await rail('Library')
    await back()
    await tab('Songs')
    const head = await text()
    const total = head.match(/([\d,]+) songs/)?.[1]
    const sortOf = (c) => run(`return [...document.querySelectorAll('main [aria-sort]')].find((e) => e.textContent.trim() === ${J(c)})?.getAttribute('aria-sort')`)
    const firstRow = () => run(`return document.querySelector('main [role=listitem]')?.innerText.split('\\n')[0]`)
    const before = await firstRow()
    await press('Artist', 'main')
    await sleep(800)
    const s1 = await sortOf('Artist')
    const byArtist = await firstRow()
    await press('Artist', 'main')
    await sleep(800)
    const s2 = await sortOf('Artist')
    await press('Title', 'main')
    await sleep(800)
    if ((await sortOf('Title')) !== 'ascending') {
      await press('Title', 'main')
      await sleep(800)
    }
    need(s1 === 'ascending' && s2 === 'descending', `Artist sort went ${s1} then ${s2}`)
    need(byArtist !== before, 'sorting by Artist did not change the first row')
    const badges = await run(`return [...document.querySelectorAll('main [role=listitem]')].filter((r) => /(FLAC|MP3|AAC|ALAC|WAV|Opus|Hi-Res)/.test(r.innerText)).length`)
    need(badges > 0, 'no quality badges on rows')
    const third = await run(`return document.querySelectorAll('main [role=listitem]')[2]?.innerText.split('\\n')[0].trim()`)
    const dbl = await dblPlay(`document.querySelectorAll('main [role=listitem]')[2]`, third)
    need(dbl.current, `double-click did not play "${third}"`)
    const fourth = await run(`const r = document.querySelectorAll('main [role=listitem]')[3]; r.focus(); return r.innerText.split('\\n')[0].trim()`)
    await t.key('Enter')
    need(await until(`document.querySelector('[aria-label="Now playing"]')?.innerText.includes(${J(fourth)})`, 5000), `Enter did not play "${fourth}"`)
    await sleep(1500)
    const enterOn = await playing()
    await pause()
    need(enterOn, `Enter on "${fourth}" did not leave it playing`)
    need(dbl.on, `"${third}" in Songs: ${DBL}`)
    return `${total} songs; Artist sorts ascending then descending; badges on ${badges} rows; double-click played "${third}", Enter played "${fourth}"`
  })

  await check('Music', 'Playlists: list, Find a playlist, New playlist, Import playlists, a playlist page', async () => {
    await rail('Library')
    await back()
    await tab('Playlists')
    const cards = await run(`return document.querySelectorAll('main .card').length`)
    need(cards > 0, 'no playlist cards')
    need(await has('New playlist', 'main'), 'no New playlist')
    need(await has('Import & export', 'main'), 'no Import & export')
    await focusField('main input[aria-label="Find a playlist"]')
    await typeIn('Marathi')
    await sleep(600)
    const found = await run(`return [...document.querySelectorAll('main .card')].filter(__sm.vis).map((c) => c.querySelector('.font-medium')?.textContent?.trim())`)
    await clearField()
    await blur()
    need(found.length >= 1 && found.every((f) => /marathi/i.test(f ?? '')), 'Find a playlist "Marathi" showed: ' + found.join(', '))
    await pressAt(`[...document.querySelectorAll('main .card')].find((c) => c.querySelector('.font-medium')?.textContent?.trim() === 'Marathi')?.querySelector('button')`)
    await sleep(1200)
    const rows = await run(`return document.querySelectorAll('main [role=listitem]').length`)
    const h = await run(`return document.querySelector('main h1, main input[aria-label="Playlist name"]')?.textContent || document.querySelector('main input[aria-label="Playlist name"]')?.value`)
    await back()
    need(rows > 0, 'the Marathi playlist page shows no songs')
    return `${cards} cards; Find "Marathi" → ${found.length}; "${h}" opened with ${rows} rows`
  })

  await check('Music', 'Search (Music): songs, albums, artists, lyrics, across scripts, "/" to focus', async () => {
    await rail('Home')
    await blur()
    await t.key('/')
    need(await until(`document.activeElement?.getAttribute('aria-label') === 'Search your music'`, 3000), '"/" did not focus the search')
    await clearField()
    await typeIn('arijit')
    await t.key('Enter')
    await sleep(1800)
    const m = await text()
    for (const w of ['Songs', 'Albums', 'Artists']) need(m.includes(w), `"arijit" found no ${w}`)
    // Devanagari for a song whose title is in English letters.
    await focusField('input[aria-label="Search your music"]')
    await clearField()
    await typeIn('सनम रे')
    await t.key('Enter')
    await sleep(1800)
    const m2 = await text()
    need(/Sanam Re/i.test(m2), '"सनम रे" did not find Sanam Re')
    // Words only in the lyrics (Chaleya: "Ishq mein dil bana hai").
    await focusField('input[aria-label="Search your music"]')
    await clearField()
    await typeIn('dil bana hai')
    await t.key('Enter')
    await sleep(1800)
    const m3 = await text()
    need(m3.includes('In the lyrics') && /Chaleya/.test(m3), '"dil bana hai" found nothing in the lyrics')
    await focusField('input[aria-label="Search your music"]')
    await clearField()
    await blur()
    return '"/" focuses; "arijit" → songs, albums, artists; "सनम रे" → Sanam Re; "dil bana hai" → Chaleya in the lyrics'
  })

  await check('Music', 'Right-click and "…" on songs, albums, artists, playlists', async () => {
    const out = []
    const wantFor = {
      song: ['Play', 'Play next', 'Add to queue', 'Add to playlist…', 'Go to album', 'Edit details'],
      album: ['Play', 'Shuffle', 'Play next', 'Add to queue', 'Add to playlist…', 'Edit details'],
      artist: ['Play', 'Shuffle', 'Play next', 'Add to queue', 'Add to playlist…'],
      playlist: ['Play', 'Shuffle', 'Play next', 'Add to queue'],
    }
    const missing = []
    const take = (kind, items) => {
      const miss = wantFor[kind].filter((w) => !items.includes(w))
      if (kind === 'song' && !items.some((i) => i.startsWith('Go to ') && i !== 'Go to album')) miss.push('Go to artist')
      if (kind === 'album' && !items.some((i) => i.startsWith('Go to '))) miss.push('Go to artist')
      out.push(`${kind} ${items.length}`)
      if (miss.length) missing.push(`${kind}: ${miss.join(', ')}`)
    }
    await rail('Library')
    await back()
    await tab('Songs')
    await rightAt(`document.querySelector('main [role=listitem]')`)
    take('song', await menuItems())
    await esc()
    await pressAt(`[...document.querySelectorAll('main [role=listitem] button')].find((b) => (b.getAttribute('aria-label') ?? '').startsWith('More for '))`)
    const dots = await menuItems()
    take('song', dots)
    await esc()
    await tab('Albums')
    await rightAt(`document.querySelector('main .card')`)
    take('album', await menuItems())
    await esc()
    await tab('Artists')
    await rightAt(`[...document.querySelectorAll('main button')].filter(__sm.vis).find((b) => /\\d+ albums?$/.test(b.textContent.trim()))`)
    take('artist', await menuItems())
    await esc()
    await tab('Playlists')
    await rightAt(`document.querySelector('main .card')`)
    take('playlist', await menuItems())
    await esc()
    need(!missing.length, 'missing: ' + missing.join('; '))
    return out.join(', ') + ' (right-click and "…" on a song)'
  })

  await check('Music', 'Ratings on songs (Now Playing, menus); Edit details for an album', async () => {
    // Ratings are likes now (Add to Liked songs): looked at, not changed.
    await rail('Library')
    await back()
    await tab('Songs')
    await rightAt(`document.querySelector('main [role=listitem]')`)
    const items = await menuItems()
    await esc()
    need(items.some((i) => /Liked songs/.test(i)), 'the song menu has no Liked songs')
    const rowHeart = await run(`return !!document.querySelector('main [role=listitem] button[aria-label*="Liked songs"]')`)
    need(rowHeart, 'rows have no like button')
    await press('Open Now Playing')
    await until(`document.querySelector('section[aria-label="Now Playing"]')`)
    const npHeart = await run(`return !!document.querySelector('section[aria-label="Now Playing"] button[aria-label*="Liked songs"]')`)
    await esc()
    need(npHeart, 'Now Playing has no like button')
    return 'like in rows, menus and Now Playing (not toggled); album Edit details checked above (opened, closed unsaved)'
  })
}

// ===============================================================================================
// Lyrics in English letters and in your language (new)
// ===============================================================================================

if (want('lyrics')) {
  begin('lyrics')
  await check('New 0.4.0', 'Lyrics in English letters (Pronunciation) in Now Playing', async () => {
    need(await playSearch('Sanam Re', 'Sanam Re (From "Sanam Re")'), 'Sanam Re did not start')
    await pause()
    await press('Open Now Playing')
    await until(`document.querySelector('section[aria-label="Now Playing"]')`)
    await press('Lyrics', 'section[aria-label="Now Playing"]')
    need(await until(`document.querySelector('[aria-label="Under each line"]')`, 5000), 'no Pronunciation and Translation switches on a Hindi song')
    const on = () => run(`return [...document.querySelectorAll('[aria-label="Under each line"] button')].map((b) => b.textContent.trim() + '=' + b.getAttribute('aria-pressed'))`)
    const start = await on()
    if (start.includes('Pronunciation=true')) await press('Pronunciation', '[aria-label="Under each line"]')
    await press('Pronunciation', '[aria-label="Under each line"]')
    await sleep(2000)
    const latin = await run(`const np = document.querySelector('section[aria-label="Now Playing"]'); return np.innerText.includes('Bhigi-bhigi') || /\\n[A-Z][a-z]+[- ][a-z]+.*karun/.test(np.innerText)`)
    const note = await run(`return document.querySelector('[aria-label="Under each line"]').parentElement.innerText.split('\\n').slice(2).join(' ')`)
    // T toggles it too.
    await blur()
    await t.key('t')
    await sleep(600)
    const afterT = await on()
    if (afterT.includes('Pronunciation=true')) await press('Pronunciation', '[aria-label="Under each line"]')
    await esc()
    need(latin, 'no English-letter lines under the Devanagari ones')
    need(afterT.includes('Pronunciation=false'), 'T did not switch Pronunciation off: ' + afterT.join(', '))
    return `Devanagari lines get "Bhigi-bhigi sadkon…" under them; ${note || 'no note'}; T switches it`
  })

  await check('New 0.4.0', 'Translation in Now Playing > Lyrics (no online engine)', async () => {
    await press('Open Now Playing')
    await until(`document.querySelector('section[aria-label="Now Playing"]')`)
    await press('Lyrics', 'section[aria-label="Now Playing"]')
    need(await until(`document.querySelector('[aria-label="Under each line"]')`, 5000), 'no switches')
    const pressed = () => run(`return [...document.querySelectorAll('[aria-label="Under each line"] button')].find((b) => b.textContent.trim() === 'Translation')?.getAttribute('aria-pressed')`)
    if ((await pressed()) === 'true') await press('Translation', '[aria-label="Under each line"]')
    await press('Translation', '[aria-label="Under each line"]')
    await sleep(1500)
    const p = await pressed()
    const note = await run(`return document.querySelector('[aria-label="Under each line"]').parentElement.innerText.split('\\n').slice(2).join(' ')`)
    const asks = await run(`return [...document.querySelector('[aria-label="Under each line"]').parentElement.querySelectorAll('button')].map((b) => b.textContent.trim()).filter((x) => !['Pronunciation', 'Translation'].includes(x))`)
    await press('Translation', '[aria-label="Under each line"]')
    await esc()
    need(p === 'true', 'Translation did not switch on')
    need(note.length > 10, 'Translation on says nothing about where a translation comes from')
    return `on: "${note.slice(0, 160)}"${asks.length ? `; offers ${asks.join(', ')} (not pressed)` : ''}; switched off again`
  })
  person('New 0.4.0', 'Translation made on this PC or by DeepL/Microsoft/Google/Claude', 'needs a model download or a key and the network: not done from a test run')
}

// ===============================================================================================
// The real windows: each once, closed at once
// ===============================================================================================

let stopped = false
async function oneWindow(kind, open, close) {
  const before = (await targets()).map((x) => x.id)
  await open()
  let w = null
  for (let i = 0; i < 50 && !w; i++) {
    await sleep(100)
    w = (await targets()).find((x) => !before.includes(x.id) && x.url.includes('#' + kind))
  }
  let inside = null
  if (w) {
    await sleep(1500)
    try {
      const s = await session((x) => x.id === w.id)
      inside = await s.run(`return { text: document.body.innerText.slice(0, 600), buttons: [...document.querySelectorAll('button')].map((b) => b.getAttribute('aria-label') || b.textContent.trim()).filter(Boolean) }`)
      await s.shot(`window-${kind}`)
      await s.close()
    } catch (e) {
      inside = { error: e.message }
    }
  }
  await close(w)
  let gone = false
  for (let i = 0; i < 50 && !gone; i++) {
    await sleep(100)
    gone = !(await targets()).some((x) => x.url.includes('#' + kind))
  }
  if (!gone) {
    stopped = true
    throw new Error(`the ${kind} window did not close: STOPPED, close it by hand`)
  }
  return { w, inside }
}

if (want('windows')) {
  begin('windows')
  const lyricsWindow = (kind) => async () => {
    await run(`await window.__TAURI_INTERNALS__.invoke('lyrics_window', { kind: ${J(kind)}, open: false, place: null }); return true`)
  }
  await check('Playing', 'Big screen: open, follows the song, close', async () => {
    if (!(await run(`return document.querySelector('[aria-label="Now playing"]')?.innerText.includes('Sanam Re')`))) await playSearch('Sanam Re', 'Sanam Re (From "Sanam Re")')
    await pause()
    // On a sung line (1:00, "सनम रे…"), so the windows have words to show.
    await run(`await window.__TAURI_INTERNALS__.invoke('player_seek', { ms: 61000 }); return true`)
    await sleep(800)
    await press('Open Now Playing')
    await until(`document.querySelector('section[aria-label="Now Playing"]')`)
    await press('Lyrics', 'section[aria-label="Now Playing"]')
    const { w, inside } = await oneWindow('stage', () => press('Big screen (F11)', 'section[aria-label="Now Playing"]'), lyricsWindow('stage'))
    await esc()
    need(w, 'no big screen window appeared in the inspector list')
    need(inside && !inside.error, 'could not read the big screen: ' + inside?.error)
    need(/[\u0900-\u097F]/.test(inside.text) || /Sanam/i.test(inside.text), 'the big screen does not show the song playing: ' + inside.text.slice(0, 120))
    return `opened (${w.url.split('/').pop()}), shows "${inside.text.split('\n').filter(Boolean).slice(0, 2).join(' / ').slice(0, 80)}", closed`
  })
  if (!stopped)
    await check('Playing', 'Desktop lyrics: open, follows the song, close', async () => {
      await press('Open Now Playing')
      await until(`document.querySelector('section[aria-label="Now Playing"]')`)
      await press('Lyrics', 'section[aria-label="Now Playing"]')
      const { w, inside } = await oneWindow('overlay', () => press('Desktop lyrics (Ctrl+Shift+D)', 'section[aria-label="Now Playing"]'), lyricsWindow('overlay'))
      await esc()
      need(w, 'no desktop lyrics window appeared in the inspector list')
      need(inside && !inside.error, 'could not read the desktop lyrics: ' + inside?.error)
      need(/[\u0900-\u097F]/.test(inside.text) || /Sanam/i.test(inside.text), 'the desktop lyrics do not show the song playing: ' + inside.text.slice(0, 120))
      return `opened, shows "${inside.text.split('\n').filter(Boolean).slice(0, 1).join('').slice(0, 60)}", buttons [${inside.buttons.join(', ')}], closed`
    })
  person('Playing', 'Big screen and desktop lyrics: lock/unlock', 'click-through on a real window over the desktop: a person checks it (the page only sees the setting)')
  if (!stopped)
    await check('Playing', 'Mini player (Ctrl+M): controls, Up next, lyrics line, pin, back to the full window', async () => {
      const { w, inside } = await oneWindow(
        'mini',
        () => press('Mini player (Ctrl M)'),
        async (win) => {
          // Back to Venuta, pressed in the mini player itself; the command if that fails.
          if (win) {
            try {
              const s = await session((x) => x.id === win.id)
              await s.run(`document.querySelector('[aria-label="Back to Venuta"]')?.click(); return true`)
              await s.close()
            } catch {}
          }
          await sleep(1200)
          if ((await targets()).some((x) => x.url.includes('#mini'))) await run(`await window.__TAURI_INTERNALS__.invoke('mini_player', { open: false }); return true`)
        },
      )
      need(w, 'no mini player window appeared in the inspector list')
      need(inside && !inside.error, 'could not read the mini player: ' + inside?.error)
      const b = inside.buttons
      const miss = ['Previous', 'Next', 'Lyrics', 'Keep on top', 'Back to Venuta'].filter((x) => !b.includes(x))
      need(!miss.length, 'the mini player lacks ' + miss.join(', ') + ' (has ' + b.join(', ') + ')')
      need(/Sanam Re/.test(inside.text), 'the mini player does not show the song: ' + inside.text.slice(0, 80))
      const main = await run(`return document.visibilityState`)
      return `opened with the song and [${b.join(', ')}]; Back to Venuta closed it; main window ${main}`
    })
  person('Playing', 'Mini player: Up next (Card size only), pin (Keep on top) behaviour, the lyrics line', 'resizing the window to Card and keeping it on top are real window states: a person checks them')
}

// ===============================================================================================
// Playing
// ===============================================================================================

if (want('playing') && !stopped) {
  begin('playing')
  // A queue of a few songs: Arijit's Aura from its album page.
  async function aura() {
    await closeLayers()
    await rail('Home')
    await blur()
    await t.key('/')
    await until(`document.activeElement?.getAttribute('aria-label') === 'Search your music'`, 3000)
    await clearField()
    await typeIn("Arijit's Aura")
    await t.key('Enter')
    await sleep(1500)
    await pressAt(`[...document.querySelectorAll('main .card')].find((c) => c.querySelector('.font-medium')?.textContent?.trim() === "Arijit's Aura")?.querySelector('button')`)
    await sleep(1000)
    need(/Arijit's Aura/.test(await run(`return document.querySelector('main h1')?.textContent ?? ''`)), "Arijit's Aura did not open")
  }

  await check('Playing', 'Play, pause (Space), next/previous (Ctrl+→/←), seek on the waveform and with keys', async () => {
    await aura()
    await mute()
    // The album from its first song (one click: a double-click pauses it again), so the queue is the album.
    await pressAt(`document.querySelector('main [role=listitem]')`)
    need(await until(`document.querySelector('[aria-label="Pause (Space)"]')`, 5000), 'a double-click on the first song did not start the album')
    await sleep(1500)
    const first = await barText()
    await blur()
    await t.key(' ')
    need(await until(`document.querySelector('[aria-label="Play (Space)"]')`, 3000), 'Space did not pause')
    await t.key(' ')
    need(await until(`document.querySelector('[aria-label="Pause (Space)"]')`, 3000), 'Space did not play again')
    await ctrlArrow('Right')
    await sleep(1200)
    const second = await barText()
    need(second.split('\n')[0] !== first.split('\n')[0], 'Ctrl+→ did not go to the next song')
    await ctrlArrow('Left')
    await sleep(1200)
    const prev = await barText()
    need(prev.split('\n')[0] === first.split('\n')[0], `Ctrl+← went to "${prev.split('\n')[0]}", not back to "${first.split('\n')[0]}"`)
    await pause()
    const p0 = await position()
    await blur()
    await t.key('ArrowRight')
    await sleep(700)
    const p1 = await position()
    need(p1 >= p0 + 4, `→ moved from ${p0}s to ${p1}s`)
    const dur = await run(`return +document.querySelector('[aria-label="Position in the song"]').getAttribute('aria-valuemax')`)
    const at = await run(`const r = document.querySelector('[aria-label="Position in the song"]').getBoundingClientRect(); return [r.left + r.width / 2, r.top + r.height / 2]`)
    await t.mouse(at[0], at[1])
    await sleep(800)
    const p2 = await position()
    need(Math.abs(p2 - dur / 2) <= Math.max(5, dur * 0.05), `a click in the middle of the waveform went to ${p2}s of ${dur}s`)
    return `Space pause/play; Ctrl+→ "${second.split('\n')[0]}", Ctrl+← back; → ${p0}→${p1}s; waveform middle → ${p2}s of ${dur}s`
  })

  await check('Playing', 'Volume and mute (while paused, 5% at most)', async () => {
    await pause()
    need(!(await playing()), 'not paused: volume left alone')
    await run(`window.__qaVolOk = true; return true`)
    try {
      await run(`const v = document.querySelector('input[aria-label=Volume]'); const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set; set.call(v, '5'); v.dispatchEvent(new Event('input', { bubbles: true })); return true`)
      const up = await until(`document.querySelector('button[aria-label=Mute]')`, 2000)
      const val = await run(`return document.querySelector('input[aria-label=Volume]').value`)
      if (await has('Mute')) await press('Mute')
      const muted = await until(`document.querySelector('button[aria-label=Unmute]') && document.querySelector('input[aria-label=Volume]').value === '0'`, 2000)
      need(up && val === '5', `the volume slider did not take 5% (${val})`)
      need(muted, 'Mute did not bring the volume to 0')
      return 'slider to 5% shows Mute; Mute → 0 and Unmute (Unmute itself not pressed: it would raise the volume)'
    } finally {
      await mute()
      await run(`window.__qaVolOk = false; return true`)
    }
  })

  await check('Playing', 'Shuffle; repeat off/all/one', async () => {
    const SH = `document.querySelector('button[aria-pressed][aria-label="Shuffle"], button[aria-pressed][aria-label="Shuffle is on"]')`
    const shuffleWas = await run(`return ${SH}?.getAttribute('aria-label')`)
    await pressAt(SH)
    const now = await run(`return ${SH}?.getAttribute('aria-label')`)
    const flipped = now && now !== shuffleWas
    await pressAt(SH)
    need(flipped, 'the Shuffle button did not change')
    const seen = []
    for (let i = 0; i < 3; i++) {
      const l = await run(`return ['Repeat', 'Repeating all', 'Repeating this song'].find((x) => document.querySelector('button[aria-label="' + x + '"]'))`)
      seen.push(l)
      await press(l)
    }
    const end = await run(`return ['Repeat', 'Repeating all', 'Repeating this song'].find((x) => document.querySelector('button[aria-label="' + x + '"]'))`)
    need(new Set(seen).size === 3 && end === seen[0], 'repeat went ' + [...seen, end].join(' → '))
    return 'shuffle on and off; repeat ' + [...seen, end].join(' → ')
  })

  await check('Playing', 'Stop after this song; sleep timer', async () => {
    await press('Sound: equalizer, levelling, crossfade')
    await until(`document.querySelector('section[aria-label="Now Playing"]')`)
    const sw = () => run(`return document.querySelector('[role=switch][aria-label="Stop after this song"]')?.getAttribute('aria-checked')`)
    const s0 = await sw()
    need(s0 !== undefined && s0 !== null, 'no Stop after this song switch in Sound')
    await pressAt(`document.querySelector('[role=switch][aria-label="Stop after this song"]')`)
    const s1 = await sw()
    await pressAt(`document.querySelector('[role=switch][aria-label="Stop after this song"]')`)
    const s2 = await sw()
    await esc()
    need(s1 !== s0 && s2 === s0, `Stop after went ${s0} → ${s1} → ${s2}`)
    await sleep(500)
    await pressAt(`document.querySelector('button[aria-haspopup=menu][aria-label^="Sleep timer"]')`)
    await until(`document.querySelector('[role=menu][aria-label="Sleep timer"]')`, 2000)
    await sleep(400)
    const items = await run(`return [...document.querySelectorAll('[role=menu][aria-label="Sleep timer"] button')].map((b) => b.textContent.trim())`)
    need(items.some((i) => /In 15 minutes/.test(i)), 'the sleep timer offers: ' + items.join(', '))
    await press('In 15 minutes', '[role=menu][aria-label="Sleep timer"]', true)
    const on = await run(`return document.querySelector('button[aria-haspopup=menu][aria-label^="Sleep timer"]')?.getAttribute('aria-label')`)
    await pressAt(`document.querySelector('button[aria-haspopup=menu][aria-label^="Sleep timer"]')`)
    await sleep(400)
    await press('Turn off', '[role=menu][aria-label="Sleep timer"]', true)
    const off = await run(`return document.querySelector('button[aria-haspopup=menu][aria-label^="Sleep timer"]')?.getAttribute('aria-label')`)
    need(/15m left/.test(on ?? ''), 'In 15 minutes set the sleep timer to: ' + on)
    need(off === 'Sleep timer', 'the sleep timer did not turn off: ' + off)
    return `stop after on and off; sleep timer "${on}", then off`
  })

  await check('Playing', 'The queue: open (Q), jump, Alt+↑/↓, drag, remove, Save as playlist', async () => {
    await closeLayers()
    await blur()
    await t.key('q')
    need(await until(`document.querySelector('aside[aria-label=Queue]')`, 2000), 'Q did not open the queue')
    // A row's name says "playing now" or "played" after it: left out, so a song keeps its name.
    const rows = () => run(`return [...document.querySelectorAll('aside[aria-label=Queue] [role=listitem]')].map((r) => r.getAttribute('aria-label').replace(/, (playing now|played)$/, ''))`)
    const r0 = await rows()
    need(r0.length >= 4, `only ${r0.length} songs in the queue`)
    const cur = await run(`return [...document.querySelectorAll('aside[aria-label=Queue] [role=listitem]')].findIndex((r) => r.getAttribute('aria-current') === 'true')`)
    // Jump: a click on a song after the current one.
    const target = r0[cur + 2]
    await pressAt(`document.querySelectorAll('aside[aria-label=Queue] [role=listitem]')[${cur + 2}]`)
    await mute()
    const jumped = await until(`document.querySelector('aside[aria-label=Queue] [role=listitem][aria-current=true]')?.getAttribute('aria-label').replace(/, (playing now|played)$/, '') === ${J(target)}`, 4000)
    await pause()
    // Alt+↓ on the song after the current one.
    const r1 = await rows()
    const c1 = r1.findIndex((x) => x === target)
    const moving = r1[c1 + 1]
    await run(`document.querySelectorAll('aside[aria-label=Queue] [role=listitem]')[${c1 + 1}].focus(); return true`)
    await altArrow('Down')
    const r2 = await rows()
    const moved = r2.indexOf(moving) === c1 + 2
    // Does the song keep the focus, for another Alt+arrow?
    const kept = await run(`return (document.activeElement?.getAttribute('aria-label') ?? '').startsWith(${J(moving)})`)
    await run(`[...document.querySelectorAll('aside[aria-label=Queue] [role=listitem]')].find((r) => r.getAttribute('aria-label').startsWith(${J(moving)}))?.focus(); return true`)
    await altArrow('Up')
    const r3 = await rows()
    const movedBack = r3.indexOf(moving) === c1 + 1
    // Drag the grip of the song two after the current one, one row down.
    const grip = await run(`const r = document.querySelectorAll('aside[aria-label=Queue] [role=listitem]')[${c1 + 1}]; const b = r.getBoundingClientRect(); const h = r.querySelector('[aria-hidden], svg')?.getBoundingClientRect() ?? b; return [b.left + 12, b.top + b.height / 2, b.height]`)
    await t.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: grip[0], y: grip[1] })
    await t.send('Input.dispatchMouseEvent', { type: 'mousePressed', x: grip[0], y: grip[1], button: 'left', clickCount: 1 })
    for (let i = 1; i <= 12; i++) {
      await t.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x: grip[0], y: grip[1] + (grip[2] * 1.6 * i) / 12, button: 'left', buttons: 1 })
      await sleep(30)
    }
    await t.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: grip[0], y: grip[1] + grip[2] * 1.6, button: 'left', clickCount: 1 })
    await sleep(700)
    const r4 = await rows()
    const dragged = r4.indexOf(moving) > c1 + 1
    // Remove a song.
    // Remove a song a few after the one playing.
    await sleep(600)
    const removeLabel = await run(`const rs = [...document.querySelectorAll('aside[aria-label=Queue] [role=listitem]')]; const cur = rs.findIndex((r) => r.getAttribute('aria-current') === 'true'); return rs[cur + 3]?.querySelector('button[aria-label^="Remove "]')?.getAttribute('aria-label')`)
    need(removeLabel, 'no Remove button on queue songs')
    // The list draws only the rows in view: count from the header's "N up next".
    const upNext = () => run(`return +(document.querySelector('aside[aria-label=Queue] header')?.innerText.match(/([\\d,]+) up next/)?.[1]?.replace(/,/g, '') ?? -1)`)
    const n0 = await upNext()
    await pressAt(`[...document.querySelectorAll('aside[aria-label=Queue] button')].find((b) => b.getAttribute('aria-label') === ${J(removeLabel)})`)
    await sleep(600)
    const n1 = await upNext()
    const stillThere = (await rows()).some((r) => removeLabel.includes(r.split(',')[0]))
    const save = await has('Save as playlist', 'aside[aria-label=Queue]', true)
    await esc()
    need(jumped, 'a click on a queue song did not play it')
    need(moved && movedBack, `Alt+↓/↑ did not move "${moving}"`)
    need(n1 === n0 - 1 && !stillThere, `Remove: ${n0} → ${n1} up next${stillThere ? ', the song still listed' : ''}`)
    need(save, 'no Save as playlist in the queue')
    need(dragged, 'a drag on a queue song did not move it (Alt+arrows did)')
    need(kept, `after Alt+↓ the focus leaves "${moving.split(',')[0]}" (to the page), so a second Alt+arrow moves nothing`)
    return `${r0.length} songs; jump, Alt+↓/↑, drag, remove (${n0}→${n1}); Save as playlist there (used in Playlists)`
  })

  await check('Playing', 'Now Playing: open/close, Lyrics (styles, timing tools)', async () => {
    await press('Open Now Playing')
    need(await until(`document.querySelector('section[aria-label="Now Playing"]')`), 'Now Playing did not open')
    await press('Lyrics', 'section[aria-label="Now Playing"]')
    await press('Lyric style', 'section[aria-label="Now Playing"]')
    await sleep(500)
    const styles = await run(`return __sm.labels().filter((l) => l.startsWith('Sing along')).length`)
    await esc()
    const stillOpen = await nowOpen()
    if (!stillOpen) await press('Open Now Playing')
    await press('Lyrics', 'section[aria-label="Now Playing"]')
    // The maker: tap a line with Space, then leave without saving (Save timing is never pressed).
    await press('Fix the timing', 'section[aria-label="Now Playing"]')
    need(await until(`[...document.querySelectorAll('section[aria-label="Now Playing"] button')].some((b) => b.textContent.trim() === 'Save timing')`, 3000), 'Fix the timing opened no maker')
    await mute()
    await blur()
    await t.key(' ')
    await sleep(1500)
    await mute()
    const tapped = await run(`return /Press Space as each line begins|Every line has its time/.test(document.querySelector('section[aria-label="Now Playing"]').innerText)`)
    await pause()
    if (await has('Cancel', 'section[aria-label="Now Playing"]')) await press('Cancel', 'section[aria-label="Now Playing"]')
    if (await has('Leave without saving', 'section[aria-label="Now Playing"]')) await press('Leave without saving', 'section[aria-label="Now Playing"]')
    const left = await until(`![...document.querySelectorAll('section[aria-label="Now Playing"] button')].some((b) => b.textContent.trim() === 'Save timing')`, 3000)
    await press('Close Now Playing (Esc)')
    const closed = await until(`!document.querySelector('section[aria-label="Now Playing"]')`, 2000)
    need(styles >= 4, `Lyric style shows ${styles} styles`)
    need(stillOpen, 'Escape on the Lyric style popover closed Now Playing too')
    need(tapped, 'Space in the maker did not tap a line')
    need(left, 'the maker did not close without saving')
    need(closed, 'Close did not close Now Playing')
    return `${styles} lyric styles; maker: Space tapped, left without saving; closed`
  })

  await check('Playing', 'Now Playing: Up next, Sound (EQ, presets, tone, levelling, crossfade), Signal', async () => {
    await press('Open Now Playing')
    await until(`document.querySelector('section[aria-label="Now Playing"]')`)
    const np = 'section[aria-label="Now Playing"]'
    await press('Up next', np)
    await sleep(800)
    const upNext = await text(np)
    const hasNext = /Autoplay/.test(upNext) || (await run(`return document.querySelectorAll('section[aria-label="Now Playing"] [role=listitem]').length`)) > 0
    await press('Sound', np)
    await sleep(800)
    const s = await text(np)
    const bitPerfect = /you hear the file exactly as it is/.test(s)
    const eq = await run(`return ['31 Hz', 'Preamp', 'Bass', 'Treble', 'Crossfade, seconds'].filter((l) => !document.querySelector('section[aria-label="Now Playing"] input[aria-label="' + l + '"]'))`)
    const lev = ['Off', 'Per song', 'Per album'].every((x) => s.includes(x))
    await press('Bass boost', np)
    await sleep(600)
    const boosted = await run(`return +document.querySelector('section[aria-label="Now Playing"] input[aria-label="31 Hz"]').value`)
    await press('Flat', np)
    await sleep(400)
    if (bitPerfect) await press('Turn everything off (bit-perfect)', np).catch(() => {})
    await sleep(500)
    const back = /you hear the file exactly as it is/.test(await text(np))
    await press('Signal', np)
    await sleep(900)
    const sig = await text(np)
    const parts = ['File', 'Decoder', 'Output', 'Device'].filter((p) => !sig.includes(p))
    const out = sig.match(/Output\n([^\n]+)/)?.[1]
    await esc()
    need(hasNext, 'Up next shows nothing')
    need(!eq.length, 'Sound lacks ' + eq.join(', '))
    need(lev, 'no levelling choices')
    need(boosted > 0, 'Bass boost left 31 Hz at ' + boosted)
    need(!bitPerfect || back, 'Turn everything off did not bring the sound back to bit-perfect')
    need(!parts.length, 'Signal lacks ' + parts.join(', '))
    return `Up next; EQ preset Bass boost 31 Hz +${boosted} dB then Flat, everything off again; tone, levelling, crossfade there; Signal output "${out}"`
  })

  person('Playing', 'Lyrics maker: save, and Undo from Activity', 'saving writes the lyrics file next to the song (hard link to the real library): not done')
  person('Playing', 'Media keys, the Windows media panel, the tray, close-to-tray, start with Windows', 'Windows itself: a person checks them')
  person('Playing', 'Gapless', 'heard, not seen: a person listens across an album join')
  person('Playing', 'Exclusive (bit-perfect) output', 'switching output mode is a system-level audio change: a person checks it (Signal is read above)')
}

// ===============================================================================================
// Playlists
// ===============================================================================================

const PL = 'D:/UIImprovement/venuta-perf/Playlists'
const plFile = (name) => existsSync(join(process.env.QA_PLAYLISTS ?? PL, `${name}.m3u8`))
const made = new Set()
async function openPlaylist(name) {
  await rail('Library')
  await back()
  await tab('Playlists')
  await sleep(600)
  await pressAt(`[...document.querySelectorAll('main .card')].find((c) => c.querySelector('.font-medium')?.textContent?.trim().replace(/^✦\\s*/, '') === ${J(name)})?.querySelector('button')`)
  await sleep(1200)
}
async function pageMenu(item) {
  await press('More for this playlist', 'main')
  await sleep(400)
  const items = await menuItems()
  if (!item) return items
  await pressAt(`[...document.querySelectorAll('[role=menu] [role=menuitem]')].find((e) => e.textContent.trim() === ${J(item)})`)
  await sleep(600)
  return items
}
// A row's title: its first line that isn't its number.
const plRows = () => run(`return [...document.querySelectorAll('main [role=listitem]')].map((r) => r.innerText.split('\\n').find((l) => l.trim() && !/^\\d+$/.test(l.trim())))`)
async function toastUndo() {
  const ok = await until(`[...document.querySelectorAll('[role=status] button, [role=alert] button')].some((b) => b.textContent.trim() === 'Undo')`, 3000)
  if (!ok) return false
  await pressAt(`[...document.querySelectorAll('[role=status] button, [role=alert] button')].find((b) => b.textContent.trim() === 'Undo')`)
  await sleep(800)
  return true
}
async function setAside(name) {
  await openPlaylist(name)
  await pageMenu('Delete…')
  await until(`document.querySelector('[role=alertdialog]')`, 2000)
  await press('Delete playlist', '[role=alertdialog]')
  await sleep(1000)
}

if (want('playlists') && !stopped) {
  begin('playlists')
  const renamed = `${TEST} renamed`

  await check('Playlists', 'Create, rename', async () => {
    need(!plFile(TEST) && !plFile(renamed), `a "${TEST}" playlist is already there: set it aside first`)
    await rail('Library')
    await back()
    await tab('Playlists')
    await press('New playlist', 'main')
    need(await until(`document.activeElement?.getAttribute('aria-label') === 'Playlist name'`, 3000), 'New playlist did not focus its name')
    await clearField()
    await typeIn(TEST)
    await t.key('Enter')
    await sleep(800)
    // A new playlist is a draft until it has a song: add one from the page picker.
    if (await has('Add songs', 'main')) await press('Add songs', 'main')
    await until(`document.querySelector('main input[aria-label="Find songs to add"]')`, 3000)
    await focusField('main input[aria-label="Find songs to add"]')
    await typeIn('Kesariya')
    await sleep(1200)
    await pressAt(`[...document.querySelectorAll('main button')].find((b) => /^Add Kesariya/.test(b.getAttribute('aria-label') ?? ''))`)
    await sleep(1200)
    if (await has('Done adding', 'main')) await press('Done adding', 'main')
    await sleep(600)
    need(await until(`true`, 10) && plFile(TEST), `no ${TEST}.m3u8 in the copy's Playlists after New playlist and a song`)
    made.add(TEST)
    await pageMenu('Rename')
    need(await until(`document.activeElement?.getAttribute('aria-label') === 'Playlist name'`, 3000), 'Rename did not focus the name')
    await clearField()
    await typeIn(renamed)
    await t.key('Enter')
    await sleep(1200)
    need(plFile(renamed) && !plFile(TEST), `rename: ${renamed}.m3u8 ${plFile(renamed) ? 'there' : 'missing'}, old ${plFile(TEST) ? 'still there' : 'gone'}`)
    made.delete(TEST)
    made.add(renamed)
    return `"${TEST}" made (page picker added Kesariya), renamed to "${renamed}" (files checked)`
  })

  await check('Playlists', 'Add songs (page picker, menus, from the queue), remove, reorder, Undo on remove', async () => {
    need(plFile(renamed), 'no test playlist to add to')
    await openPlaylist(renamed)
    // The page draws only the rows in view: count from its "N songs".
    const plCount = async () => +((await text()).match(/([\d,]+) songs?/)?.[1]?.replace(/,/g, '') ?? -1)
    const n0 = await plCount()
    // A song's menu: Add to playlist… → the test playlist.
    await rail('Library')
    await back()
    await tab('Songs')
    await rightAt(`document.querySelectorAll('main [role=listitem]')[1]`)
    await pressAt(`[...document.querySelectorAll('[role=menu] [role=menuitem]')].find((e) => e.textContent.trim() === 'Add to playlist…')`)
    need(await until(`document.querySelector('[role=dialog][aria-label="Add to playlist"]')`, 3000), 'Add to playlist… opened no chooser')
    await until(`[...document.querySelectorAll('[role=dialog][aria-label="Add to playlist"] button')].some((b) => b.textContent.trim().startsWith(${J(renamed)}))`, 15000)
    await pressAt(`[...document.querySelectorAll('[role=dialog][aria-label="Add to playlist"] button')].find((b) => b.textContent.trim().startsWith(${J(renamed)}))`)
    await sleep(1000)
    await closeLayers()
    // From the queue: Save as playlist → the test playlist.
    await blur()
    await t.key('q')
    await until(`document.querySelector('aside[aria-label=Queue]')`, 2000)
    await press('Save as playlist', 'aside[aria-label=Queue]', true)
    await sleep(600)
    const chooser = await until(`document.querySelector('[role=dialog][aria-label="Add to playlist"]')`, 3000)
    if (chooser) await until(`[...document.querySelectorAll('[role=dialog][aria-label="Add to playlist"] button')].some((b) => b.textContent.trim().startsWith(${J(renamed)}))`, 15000)
    await pressAt(`[...document.querySelectorAll('[role=dialog][aria-label="Add to playlist"] button')].find((b) => b.textContent.trim().startsWith(${J(renamed)}))`)
    await sleep(1200)
    await closeLayers()
    await openPlaylist(renamed)
    const r1 = await plRows()
    const c1 = await plCount()
    need(c1 > n0 + 1, `menus and the queue added ${c1 - n0} songs (had ${n0})`)
    // Reorder: Alt+↓ on the first row.
    const firstSong = r1[0]
    await run(`document.querySelector('main [role=listitem]').focus(); return true`)
    await altArrow('Down')
    await sleep(800)
    const r2 = await plRows()
    const c2 = await plCount()
    const reordered = r2[1] === firstSong
    // Remove, then Undo.
    const removeBtn = await run(`return [...document.querySelectorAll('main button')].map((b) => b.getAttribute('aria-label') ?? '').find((l) => /^Remove .* from the playlist$/.test(l))`)
    need(removeBtn, 'no Remove on playlist rows')
    await pressAt(`[...document.querySelectorAll('main button')].find((b) => b.getAttribute('aria-label') === ${J(removeBtn)})`)
    await sleep(900)
    const c3 = await plCount()
    const undone = await toastUndo()
    await sleep(800)
    const c4 = await plCount()
    need(reordered, `Alt+↓ did not move "${firstSong}" down`)
    need(c3 === c2 - 1, `Remove: ${c2} → ${c3} songs`)
    need(undone && c4 === c2, `Undo on remove: ${undone ? `${c4} of ${c2} back` : 'no Undo in the notice'}`)
    return `page picker (above), a song's menu and the queue's Save as playlist added → ${c1} songs; Alt+↓ moves; remove ${c2}→${c3}, Undo → ${c4}`
  })

  await check('Playlists', 'Duplicate, delete (Set aside), export as a file', async () => {
    need(plFile(renamed), 'no test playlist')
    await openPlaylist(renamed)
    const items = await pageMenu('Duplicate')
    await sleep(1200)
    const copy = readdirSync(PL).map((f) => f.replace(/\.m3u8$/, '')).find((f) => f !== renamed && f.startsWith(renamed))
    need(copy, 'Duplicate made no copy: menu was ' + items.join(', '))
    made.add(copy)
    await openPlaylist(renamed)
    await pageMenu('Export…')
    await sleep(1000)
    const send = await text()
    const chosen = await run(`return [...document.querySelectorAll('main input[type=checkbox]')].filter((c) => c.checked).length`)
    await back()
    need(/Send out/.test(send) && chosen === 1, `Export… opened ${/Send out/.test(send) ? 'Send out' : 'something else'} with ${chosen} playlists chosen`)
    await setAside(copy)
    const aside = existsSync(join(PL, '..', 'Set aside', 'playlists'))
      ? readdirSync(join(PL, '..', 'Set aside', 'playlists')).some((f) => f.startsWith(copy))
      : false
    need(!plFile(copy), `"${copy}" still in Playlists after Delete…`)
    made.delete(copy)
    return `Duplicate → "${copy}"; Export… opens Send out with it chosen (nothing written); Delete… set "${copy}" aside${aside ? ' (in Set aside)' : ''}`
  })

  await check('Playlists', 'Smart playlists: rules, live results', async () => {
    await openPlaylist('Recently added')
    const m = await text()
    const rows = (await plRows()).length
    const items = await pageMenu()
    await esc()
    await back()
    need(/Smart/i.test(m), 'Recently added does not say it is smart')
    need(rows > 0, 'Recently added shows no songs')
    need(/rule|added in|Added|newest|Edit rules/i.test(m) || items.some((i) => /rules/i.test(i)), 'no rules shown on the smart playlist')
    return `Recently added: smart, ${rows} songs shown, rules on the page`
  })

  await check('Playlists', 'Imported playlists: songs to get, Find this song, Update from the source, Complete your Apple Music playlists', async () => {
    await rail('Library')
    await back()
    await tab('Playlists')
    const complete = await run(`return !!document.querySelector('main [aria-label="Complete your Apple Music playlists"]') || /Complete your Apple Music playlists/.test(document.querySelector('main').innerText)`)
    await openPlaylist('Hindi (Apple, whole)')
    const m = await text()
    // A song to get: its menu's Find it in your library… opens Find this song.
    await pressAt(`[...document.querySelectorAll('main button')].find((b) => /^To get \\d+/.test(b.textContent.trim()))`)
    await sleep(1000)
    await pressAt(`document.querySelector('main [role=listitem] button[aria-label^="More for "]')`)
    const rowItems = await menuItems()
    if (rowItems.includes('Find it in your library…')) await pressAt(`[...document.querySelectorAll('[role=menu] [role=menuitem]')].find((e) => e.textContent.trim() === 'Find it in your library…')`)
    const find = await until(`document.querySelector('[aria-label="Find this song"]')`, 3000)
    const field = await run(`return !!document.querySelector('[aria-label="Find this song"] input, input[aria-label="Find the song"]')`)
    await esc()
    await pressAt(`[...document.querySelectorAll('main button')].find((b) => /^All \\d+/.test(b.textContent.trim()))`)
    const items = await pageMenu()
    await esc()
    await back()
    need(/to get/i.test(m), 'no songs to get kept in place')
    need(find && field, `a song to get: Find it in your library… ${find ? 'opened Find this song with no field' : 'opened nothing'} (menu: ${rowItems.join(', ')})`)
    need(items.some((i) => /^Update from/.test(i)), 'no Update from … in the menu: ' + items.join(', '))
    return `"Hindi (Apple, whole)": songs to get, Find it in your library… → Find this song, ${items.find((i) => /^Update from/.test(i))} (not run); Complete your Apple Music playlists ${complete ? 'shown' : 'not shown (it shows only when the Apple Music library has more than these playlists)'}`
  })

  await check('Playlists', 'Import & export: Bring in, a list of songs, Review, Import', async () => {
    await rail('Library')
    await back()
    await tab('Playlists')
    await press('Import & export', 'main')
    await sleep(1000)
    need(await has('Bring in', 'main') && (await has('Send out', 'main')), 'no Bring in and Send out')
    // A list of songs, "Artist - Title" a line: read into Review, imported, then set aside.
    await focusField('main textarea')
    await typeIn('Arijit Singh - Kesariya\nArijit Singh - Sanam Re\nNobody At All - No Such Song Here')
    await sleep(500)
    await pressAt(`[...document.querySelectorAll('main button')].find((b) => b.textContent.trim() === 'Bring in' && b.getAttribute('role') !== 'tab')`)
    need(await until(`[...document.querySelectorAll('main button')].some((b) => b.textContent.trim() === 'Import playlist')`, 8000), 'the pasted list never reached Review')
    const review = await text()
    const counts = review.match(/(\d+) found\n(\d+) to get/)
    need(counts && +counts[1] >= 1 && +counts[2] >= 1, 'Review: ' + review.slice(0, 200))
    await focusField('main input[aria-label="Name for this playlist"]')
    await clearField()
    await typeIn(`${TEST} import`)
    const before = readdirSync(PL)
    await press('Import playlist', 'main')
    await sleep(2500)
    const added = readdirSync(PL).filter((f) => !before.includes(f))
    added.forEach((f) => made.add(f.replace(/\.m3u8$/, '')))
    const done = await text()
    const undo = (await labels('main')).filter((l) => /Undo/.test(l))
    await back()
    need(added.length === 1, 'Import playlist wrote ' + (added.join(', ') || 'nothing'))
    return `a pasted list of 3 → Review (${counts[1]} found, ${counts[2]} to get) → "${added[0]}" written; the done step ${undo.length ? 'offers Undo' : 'offers no Undo for a new playlist (set aside at the end)'}`
  })

  await check('Playlists', 'Where are your playlists?, Copy for Venuta bookmark, Send out choices', async () => {
    await rail('Library')
    await back()
    await tab('Playlists')
    await press('Import & export', 'main')
    await sleep(900)
    const services = (await labels('main')).filter((l) => /^(Spotify|Apple Music|YouTube Music|JioSaavn|Deezer|TIDAL)/.test(l))
    await press('Apple Music', 'main', true)
    await sleep(900)
    const steps = await text()
    const bookmark = /Copy for Venuta/.test(steps)
    await back()
    await press('Import & export', 'main').catch(() => {})
    await sleep(600)
    await press('Send out', 'main')
    await sleep(900)
    const outs = (await labels('main')).filter((l) => /^(A file|A backup|YouTube Music|Spotify|Apple Music|Deezer|TIDAL|Copy as text)/.test(l))
    await back()
    need(services.length >= 6, 'Bring in names ' + services.length + ' services')
    need(bookmark, 'Apple Music steps show no Copy for Venuta bookmark')
    need(outs.length >= 7, 'Send out offers ' + outs.join(', '))
    return `${services.length} services with steps; the Copy for Venuta bookmark; Send out: ${outs.length} ways (none run)`
  })

  person('Playlists', 'Bring in: files, zips, folders dropped or chosen; share links; Spotify sign-in; Apple Music library; Google Takeout; phone playlists', 'file pickers, drops and the network: a person does them')
  person('Playlists', 'Review: close matches, Not it remembered, Update/Keep both/Replace, To get; Check them for new songs', 'needs an import of a real service playlist')
  person('Playlists', 'Send out: write a file, a phone or player, a server, a backup brought back, service links', 'writes files outside the copy or uses the network')
}

// ===============================================================================================
// Library
// ===============================================================================================

if (want('library') && !stopped) {
  begin('library')
  await check('Library', 'Health, Fix details: what\'s missing, filters, editing an album', async () => {
    await rail('Health')
    await tab('Fix details')
    await sleep(800)
    const all = await run(`return document.querySelectorAll('main [role=row], main button').length`)
    const filter = (await labels('main')).find((l) => /^No composer \d+/.test(l))
    need(filter, 'no No composer filter')
    await press(filter, 'main')
    await sleep(900)
    const rows = await run(`return [...document.querySelectorAll('main button')].filter(__sm.vis).filter((b) => /(19|20)\\d\\d(Composer|Complete|Synced lyrics|Sharp cover)/.test(b.textContent)).map((b) => b.textContent)`)
    const onlyComposer = rows.length > 0 && rows.every((r) => /Composer/.test(r))
    await pressAt(`[...document.querySelectorAll('main button')].filter(__sm.vis).find((b) => /(19|20)\\d\\dComposer/.test(b.textContent))`)
    const editor = await until(`document.querySelector('[role=dialog][aria-label$=", details"]')`, 3000)
    const dirty = await has('Discard')
    await esc()
    const closed = await until(`!document.querySelector('[role=dialog][aria-label$=", details"]')`, 2000)
    await press('All', 'main', true)
    need(onlyComposer, 'No composer shows other albums too')
    need(editor, 'an album did not open its editor')
    need(!dirty && closed, 'the editor did not close cleanly')
    return `${filter}: ${rows.length} albums all missing a composer; the editor opened and closed unsaved`
  })

  await check('Library', 'Health: completeness, tools, issues and their lists, Lyrics without timing', async () => {
    await rail('Health')
    await tab('Overview')
    await sleep(800)
    const m = await text()
    for (const w of ['How complete your library is', 'Look up details online', 'Check quality', 'Look for changes', 'Lyrics without timing']) need(m.includes(w), `no "${w}"`)
    const pct = m.match(/(\d+)%\s*\nHow complete/)?.[1]
    await pressAt(`[...document.querySelectorAll('main button')].find((b) => /See albums$/.test(b.textContent.trim()))`)
    await sleep(1000)
    const sheet = await until(`[...document.querySelectorAll('[role=dialog]')].some((d) => /still without/.test(d.innerText))`, 3000)
    const sheetAlbums = await run(`return [...document.querySelectorAll('[role=dialog] button')].filter(__sm.vis).length`)
    await esc()
    await until(`![...document.querySelectorAll('[role=dialog]')].some((d) => /still without/.test(d.innerText))`, 2000)
    await sleep(600)
    if (!(await text()).includes('Lyrics without timing')) {
      await rail('Health')
      await tab('Overview')
    }
    await pressAt(`[...document.querySelectorAll('main button')].find((b) => /See songs$/.test(b.textContent.trim()))`)
    await sleep(1000)
    const songsSheet = await until(`document.querySelector('aside[aria-label="Lyrics without timing"]')`, 3000)
    const songsText = await run(`return document.querySelector('aside[aria-label="Lyrics without timing"]')?.innerText.slice(0, 80) ?? ''`)
    await esc()
    need(sheet, 'See albums opened no list of albums')
    need(songsSheet, 'See songs (Lyrics without timing) opened no list')
    return `${pct ?? '?'}% complete; tools listed (none run: online lookup and measuring); See albums opens a list (${sheetAlbums} buttons), See songs opens "${songsText.replace(/\n/g, ' ').slice(0, 50)}…"`
  })
  person('Settings', 'Export a theme', 'only your own themes have Export (a built-in offers "Make a copy to change"); none here: a person checks it with a made theme')
  person('Library', 'Health tools: Look up details online, Check quality, Look for changes', 'they read the network or the files at length and may write tags: not run on the real library')

  await check('Library', 'Organize: your library vs rules; a naming change previewed; New music', async () => {
    await rail('Organize')
    await tab('Your library')
    await sleep(1200)
    const m = await text()
    const rules = /matches your rules|change|Apply/.test(m)
    // Try another folder name: Organize shows what it would change; then back as it was.
    const was = await run(`return [...document.querySelectorAll('main [role=radio][aria-checked=true]')].map((b) => b.textContent.trim())[0]`)
    await press('1999 - Taal', 'main')
    await sleep(2500)
    const trial = await text()
    const preview = trial !== m
    await pressAt(`[...document.querySelectorAll('main [role=radio]')].find((b) => b.textContent.trim() === ${J(was)})`)
    await sleep(1500)
    const restored = await run(`return [...document.querySelectorAll('main [role=radio][aria-checked=true]')].map((b) => b.textContent.trim())[0]`)
    await tab('New music')
    const nm = await text()
    need(rules, 'Organize says nothing about the rules')
    need(preview, 'a different folder name showed no preview')
    need(restored === was, `the folder name was left at "${restored}" (was "${was}")`)
    need(/Drop album folders or files/.test(nm), 'New music has no drop area')
    return `"${m.match(/Your library matches your rules|[^\n]*change[^\n]*/)?.[0] ?? ''}"; "1999 - Taal" previewed then back to "${was}" (nothing saved); New music drop area`
  })
  person('Library', 'Organize: Apply, Undo; dropping files into New music', 'Apply moves and renames music files (hard links): never done here')

  await check('Library', 'Devices: the page', async () => {
    await rail('Devices')
    const m = await text()
    need(/SD cards and USB drives/.test(m) && /Phones over Wi-Fi/.test(m), 'Devices lacks its sections')
    return 'SD cards and USB drives, Phones over Wi-Fi shown (no card attached; Share with phones not started)'
  })
  person('Library', 'Devices: cards, selections, conversions, write, eject', 'needs a card, and writes to it: a person does it')

  await check('Library', 'Activity: every change with Undo', async () => {
    await ctrl('h')
    await sleep(1200)
    const m = await text()
    const undo = await run(`return [...document.querySelectorAll('main button')].filter((b) => /^Undo/.test(b.textContent.trim())).length`)
    need(/Activity/.test(m), 'Ctrl+H did not open Activity')
    need(undo > 0 || /Nothing/.test(m), 'Activity lists changes with no Undo')
    return `Activity opens (Ctrl+H) with ${undo} Undo buttons; nothing undone here`
  })
}

// ===============================================================================================
// Settings
// ===============================================================================================

async function settingsPage(name) {
  await ctrl(',')
  await sleep(800)
  await pressAt(`[...document.querySelectorAll('main button')].find((b) => b.textContent.trim().startsWith(${J(name)}) && __sm.vis(b))`)
  await sleep(1000)
  return text()
}

if (want('settings') && !stopped) {
  begin('settings')
  await check('Settings', 'Look and feel: themes, filters, search, New theme / Theme Studio, import/export, Mode, Colour, Window, Motion, App icon', async () => {
    const m = await settingsPage('Look and feel')
    const inUse = /In use/.test(m)
    const filters = ['All', 'Built-in', 'Created', 'Recent'].filter((f) => !m.includes(f))
    const modes = ['Light', 'Dark', 'Match Windows'].filter((f) => !m.includes(f))
    const rows = ['Colour', 'Window', 'Motion', 'App icon'].filter((f) => !m.includes(f))
    const cards0 = await run(`return __sm.labels('main').filter((l) => l.startsWith('More for ')).length`)
    await press('Find a theme', 'main')
    await focusField('main input[placeholder="Find a theme"]')
    await typeIn('Fjord')
    await sleep(700)
    const cards1 = await run(`return __sm.labels('main').filter((l) => l.startsWith('More for ')).length`)
    await clearField()
    await blur()
    await press('More for Lumen', 'main')
    const items = await menuItems()
    await esc()
    await press('Explore themes', 'main')
    const explore = await until(`[...document.querySelectorAll('[role=dialog]')].some((d) => /Import a theme file/.test(d.innerText))`, 3000)
    await esc()
    await press('New theme', 'main')
    await sleep(1200)
    const studio = await until(`document.querySelector('[role=dialog][aria-label="Theme Studio"]')`, 3000)
    await esc()
    await sleep(500)
    const studioEsc = await run(`return !document.querySelector('[role=dialog][aria-label="Theme Studio"]')`)
    if (!studioEsc) await press('Close the Theme Studio')
    await sleep(500)
    const studioClosed = await run(`return !document.querySelector('[role=dialog][aria-label="Theme Studio"]')`)
    need(inUse, 'no theme In use')
    need(!filters.length, 'no theme filters ' + filters.join(', '))
    need(!modes.length, 'no Mode ' + modes.join(', '))
    need(!rows.length, 'no ' + rows.join(', '))
    need(cards1 >= 1 && cards1 < cards0, `Find a theme "Fjord" left ${cards1} of ${cards0}`)
    need(items.length > 0, 'a theme has no menu')
    need(explore, 'Explore themes did not open with Import a theme file')
    need(studio, 'New theme did not open Theme Studio')
    need(studioClosed, 'the Theme Studio did not close with its × either')
    need(studioEsc, 'Escape does not close the Theme Studio (New theme): only its × does')
    return `${cards0} themes, Fjord search → ${cards1}; menu [${items.join(', ')}]; Explore themes (Import a theme file…), Theme Studio opened and closed; Mode, Colour, Window, Motion, App icon`
  })

  await check('Settings', 'Playback: output device, exclusive, levelling, crossfade, what plays, stream quality, never offered', async () => {
    const m = await settingsPage('Playback')
    const miss = ['Where the sound goes', 'Follow Windows', 'Off', 'Per song', 'Per album', 'Crossfade', 'What plays', 'Autoplay', 'Online songs', 'Best', 'Data saver', 'Never offered'].filter((w) => !m.includes(w))
    const exclusive = /xclusive/.test(m)
    need(!miss.length, 'Playback lacks ' + miss.join(', '))
    need(exclusive, 'no exclusive mode mentioned on Playback')
    return 'every group there (exclusive seen, not switched)'
  })

  await check('Settings', 'Lyrics: sources, big screen, desktop lyrics', async () => {
    const m = await settingsPage('Lyrics')
    const sources = await run(`return document.querySelectorAll('main [role=switch]').length`)
    const miss = ['Under each line', 'Translation', 'Sources, in the order they are asked', 'Lyrics windows', 'Big screen', 'Desktop lyrics'].filter((w) => !m.includes(w))
    need(!miss.length, 'Lyrics lacks ' + miss.join(', '))
    return `${sources} switches; sources in order, under each line, translation engine, lyrics windows`
  })

  await check('Settings', 'Library: folders, detail sources and keys, backup, rules', async () => {
    const m = await settingsPage('Library')
    const miss = ['Folders', 'Where details come from', 'Sources that need a key', 'Keeping it safe', 'Organising rules'].filter((w) => !m.includes(w))
    need(!miss.length, 'Library lacks ' + miss.join(', '))
    return 'folders, detail sources, keys, backup, rules (nothing changed)'
  })

  await check('Settings', 'Online: Local only, catalog, accounts, region, listen together, relay and proxy', async () => {
    const m = await settingsPage('Online')
    const miss = ['Local only', 'Accounts', 'The catalog', 'Region', 'Listen together', 'Network'].filter((w) => !m.includes(w))
    need(!miss.length, 'Online lacks ' + miss.join(', '))
    return 'every group there (no sign-in, Local only untouched)'
  })

  await check('Settings', 'System: close, startup, shortcuts, history, cache', async () => {
    const m = await settingsPage('System')
    const miss = ['Keep playing in the tray', 'Start with Windows', 'Play or pause', 'Keep a history', 'Clear cache'].filter((w) => !m.includes(w))
    need(!miss.length, 'System lacks ' + miss.join(', '))
    need(!m.includes('Check for updates'), 'System still shows the updates')
    return 'every group there (tray, startup and shortcuts not touched)'
  })

  await check('Settings', 'About and updates: version, update, checks, pre-releases, diagnostics, credits', async () => {
    const m = await settingsPage('About and updates')
    const miss = ['Version', 'Check for updates', 'Check automatically', 'Pre-releases', 'Copy diagnostics', 'MIT licence'].filter((w) => !m.includes(w))
    need(!miss.length, 'About and updates lacks ' + miss.join(', '))
    return 'every group there (nothing pressed)'
  })

  await check('Settings', 'Jump to (Ctrl+K) finds settings by name', async () => {
    await closeLayers()
    await ctrl('k')
    await focusField('[role=dialog][aria-label="Command palette"] input')
    await typeIn('updates')
    await sleep(600)
    const opts = await run(`return [...document.querySelectorAll('[role=dialog][aria-label="Command palette"] [role=option]')].map((o) => o.innerText.replace(/\\s+/g, ' ').trim())`)
    await esc()
    const hit = opts.find((o) => o.includes('Check for updates'))
    need(hit, 'typing updates offers no setting (first: ' + opts.slice(0, 3).join(' / ') + ')')
    return hit
  })
}

// ===============================================================================================
// Everywhere
// ===============================================================================================

if (want('everywhere') && !stopped) {
  begin('everywhere')
  await check('Everywhere', 'Jump to (Ctrl+K): places, albums, commands', async () => {
    await closeLayers()
    const found = {}
    for (const [kind, q, w] of [
      ['place', 'Health', /Health/],
      ['album', 'Lakshya', /Lakshya/],
      ['command', 'keyboard shortcuts', /Show keyboard shortcuts/],
    ]) {
      await ctrl('k')
      need(await until(`document.querySelector('[role=dialog][aria-label="Command palette"]')`, 2000), 'Ctrl+K opened no palette')
      await focusField('[role=dialog][aria-label="Command palette"] input')
      await clearField()
      await typeIn(q)
      await sleep(900)
      const opts = await run(`return [...document.querySelectorAll('[role=dialog][aria-label="Command palette"] [role=option]')].map((o) => o.innerText.replace(/\\n/g, ' '))`)
      found[kind] = opts.find((o) => w.test(o))
      await esc()
      need(found[kind], `"${q}" offered no ${kind}: ${opts.slice(0, 4).join(' | ')}`)
    }
    await ctrl('k')
    await focusField('[role=dialog][aria-label="Command palette"] input')
    await typeIn('Health')
    await sleep(700)
    await t.key('Enter')
    await sleep(900)
    const h = await text()
    need(/Health/.test(h.split('\n')[0] + h.slice(0, 80)), 'Enter on "Health" did not go to Health')
    return Object.entries(found).map(([k, v]) => `${k}: ${v.slice(0, 40)}`).join('; ') + '; Enter goes there'
  })

  await check('Everywhere', 'Jump to (Ctrl+K): artists', async () => {
    await closeLayers()
    await ctrl('k')
    await focusField('[role=dialog][aria-label="Command palette"] input')
    await typeIn('Pritam')
    await sleep(2500)
    const opts = await run(`return [...document.querySelectorAll('[role=dialog][aria-label="Command palette"] [role=option]')].map((o) => o.innerText.replace(/\\s+/g, ' ').trim())`)
    await esc()
    const artist = opts.find((o) => o.includes('Pritam') && o.includes('Artist'))
    need(artist, `"Pritam" (an artist with 25 albums here) offers no artist among ${opts.length} options (first: ${opts.slice(0, 3).join(' / ')})`)
    return 'artist: ' + artist
  })

  await check('Everywhere', 'Keyboard: Ctrl+1–7, Ctrl+H, Ctrl+, , Escape closes the top layer', async () => {
    await closeLayers()
    const names = ['Home', 'Search', 'Library', 'Stats', 'Health', 'Organize', 'Devices']
    const got = []
    for (let i = 1; i <= 7; i++) {
      await ctrl(String(i))
      await sleep(700)
      got.push(await run(`const b = document.querySelector('nav [aria-current=page]'); return b ? (b.getAttribute('aria-label') || b.textContent.trim()) : ''`))
    }
    await ctrl('h')
    await sleep(800)
    const act = (await text()).includes('Activity')
    await ctrl(',')
    await sleep(800)
    const set = /Settings/.test((await text()).slice(0, 60))
    await ctrl('k')
    await until(`document.querySelector('[role=dialog][aria-label="Command palette"]')`, 2000)
    await esc()
    const palette = await until(`!document.querySelector('[role=dialog][aria-label="Command palette"]')`, 1500)
    const wrong = names.filter((nm, i) => !(got[i] ?? '').includes(nm))
    need(!wrong.length, 'Ctrl+1–7 went to ' + got.join(', '))
    need(act && set, `Ctrl+H ${act ? 'ok' : 'no Activity'}, Ctrl+, ${set ? 'ok' : 'no Settings'}`)
    need(palette, 'Escape did not close the palette')
    return `Ctrl+1–7 → ${got.join(', ')}; Ctrl+H Activity; Ctrl+, Settings; Escape closes the palette`
  })
  person('Everywhere', 'Every theme renders every screen; reduced motion', 'scripts/theme-shots.mjs (release step 3) and a person look through them')
  person('Everywhere', 'The test copy and the installed app run side by side', 'needs the installed app started: a person checks it')
}

// ===============================================================================================
// New in 0.4.0: updates, Listen together, outputs, play counts
// ===============================================================================================

if (want('updates') && !stopped) {
  begin('updates')
  await check('New 0.4.0', 'Title-bar update button (a pretended update, nothing downloaded)', async () => {
    // update_check answered in the page, at the IPC fetch (invoke itself can't be replaced):
    // found, then up to date. update_install is refused, so nothing downloads even by accident.
    // The answer is proven in place on a harmless command (is_test_copy) before any check.
    const kept = await run(`return localStorage.getItem('venuta.updates')`)
    await run(`
      if (!window.__qaFetch) window.__qaFetch = window.fetch
      window.__qaAnswer = 'found'
      const answer = (ok, body) => new Response(JSON.stringify(body), { headers: { 'Tauri-Response': ok ? 'ok' : 'error', 'Content-Type': 'application/json' } })
      window.fetch = (url, init) => {
        const cmd = String(url).split('?')[0].split('/').pop()
        if (cmd === 'is_test_copy' && window.__qaProbe) return Promise.resolve(answer(true, 'QA-ANSWERED'))
        if (cmd === 'update_check') return Promise.resolve(answer(true, window.__qaAnswer === 'found'
          ? { status: 'found', info: { version: '9.9.9', current: '0.4.0', notes: 'QA pretend update', date: null } }
          : { status: 'current', current: '0.4.0' }))
        if (cmd === 'update_install' || cmd === 'update_download_page') return Promise.resolve(answer(false, { kind: 'busy', message: 'QA: not installed' }))
        return window.__qaFetch(url, init)
      }
      return true`)
    try {
      const probe = await run(`window.__qaProbe = true; try { return await window.__TAURI_INTERNALS__.invoke('is_test_copy') } finally { window.__qaProbe = false }`)
      need(probe === 'QA-ANSWERED', 'the pretend answer is not in place (probe: ' + J(probe) + '): stopped before any check')
      await settingsPage('About and updates')
      await press('Check for updates', 'main')
      const shown = await until(`[...document.querySelectorAll('[data-tauri-drag-region] button')].some((b) => b.textContent.trim() === 'Update to 9.9.9')`, 3000)
      const inSettings = await has('Update to 9.9.9', 'main')
      const notice = await run(`return [...document.querySelectorAll('[role=status]')].some((s) => /9\\.9\\.9 is out/.test(s.innerText))`)
      await run(`window.__qaAnswer = 'current'; return true`)
      await press('Check for updates', 'main')
      const gone = await until(`![...document.querySelectorAll('[data-tauri-drag-region] button')].some((b) => /Update to|Update failed|Updating/.test(b.textContent))`, 3000)
      const words = (await text()).match(/Updates\n([^\n]+)/)?.[1]
      need(shown, 'a found update showed no Update to 9.9.9 in the title bar')
      need(inSettings, 'Settings showed no Update to 9.9.9')
      need(gone, 'the title-bar button stayed after an up-to-date answer')
      return `found → title bar "Update to 9.9.9"${notice ? ' and the notice' : ''}, Settings too; up to date → gone; not pressed`
    } finally {
      await run(`if (window.__qaFetch) { window.fetch = window.__qaFetch; delete window.__qaFetch } return true`)
      await run(`${kept === null ? `localStorage.removeItem('venuta.updates')` : `localStorage.setItem('venuta.updates', ${J(kept)})`}; return true`)
    }
  })
  person('New 0.4.0', 'Update: download, install and restart; a failed install', 'needs a real signed release')
}

if (want('together') && !stopped) {
  begin('together')
  await check('New 0.4.0', 'Listen together panel opens (no room made or joined)', async () => {
    await closeLayers()
    await press('Listen together')
    await sleep(900)
    const ok = await until(`[...document.querySelectorAll('button')].some((b) => b.textContent.trim() === 'Start a room' && __sm.vis(b))`, 3000)
    const invite = await run(`return !!document.querySelector('input[aria-label=Invite]')`)
    const join = await run(`return [...document.querySelectorAll('button')].find((b) => b.textContent.trim() === 'Join')?.disabled`)
    const name = await run(`return !!document.querySelector('input[aria-label="Your name"]')`)
    await esc()
    const closed = await until(`![...document.querySelectorAll('button')].some((b) => b.textContent.trim() === 'Start a room' && __sm.vis(b))`, 2000)
    need(ok, 'no Start a room')
    need(invite && join === true, `Invite field ${invite ? 'there' : 'missing'}, Join ${join ? 'disabled' : 'enabled with no invite'}`)
    need(name, 'no Your name')
    need(closed, 'Escape did not close the panel')
    return 'Start a room, Invite with Join disabled, Your name; Escape closes (nothing pressed)'
  })
  person('New 0.4.0', 'Listen together: rooms, the three ways, rejoin, nearby', 'over the network on two computers: docs/qa/listen-together-live.md (step 4b)')
}

if (want('output') && !stopped) {
  begin('output')
  await check('New 0.4.0', 'Output device list in Settings > Playback', async () => {
    await settingsPage('Playback')
    const devices = await run(`return (await window.__TAURI_INTERNALS__.invoke('player_devices')) ?? null`)
    const names = (devices?.devices ?? devices?.outputs ?? (Array.isArray(devices) ? devices : [])).map((d) => d.name ?? d.label ?? String(d))
    const shown = await run(`const h = [...document.querySelectorAll('main h2, main h3')].find((e) => /Where the sound goes/.test(e.textContent)); const box = h?.closest('section') ?? h?.parentElement?.parentElement; return box ? box.innerText : ''`)
    const follow = await run(`return document.querySelector('main [role=switch][aria-label="Follow Windows"]')?.getAttribute('aria-checked')`)
    const missing = names.filter((nm) => !shown.includes(nm.split(' (')[0]))
    need(follow !== null && follow !== undefined, 'no Follow Windows')
    need(names.length > 0, 'the app knows no output devices: ' + JSON.stringify(devices).slice(0, 200))
    need(!missing.length, 'devices not listed: ' + missing.join(', '))
    return `${names.length} device(s) listed: ${names.join(', ')}; Follow Windows ${follow} (nothing switched)`
  })

  await check('New 0.4.0', '"Pause when headphones or Bluetooth disconnect" in Settings > Playback', async () => {
    await settingsPage('Playback')
    const sel = 'main [role=switch][aria-label="Pause when headphones or Bluetooth disconnect"]'
    const s0 = await run(`return document.querySelector(${J(sel)})?.getAttribute('aria-checked')`)
    need(s0 === 'true' || s0 === 'false', 'no Pause when headphones or Bluetooth disconnect switch')
    const k0 = await run(`return await window.__TAURI_INTERNALS__.invoke('player_pause_when_headphones_go')`)
    await pressAt(`document.querySelector(${J(sel)})`)
    await sleep(600)
    const s1 = await run(`return document.querySelector(${J(sel)})?.getAttribute('aria-checked')`)
    const k1 = await run(`return await window.__TAURI_INTERNALS__.invoke('player_pause_when_headphones_go')`)
    await pressAt(`document.querySelector(${J(sel)})`)
    await sleep(600)
    const s2 = await run(`return document.querySelector(${J(sel)})?.getAttribute('aria-checked')`)
    const k2 = await run(`return await window.__TAURI_INTERNALS__.invoke('player_pause_when_headphones_go')`)
    need(s1 !== s0 && k1 === (s1 === 'true'), `switching: shown ${s1}, kept ${k1}`)
    need(s2 === s0 && k2 === k0, `back: shown ${s2}, kept ${k2}`)
    return `${s0 === 'true' ? 'on' : 'off'} by default here; switching it is kept by the player (${k0} → ${k1} → ${k2})`
  })
  person('New 0.4.0', 'Outputs coming and going (headphones, Bluetooth, USB DAC), pause on disconnect, per-output delay', 'unplugging hardware: a person checks it')
}

if (want('counts') && !stopped) {
  begin('counts')
  await check('New 0.4.0', 'Play counts: a song heard through counts in History and Stats', async () => {
    // The shortest song (13 s), heard through at volume 0.
    await rail('Stats')
    await tab('History')
    await sleep(1200)
    const before = await text()
    const firstBefore = before.split('\n').slice(0, 14).join(' | ')
    await rail('Stats')
    await tab('Stats')
    await press('All time', 'main').catch(() => {})
    await sleep(900)
    const statsBefore = await text()
    const countOf = (s) => {
      const m = s.match(/Jimmy Ka Tashan[^\n]*\n?[^\n]*?(\d+) plays?/)
      return m ? +m[1] : 0
    }
    const c0 = countOf(statsBefore)
    need(await playSearch('Jimmy Ka Tashan', 'Jimmy Ka Tashan'), 'Jimmy Ka Tashan did not start')
    await mute()
    const dur = await run(`return +document.querySelector('[aria-label="Position in the song"]').getAttribute('aria-valuemax')`)
    need(dur > 0 && dur < 60, `the song is ${dur}s long: not waiting that long`)
    // Heard to its end; then pause whatever plays next.
    need(await until(`document.querySelector('[aria-label="Pause (Space)"]')`, 5000), 'Jimmy Ka Tashan is not playing')
    const t0 = Date.now()
    let heard = 0
    while (Date.now() - t0 < (dur + 8) * 1000) {
      await mute()
      const still = await run(`return document.querySelector('[aria-label="Now playing"]')?.innerText.includes('Jimmy Ka Tashan')`)
      if (!still) break
      heard = Math.max(heard, await position())
      await sleep(500)
    }
    await pause()
    await sleep(1500)
    await rail('Stats')
    await tab('History')
    await sleep(1200)
    const after = await text()
    const today = after.split(/\n(?=Today|Yesterday|Monday|Tuesday|Wednesday|Thursday|Friday|Saturday|Sunday)/)[1] ?? after
    const inHistory = /Jimmy Ka Tashan/.test(today.split('\n').slice(0, 8).join('\n'))
    await tab('Stats')
    await sleep(900)
    const c1 = countOf(await text())
    need(inHistory, `heard to ${heard}s of ${dur}s, then the next song; the play is not at the top of History: ` + today.split('\n').slice(0, 6).join(' | '))
    return `heard to ${heard}s of ${dur}s; History: Jimmy Ka Tashan at the top of Today; Top songs count ${c0} → ${c1}${c1 > c0 ? '' : ' (outside the top list)'}`
  })
}

// ===============================================================================================
// Put back what the run changed, and the console
// ===============================================================================================

begin('cleanup')
await pause().catch(() => {})
await closeLayers().catch(() => {})
for (const name of [...made]) {
  try {
    if (plFile(name)) await setAside(name)
    line(`- set aside "${name}": ${plFile(name) ? 'STILL THERE' : 'done'}`)
  } catch (e) {
    line(`- set aside "${name}" failed: ${e.message}`)
  }
}
await closeLayers().catch(() => {})
await rail('Home').catch(() => {})
await mute()
await pause().catch(() => {})
await run(`clearInterval(window.__qaGuard); window.__qaGuard = 0; return true`)

const errors = t.console.filter((c) => /^(error|exception)/.test(c))
line(`\n## Console\n`)
for (const c of t.console) line(`- ${c}`)
if (!t.console.length) line('- nothing')

const count = (r) => results.filter((x) => x.result === r).length
line(`\n**${count('pass')} pass, ${count('fail')} fail, ${count('person')} need a person; ${errors.length} console errors, ${t.console.length - errors.length} warnings.**`)
writeFileSync(join(DIR, 'smoke.json'), JSON.stringify({ results, console: t.console }, null, 2))
await t.close()
process.exit(count('fail') || stopped ? 1 : 0)
