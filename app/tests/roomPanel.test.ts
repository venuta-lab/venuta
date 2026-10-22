// The room panel v2's logic (components/together/model.ts), case by case over the window's
// fixture of every Listen together state (made by crates/together/src/view.rs): which screen,
// what the status strip says, the chip's colour, who may do what, where a press goes, the join
// choice, the output notes, the people's rows, the remote's progress, and the projection to
// today's view that the rest of the window still reads.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import type { TogetherViewV2 } from '../src/lib/togetherView.ts'
import {
  chipTone,
  copyWords,
  delayWords,
  doorWords,
  handOverChoices,
  inviteWords,
  isV2,
  joinChoice,
  joiningWords,
  may,
  modeSwitches,
  modeWord,
  names,
  newRoleValue,
  outputPanel,
  progress,
  requestsShown,
  routePress,
  rows,
  screenOf,
  startWords,
  statusLines,
  stepButton,
  toV1,
  transitionOptions,
  transitionWords,
  versionNote,
} from '../src/components/together/model.ts'

const { cases } = JSON.parse(readFileSync(new URL('./fixtures/together-views.json', import.meta.url), 'utf8')) as {
  cases: { name: string; view: TogetherViewV2 }[]
}
const view = (name: string): TogetherViewV2 => {
  const c = cases.find((c) => c.name === name)
  assert.ok(c, `no fixture case "${name}"`)
  return structuredClone(c.view)
}

// Every case, what the panel opens on, the status strip and the chip.
const EXPECTED: Record<string, { screen: string; status: string[]; chip: string }> = {
  off: { screen: 'start', status: [], chip: 'accent' },
  'off, rooms nearby and one to rejoin': { screen: 'start', status: [], chip: 'accent' },
  'off, your room to reopen': { screen: 'start', status: [], chip: 'accent' },
  starting: { screen: 'start', status: [], chip: 'accent' },
  'hosting, everyone in every state': { screen: 'room', status: [], chip: 'accent' },
  'hosting alone, nothing playing': { screen: 'room', status: ['Add songs to start'], chip: 'accent' },
  'hosting, a one speaker room': { screen: 'room', status: [], chip: 'accent' },
  'hosting, the queue ended': { screen: 'room', status: ["The room's queue has ended"], chip: 'accent' },
  'joining, buffering': { screen: 'joining', status: [], chip: 'accent' },
  "joining on the host's network, choosing": { screen: 'joining', status: [], chip: 'accent' },
  "joining on the host's network, on headphones": { screen: 'joining', status: [], chip: 'accent' },
  'waiting at the door': { screen: 'door', status: [], chip: 'accent' },
  'in, pausing': { screen: 'room', status: ['Pausing...'], chip: 'accent' },
  'in, moving': { screen: 'room', status: ['Moving to 1:35...'], chip: 'accent' },
  'in, skipping': { screen: 'room', status: ['Skipping...'], chip: 'accent' },
  'in, starting': { screen: 'room', status: ['Starting...'], chip: 'accent' },
  'in, going back': { screen: 'room', status: ['Going back...'], chip: 'accent' },
  'in, crossfading between songs': { screen: 'room', status: [], chip: 'accent' },
  'in, cut between songs': { screen: 'room', status: [], chip: 'accent' },
  "in, a remote on the host's speaker": { screen: 'room', status: [], chip: 'accent' },
  'in, a remote who controls and may change the volume': { screen: 'room', status: [], chip: 'accent' },
  "in, every device, on the host's network": { screen: 'room', status: [], chip: 'accent' },
  'in, every device not available': { screen: 'room', status: [], chip: 'accent' },
  'in, the room paused': { screen: 'room', status: ['Paused'], chip: 'accent' },
  'in, a start waiting for someone': { screen: 'room', status: ['Starting... waiting for Ali'], chip: 'accent' },
  'in, a skip waiting for two': { screen: 'room', status: ['Skipping... waiting for Ali and Dev'], chip: 'accent' },
  'in, the host reconnecting': { screen: 'room', status: ['Asha is reconnecting. The music goes on.'], chip: 'faded' },
  'in, on Bluetooth with its delay set': { screen: 'room', status: [], chip: 'accent' },
  'in, on Bluetooth with its delay not known': { screen: 'room', status: [], chip: 'accent' },
  'in, stepped out': { screen: 'room', status: [], chip: 'faded' },
  'in, stepped out by the sleep timer': { screen: 'room', status: [], chip: 'faded' },
  'in, headphones disconnected': { screen: 'room', status: [], chip: 'faded' },
  'in, buffering': { screen: 'room', status: [], chip: 'warning' },
  'in, sound output unavailable': { screen: 'room', status: [], chip: 'warning' },
  'in, interrupted': { screen: 'room', status: [], chip: 'warning' },
  'in, not available for you': { screen: 'room', status: [], chip: 'faded' },
  'in, catching up': { screen: 'room', status: [], chip: 'warning' },
  'in, on another version': { screen: 'room', status: [], chip: 'accent' },
  'in, just listening': { screen: 'room', status: [], chip: 'accent' },
  'in, in control with a request to answer': { screen: 'room', status: [], chip: 'accent' },
  "in, the room's queue ended": { screen: 'room', status: ["The room's queue has ended"], chip: 'accent' },
  reconnecting: { screen: 'room', status: ['Reconnecting... the music goes on'], chip: 'faded' },
  ended: { screen: 'start', status: [], chip: 'accent' },
  'ended, not let in': { screen: 'start', status: [], chip: 'accent' },
}

test('every fixture case has its screen, status strip and chip here', () => {
  assert.deepEqual(cases.map((c) => c.name).sort(), Object.keys(EXPECTED).sort())
  for (const c of cases) {
    const want = EXPECTED[c.name]
    assert.equal(screenOf(c.view), want.screen, `${c.name}: screen`)
    assert.deepEqual(
      statusLines(c.view).map((s) => s.text),
      want.status,
      `${c.name}: status`,
    )
    assert.equal(chipTone(c.view), want.chip, `${c.name}: chip`)
  }
})

test('a v2 view is told from today', () => {
  for (const c of cases) assert.ok(isV2(c.view), c.name)
  assert.ok(!isV2({ state: 'off', delay_ms: 0 }))
  assert.ok(!isV2(null))
})

test('names are listed in plain words', () => {
  assert.equal(names([]), '')
  assert.equal(names(['Ali']), 'Ali')
  assert.equal(names(['Ali', 'Dev']), 'Ali and Dev')
  assert.equal(names(['Ali', 'Dev', 'Ira']), 'Ali, Dev and Ira')
  assert.equal(names(['Ali', 'Dev', 'Ira', 'Om']), 'Ali, Dev and 2 more')
})

test('a press under way while the start waits for nobody says only the press', () => {
  const v = view('in, skipping')
  v.room!.starting = []
  assert.deepEqual(
    statusLines(v).map((s) => s.text),
    ['Skipping...'],
  )
  // The phase line gives way to a press: a resume from a pause says Starting, not Paused.
  const p = view('in, the room paused')
  p.you!.pressing = 'Starting...'
  p.room!.starting = ['Ali']
  assert.deepEqual(
    statusLines(p).map((s) => s.text),
    ['Starting... waiting for Ali'],
  )
})

test('a guest who may not add sees the resting room differently', () => {
  const v = view('hosting alone, nothing playing')
  v.host = false
  v.me = 2
  v.room!.members.push({ ...v.room!.members[0], id: 2, name: 'Meera', host: false, role: 'listen' })
  assert.deepEqual(
    statusLines(v).map((s) => s.text),
    ['Nothing is playing yet'],
  )
})

test('who may do what, by role, for the host and while the host is away', () => {
  const host = may(view('hosting, everyone in every state'))
  assert.deepEqual(host, { control: true, add: true, answer: true, volume: true, settings: true, modes: true })
  // Meera with Add (the fixture's default guest role).
  assert.deepEqual(may(view('in, skipping')), { control: false, add: true, answer: false, volume: false, settings: false, modes: true })
  assert.deepEqual(may(view('in, just listening')), { control: false, add: false, answer: false, volume: false, settings: false, modes: true })
  assert.deepEqual(may(view('in, in control with a request to answer')), {
    control: true,
    add: true,
    answer: true,
    volume: false,
    settings: false,
    modes: true,
  })
  // A remote: volume only when the host lets guests change it.
  assert.equal(may(view("in, a remote on the host's speaker")).volume, false)
  assert.equal(may(view('in, a remote who controls and may change the volume')).volume, true)
  // A guest who plays never has the host speaker's volume, even when guests may.
  const playing = view("in, every device, on the host's network")
  playing.room!.guests_volume = true
  assert.equal(may(playing).volume, false)
  // The host away: every room control greyed.
  const away = view('in, the host reconnecting')
  away.room!.members.find((m) => m.id === 2)!.role = 'control'
  assert.deepEqual(may(away), { control: false, add: false, answer: false, volume: false, settings: false, modes: false })
  // Out of a room: nothing.
  assert.deepEqual(may(view('off')), { control: false, add: false, answer: false, volume: false, settings: false, modes: false })
})

test('a press goes to the room, becomes a request, or is explained', () => {
  const host = view('hosting, everyone in every state')
  for (const p of ['play', 'pause', 'next', 'previous', 'seek', 'jump', 'remove', 'move', 'add'] as const) assert.deepEqual(routePress(host, p), { send: 'ask' }, p)
  const adder = view('in, skipping')
  assert.deepEqual(routePress(adder, 'add'), { send: 'ask' })
  assert.deepEqual(routePress(adder, 'next'), { send: 'request', said: 'Asked the room to skip' })
  assert.deepEqual(routePress(adder, 'pause'), { send: 'request', said: 'Asked the room to pause' })
  assert.deepEqual(routePress(adder, 'play'), { send: 'request', said: 'Asked the room to play' })
  assert.deepEqual(routePress(adder, 'previous'), { send: 'request', said: 'Asked the room to go back' })
  assert.deepEqual(routePress(adder, 'seek', 95_000), { send: 'request', said: 'Asked the room to move to 1:35' })
  assert.deepEqual(routePress(adder, 'jump'), { send: 'request', said: 'Asked the room to play that song' })
  assert.deepEqual(routePress(adder, 'remove'), { send: 'refuse', said: 'Asha is in control of the queue.' })
  assert.deepEqual(routePress(adder, 'move'), { send: 'refuse', said: 'Asha is in control of the queue.' })
  const listener = view('in, just listening')
  assert.deepEqual(routePress(listener, 'add'), { send: 'refuse', said: "Asha hasn't let you add songs." })
  assert.deepEqual(routePress(listener, 'next'), { send: 'request', said: 'Asked the room to skip' })
  assert.deepEqual(routePress(view('in, in control with a request to answer'), 'next'), { send: 'ask' })
  // While you reconnect a press still goes (the session holds it, B12).
  const back = view('reconnecting')
  back.room!.members.find((m) => m.id === 2)!.role = 'control'
  assert.deepEqual(routePress(back, 'pause'), { send: 'ask' })
})

test('requests show only to whoever controls', () => {
  assert.deepEqual(
    requestsShown(view('hosting, everyone in every state')).map((r) => r.words),
    ['Ravi asks to skip'],
  )
  assert.deepEqual(
    requestsShown(view('in, in control with a request to answer')).map((r) => r.words),
    ['Ali asks to skip'],
  )
  const adder = view('in, in control with a request to answer')
  adder.room!.members.find((m) => m.id === 2)!.role = 'add'
  assert.deepEqual(requestsShown(adder), [])
})

test('the join choice: one speaker first, this device beside it', () => {
  assert.equal(joinChoice(view('joining, buffering')), null)
  const c = joinChoice(view("joining on the host's network, choosing"))
  assert.deepEqual(c, {
    speaker: "Play on Asha's speaker",
    speakerSays: "Only Asha's speaker plays. You see the song and the queue.",
    here: 'Play on this device too',
    hereSays: "Your speaker plays in step with Asha's.",
    hereMode: 'every_device',
    hereWhyNot: null,
    selected: 'speaker',
  })
  const h = joinChoice(view("joining on the host's network, on headphones"))
  assert.equal(h?.hereMode, 'apart')
  assert.equal(h?.hereSays, 'You hear it on this device, with the room.')
  // Once chosen, the choice shows it.
  const chose = view("joining on the host's network, choosing")
  chose.you!.mode = 'every_device'
  assert.equal(joinChoice(chose)?.selected, 'here')
  // In the room the choice is not the join screen's.
  assert.equal(joinChoice(view("in, a remote on the host's speaker")), null)
})

test('in the room, the mode is one button, greyed with why when it cannot be had', () => {
  assert.deepEqual(modeSwitches(view("in, a remote on the host's speaker")), [{ label: 'Play here too', mode: 'every_device', why_not: null }])
  assert.deepEqual(modeSwitches(view("in, every device, on the host's network")), [{ label: "Listen on Asha's speaker", mode: 'one_speaker', why_not: null }])
  assert.deepEqual(modeSwitches(view('in, every device not available')), [
    { label: "Play in step with the room's speakers", mode: 'every_device', why_not: 'Not while Hide my IP is on' },
  ])
  assert.deepEqual(modeSwitches(view('in, skipping')), [])
  // The host away: no mode changes (the host takes them).
  const away = view("in, every device, on the host's network")
  away.room!.host_away = true
  assert.deepEqual(
    modeSwitches(away).map((s) => s.why_not),
    ['Asha is reconnecting'],
  )
})

test('modes and transitions in a word', () => {
  assert.equal(modeWord('one_speaker', 'Asha'), "Asha's speaker")
  assert.equal(modeWord('every_device', 'Asha'), 'Every device')
  assert.equal(modeWord('apart', 'Asha'), 'Apart')
  assert.equal(transitionWords({ kind: 'gapless' }), 'Gapless')
  assert.equal(transitionWords({ kind: 'crossfade', ms: 4000 }), 'Crossfade 4 s')
  assert.equal(transitionWords({ kind: 'crossfade', ms: 2500 }), 'Crossfade 2.5 s')
  assert.equal(transitionWords({ kind: 'cut' }), 'Cut')
  assert.deepEqual(
    transitionOptions({ kind: 'gapless' }).map((o) => o.label),
    ['Gapless', 'Crossfade 4 s', 'Cut'],
  )
  assert.deepEqual(
    transitionOptions({ kind: 'crossfade', ms: 8000 }).map((o) => o.label),
    ['Gapless', 'Crossfade 4 s', 'Crossfade 8 s', 'Cut'],
  )
  assert.equal(newRoleValue(view('hosting, everyone in every state').room!), 'auto')
  assert.equal(newRoleValue(view('hosting, a one speaker room').room!), 'add')
})

test('your output: the delay, its notes, the Bluetooth suggestion and Line up by ear', () => {
  assert.equal(delayWords(0), 'none')
  assert.equal(delayWords(200), '+200 ms')
  assert.equal(delayWords(-40), '-40 ms')
  const plain = outputPanel(view('in, skipping'))
  assert.deepEqual(plain, { show: true, name: 'Speakers', bluetooth: false, delay: 'none', note: null, suggest: null, lineUp: false })
  assert.deepEqual(outputPanel(view('in, on Bluetooth with its delay set')).note, { text: 'Bluetooth: about 200 ms behind', warn: false })
  const unknown = outputPanel(view('in, on Bluetooth with its delay not known'))
  assert.deepEqual(unknown.note, { text: 'Bluetooth delay not measured: you may hear an echo', warn: true })
  assert.deepEqual(unknown.suggest, { ms: 170, says: 'Bluetooth is usually about 170 ms late' })
  assert.equal(unknown.lineUp, true)
  const fx = view("in, every device, on the host's network")
  fx.output.delay_known = false
  assert.deepEqual(outputPanel(fx).note, { text: "This output's delay isn't measured: you may hear an echo", warn: true })
  assert.equal(outputPanel(fx).suggest, null)
  // A remote's device plays nothing; the host lines up nobody.
  assert.equal(outputPanel(view("in, a remote on the host's speaker")).show, false)
  assert.equal(outputPanel(view('hosting, everyone in every state')).lineUp, false)
  assert.equal(outputPanel(view('off')).show, false)
})

test('the people: mode, line, copy for whom it is shown, numbers, path and the host notes', () => {
  const host = rows(view('hosting, everyone in every state'))
  assert.equal(host.length, 14)
  const asha = host[0]
  assert.equal(asha.you, true)
  assert.equal(asha.mode, 'Every device')
  assert.equal(asha.line, 'In step, ±2 ms')
  assert.equal(asha.manage, false)
  const ravi = host.find((r) => r.m.name === 'Ravi')!
  assert.equal(ravi.copy, 'YouTube Music, another version (+12 s)')
  assert.equal(ravi.numbers, 'Off by 4 ms, the clock within 9 ms')
  assert.equal(ravi.manage, true)
  assert.equal(ravi.path, null)
  assert.equal(host.find((r) => r.m.name === 'Dev')!.slow, "Dev's connection is slow")
  assert.equal(host.find((r) => r.m.name === 'Veer')!.mode, "Asha's speaker")
  assert.equal(host.find((r) => r.m.name === 'Veer')!.numbers, null)
  assert.equal(host.find((r) => r.m.name === 'Ishan')!.line, 'Reconnecting')
  const asEveryDevice = host.find((r) => r.m.name === 'Kabir')!
  assert.equal(asEveryDevice.numbers, 'Off by under 1 ms, the clock within 1 ms')
  // A guest sees only their own copy, their path, and nobody's slow note.
  const guest = rows(view('in, skipping'))
  assert.deepEqual(
    guest.map((r) => [r.m.name, r.you, r.copy, r.manage]),
    [
      ['Asha', false, null, false],
      ['Meera', true, 'own file, matched', false],
      ['Ali', false, null, false],
    ],
  )
  assert.equal(guest[1].path, 'Through a relay, 118 ms round trip')
  assert.equal(rows(view("in, every device, on the host's network"))[1].path, 'Direct, 3 ms round trip')
})

test('copies in words, as Rust says them', () => {
  assert.equal(copyWords({ kind: 'own_file', class: 'measured', length_diff_s: null }), 'own file, matched')
  assert.equal(copyWords({ kind: 'host_stream', class: 'exact', length_diff_s: null }), 'your file, streamed, matched')
  // To a guest, the host's file is the host's.
  assert.equal(copyWords({ kind: 'host_stream', class: 'exact', length_diff_s: null }, 'Asha'), "Asha's file, streamed, matched")
  assert.equal(copyWords({ kind: 'catalog', class: 'approximate', length_diff_s: 12 }), 'YouTube Music, another version (+12 s)')
  assert.equal(copyWords({ kind: 'catalog', class: 'approximate', length_diff_s: -3 }), 'YouTube Music, another version (-3 s)')
  assert.equal(copyWords({ kind: 'close_match', class: 'approximate', length_diff_s: null }), 'a close match, another version')
})

test('on another version over 2 s off, one tap to the host copy', () => {
  assert.equal(versionNote(view('in, on another version')), "Different version (+12 s): use Asha's copy")
  assert.equal(versionNote(view('in, skipping')), null)
  const close = view('in, on another version')
  close.room!.members.find((m) => m.id === 2)!.copy!.length_diff_s = 2
  assert.equal(versionNote(close), null)
  // The host never sees it for itself.
  assert.equal(versionNote(view('hosting, everyone in every state')), null)
})

test('step out, catch up, rejoin', () => {
  assert.deepEqual(stepButton(view('in, skipping')), { label: 'Step out', out: false })
  assert.deepEqual(stepButton(view('in, stepped out')), { label: 'Catch up', out: true })
  assert.deepEqual(stepButton(view('in, stepped out by the sleep timer')), { label: 'Catch up', out: true })
  assert.deepEqual(stepButton(view('in, headphones disconnected')), { label: 'Rejoin', out: true })
  assert.equal(stepButton(view("in, a remote on the host's speaker")), null)
  assert.equal(stepButton(view('joining, buffering')), null)
})

test('a remote follows the anchor by the host clock', () => {
  const v = view("in, a remote on the host's speaker")
  // The anchor: content 95 s at host 3600 s; the view made at host 3612.5 s.
  assert.deepEqual(progress(v, 0), { entry: v.room!.queue[0], ms: 107_500, length_ms: 201_000, playing: true })
  assert.equal(progress(v, 1_500)?.ms, 109_000)
  // Never past the song's end.
  assert.equal(progress(v, 10_000_000)?.ms, 201_000)
  // A start set ahead waits at its place; a held room stays where it is.
  v.room!.anchor!.host_us = 3_700_000_000
  assert.equal(progress(v, 0)?.ms, 95_000)
  const held = view('in, the room paused')
  assert.deepEqual(progress(held, 5_000)?.ms, 95_000)
  assert.equal(progress(held, 5_000)?.playing, false)
  // No anchor, or no clock yet: nothing to show.
  assert.equal(progress(view("in, the room's queue ended"), 0), null)
  const noClock = view("in, a remote on the host's speaker")
  noClock.host_now_us = null
  assert.equal(progress(noClock, 0), null)
})

test('hand over: who can take the room, and what is lost', () => {
  assert.deepEqual(handOverChoices(view('hosting, everyone in every state')), [
    { id: 1, name: 'Ravi', warn: null },
    { id: 2, name: 'Meera', warn: '2 people will lose songs only you have' },
  ])
  const one = view('hosting, everyone in every state')
  one.room!.members[2].hand_over = 1
  assert.equal(handOverChoices(one)[1].warn, '1 person will lose songs only you have')
  assert.deepEqual(handOverChoices(view('in, skipping')), [])
})

test('the start screen: opening, after an ending, offers to go back, rooms nearby', () => {
  assert.deepEqual(startWords(view('off')), { opening: false, message: null, saveQueue: null, resume: null, nearby: [] })
  assert.equal(startWords(view('starting')).opening, true)
  assert.deepEqual(startWords(view('ended')), {
    opening: false,
    message: 'Asha ended the room',
    saveQueue: "Save the room's queue as a playlist",
    resume: null,
    nearby: [],
  })
  assert.equal(startWords(view('ended, not let in')).saveQueue, null)
  assert.deepEqual(startWords(view('off, rooms nearby and one to rejoin')).resume, { text: "Rejoin Asha's room?", button: 'Rejoin' })
  assert.deepEqual(startWords(view('off, rooms nearby and one to rejoin')).nearby, [
    { id: 1, text: "Asha's room", sub: '3 listening' },
    { id: 2, text: "Kabir's room", sub: '1 listening' },
  ])
  assert.deepEqual(startWords(view('off, your room to reopen')).resume, { text: 'Reopen your room? Friends still on the invite can come back.', button: 'Reopen' })
  assert.equal(inviteWords(null), "That isn't a Venuta invite.")
  assert.equal(inviteWords({ host: 'Asha', open: false, until: 0 }), "Asha's invite has ended. Ask for a new one.")
  assert.equal(inviteWords({ host: 'Asha', open: true, until: 0 }), "Asha's room")
  assert.equal(inviteWords({ host: 'Asha', open: true, until: 0, listening: 3, now_playing: 'Song 1' }), "Asha's room, 3 listening, now playing Song 1")
})

test('the door and joining', () => {
  assert.equal(doorWords(view('waiting at the door')), 'Waiting for the host to let you in...')
  assert.deepEqual(joiningWords(view('joining, buffering')), { title: 'Joining the room', line: 'Joining... buffering 60%', pct: 60 })
  assert.deepEqual(joiningWords(view("joining on the host's network, choosing")), { title: "Joining Asha's room", line: 'Getting ready', pct: null })
})

test("today's view, projected for the rest of the window", () => {
  const v = view('hosting, everyone in every state')
  const old = toV1(v)
  assert.equal(old.state, 'hosting')
  assert.equal(old.host, true)
  assert.equal(old.delay_ms, 0)
  assert.equal(old.room!.host_name, 'Asha')
  assert.deepEqual(
    old.room!.queue.map((s) => s.title),
    ['Song 1', 'Song 2', 'Song 3', 'Song 4'],
  )
  assert.deepEqual(old.room!.timeline, { index: 0, rev: 12, position_ms: 95_000, at_us: 3_600_000_000, playing: true })
  assert.equal(old.room!.same_room, true)
  const hearing = Object.fromEntries(old.room!.members.map((m) => [m.name, m.hearing]))
  assert.equal(hearing.Asha, 'from_host')
  assert.equal(hearing.Ravi, 'online')
  assert.equal(hearing.Meera, 'opening')
  assert.equal(hearing.Ali, 'online')
  assert.equal(hearing.Dev, 'local')
  assert.equal(hearing.Om, 'stepped_out')
  assert.equal(hearing.Tara, 'cannot')
  assert.equal(hearing.Veer, 'from_host')
  assert.equal(old.room!.members.find((m) => m.name === 'Ishan')!.away, true)
  assert.equal(old.room!.members.find((m) => m.name === 'Ravi')!.error_ms, -4)
  assert.equal(toV1(view('in, the room paused')).room!.timeline!.playing, false)
  assert.equal(toV1(view("in, the room's queue ended")).room!.timeline, null)
  assert.equal(toV1(view('off')).room, null)
  assert.equal(toV1(view('in, on Bluetooth with its delay set')).delay_ms, 200)
})

test('no em dashes, and sentence case, in what the panel says', () => {
  const said: string[] = []
  for (const c of cases) {
    const v = c.view
    said.push(...statusLines(v).map((s) => s.text), doorWords(v), joiningWords(v).title, joiningWords(v).line ?? '')
    const j = joinChoice(v)
    if (j) said.push(j.speaker, j.here, j.speakerSays, j.hereSays)
    said.push(...modeSwitches(v).map((s) => s.label), ...rows(v).flatMap((r) => [r.mode, r.line, r.copy ?? '', r.numbers ?? '', r.slow ?? '']))
    const o = outputPanel(v)
    said.push(o.note?.text ?? '', o.suggest?.says ?? '', versionNote(v) ?? '')
    const s = startWords(v)
    said.push(s.message ?? '', s.saveQueue ?? '', s.resume?.text ?? '')
  }
  for (const s of said.filter(Boolean)) {
    assert.ok(!s.includes('—'), s)
    assert.ok(/^[A-Z0-9a-z+\-±]/.test(s), s)
    // Sentence case: no word after the first capitalised unless it is a name or a product.
    const words = s.split(/\s+/)
    for (const [i, w] of words.entries())
      if (i > 0 && !/[.?!:]$/.test(words[i - 1]) && /^[A-Z][a-z]/.test(w))
        assert.ok(/^(Asha|Ravi|Meera|Ali|Kabir|Sara|Dev|Ira|Noor|Zoya|Om|Tara|Veer|Ishan|YouTube|Music|Bluetooth|Venuta|Hide|IP|Song)/.test(w), `${w} in "${s}"`)
  }
})
