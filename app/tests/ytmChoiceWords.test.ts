// node --test tests/   (Node runs the TypeScript as is)
// Your YouTube Music choices: safe for a new account, grouped as the sign-in screen shows them,
// and said in words that match what Venuta really does.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { SAFE, GROUPS, settle, summary, accountLine, rowOn, playChoicesOf } from '../src/lib/ytmChoiceWords.ts'

test('a new account starts safe: what reads is on, what writes is off', () => {
  assert.deepEqual(SAFE, {
    personal: true,
    library: true,
    count_plays: false,
    listening_time: false,
    likes: false,
    signed_search: false,
    new_likes_in: false,
    links_in_step: true,
  })
})

test('how much you listened goes only with count plays', () => {
  assert.equal(settle({ ...SAFE, listening_time: true }).listening_time, false)
  assert.equal(settle({ ...SAFE, count_plays: true, listening_time: true }).listening_time, true)
})

test('the groups are the four the plan names, in order', () => {
  assert.deepEqual(
    GROUPS.map((g) => g.title),
    ['What Venuta reads from your account', 'What Venuta tells YouTube Music', 'What Venuta keeps in step', 'How it plays'],
  )
  const names = GROUPS.flatMap((g) => g.rows.map((r) => r.name))
  assert.deepEqual(names, [
    'Recommendations for me',
    'My library in Venuta',
    'Count plays',
    'How much I listened',
    'Likes and dislikes',
    'Signed-in search',
    'Bring my Liked Music in',
    'New YouTube likes come in',
    'Linked playlists stay in step',
    'Autoplay',
    'Play my copy when I have it',
    'Songs, not music videos',
    'Hide artists I blocked',
  ])
})

test('every account switch is shown once, and bringing likes in is an action', () => {
  const keys = GROUPS.flatMap((g) => g.rows.flatMap((r) => (r.kind === 'account' ? [r.key] : [])))
  assert.deepEqual([...keys].sort(), Object.keys(SAFE).sort())
  const bring = GROUPS.flatMap((g) => g.rows).find((r) => r.name === 'Bring my Liked Music in')
  assert.equal(bring?.kind, 'action')
})

test('a row that needs another is off and still while that one is off', () => {
  const row = GROUPS.flatMap((g) => g.rows).find((r) => r.name === 'How much I listened')!
  const plays = { autoplay: true, play_my_copy: true, hide_videos: false, hide_blocked: true }
  assert.deepEqual(rowOn(row, { ...SAFE, listening_time: true }, plays), { on: false, disabled: true })
  assert.deepEqual(rowOn(row, { ...SAFE, count_plays: true, listening_time: true }, plays), { on: true, disabled: false })
  const autoplay = GROUPS.flatMap((g) => g.rows).find((r) => r.name === 'Autoplay')!
  assert.deepEqual(rowOn(autoplay, SAFE, { ...plays, autoplay: false }), { on: false, disabled: false })
})

test('how it plays takes just its four out of the listening choices', () => {
  const l = { autoplay: false, play_my_copy: true, hide_videos: true, hide_blocked: false, history: true, disliked: [] }
  assert.deepEqual(playChoicesOf(l), { autoplay: false, play_my_copy: true, hide_videos: true, hide_blocked: false })
})

test('the title bar says in a few words what is on', () => {
  assert.equal(summary(SAFE), 'Plays not counted, likes off')
  assert.equal(summary({ ...SAFE, count_plays: true }), 'Counting plays, likes off')
  assert.equal(summary({ ...SAFE, count_plays: true, likes: true, signed_search: true }), 'Counting plays, likes on, signed-in search')
  assert.equal(summary({ ...SAFE, personal: false }), 'Plays not counted, likes off, general recommendations')
})

test('the account card never says search is yours while it goes anonymous', () => {
  const line = accountLine(SAFE)
  assert.match(line, /Home and radios are yours/)
  assert.match(line, /Searches stay anonymous/)
  assert.doesNotMatch(line, /search(es)? (and|are) yours/i)
  assert.match(accountLine({ ...SAFE, signed_search: true }), /Searches go as you/)
  assert.match(accountLine({ ...SAFE, personal: false }), /general ones/)
  assert.match(accountLine({ ...SAFE, library: false }), /library stays out of Venuta/)
})

test('what Venuta tells YouTube Music is said plainly, as it is sent', () => {
  const line = (name: string) => GROUPS.flatMap((g) => g.rows).find((r) => r.name === name)!.line
  // Count plays: what goes (that you played it), when (30 seconds in) and what never does.
  assert.match(line('Count plays'), /After 30 seconds of an online song, Venuta tells YouTube Music you played it/)
  assert.match(line('Count plays'), /Your files are never sent\./)
  // How much I listened: the seconds heard, while it plays and when it ends or is skipped.
  assert.match(line('How much I listened'), /how many seconds of each online song you heard/)
  assert.match(line('How much I listened'), /skip/)
  for (const n of ['Count plays', 'How much I listened']) assert.doesNotMatch(line(n), /—/, 'no em dashes')
})

test('count plays says where a play lands, and where when the security check cannot be passed', () => {
  const row = GROUPS.flatMap((g) => g.rows).find((r) => r.name === 'Count plays')!
  assert.match(row.line, /YouTube Music history/)
  assert.match(row.line, /security check/)
  assert.match(row.line, /YouTube watch history/)
  assert.ok(!row.line.includes(String.fromCharCode(0x2014)), 'no em dash')
})
