// The window's fixture of every Listen together state (made by crates/together/src/view.rs):
// every state, mode, copy and press is in it, in plain words, so the room panel can be built
// and checked against it before the rooms behind it exist.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import type { MemberView, SubState, TogetherViewV2 } from '../src/lib/togetherView.ts'

const { cases } = JSON.parse(readFileSync(new URL('./fixtures/together-views.json', import.meta.url), 'utf8')) as {
  cases: { name: string; view: TogetherViewV2 }[]
}

const members: MemberView[] = cases.flatMap((c) => c.view.room?.members ?? [])
const subName = (s: SubState | null) => (s === null ? 'away' : s.state === 'suspended' ? `suspended:${s.why}` : s.state)

test('every session state is drawn', () => {
  const states = new Set(cases.map((c) => c.view.state))
  for (const s of ['off', 'starting', 'joining', 'hosting', 'waiting', 'in', 'reconnecting', 'ended']) assert.ok(states.has(s as never), s)
})

test('every player state, mode and copy appears beside someone', () => {
  const subs = new Set(members.map((m) => subName(m.sub_state)))
  for (const s of ['opening', 'ready', 'in_step', 'correcting', 'realigning', 'stepped_out', 'cannot_play', 'remote', 'suspended:buffering', 'suspended:output_lost', 'suspended:interrupted', 'suspended:sleeping', 'away'])
    assert.ok(subs.has(s), s)
  const modes = new Set(members.map((m) => m.mode))
  for (const m of ['one_speaker', 'every_device', 'apart']) assert.ok(modes.has(m as never), m)
  const copies = new Set(members.map((m) => `${m.copy?.kind}/${m.copy?.class}`))
  for (const k of ['host_stream/exact', 'own_file/measured', 'catalog/approximate', 'close_match/approximate']) assert.ok(copies.has(k), k)
})

test('each state reads as its plain line', () => {
  const lineOf = (s: string) => members.find((m) => subName(m.sub_state) === s)?.line
  assert.equal(lineOf('opening'), 'Getting ready')
  assert.equal(lineOf('suspended:buffering'), 'Buffering')
  assert.equal(lineOf('suspended:output_lost'), 'Sound output unavailable')
  assert.equal(lineOf('cannot_play'), 'Not available for you, back with the next song')
  assert.equal(lineOf('remote'), "Listening on Asha's speaker")
  assert.equal(lineOf('away'), 'Reconnecting')
  for (const m of members.filter((m) => m.sub_state?.state === 'in_step'))
    assert.match(m.line, m.mode === 'every_device' ? /^In step, ±\d+ ms$/ : /^In step, about ±\d+ ms$/)
})

test('presses under way, transitions and outputs are all there', () => {
  const pressing = new Set(cases.map((c) => c.view.you?.pressing).filter(Boolean))
  for (const p of ['Pausing...', 'Moving to 1:35...', 'Skipping...', 'Starting...', 'Going back...']) assert.ok(pressing.has(p), p)
  const transitions = new Set(cases.map((c) => c.view.room?.transition.kind).filter(Boolean))
  for (const t of ['gapless', 'crossfade', 'cut']) assert.ok(transitions.has(t as never), t)
  assert.ok(cases.some((c) => c.view.output.bluetooth && !c.view.output.delay_known), 'a Bluetooth output whose delay is not known')
  assert.ok(cases.some((c) => c.view.room?.host_away), 'the host reconnecting')
  assert.ok(cases.some((c) => c.view.you?.joining_pct != null), 'joining, with how much is in')
})

test('no em dashes in anything a person reads', () => {
  assert.ok(!JSON.stringify(cases).includes('\u2014'))
})
