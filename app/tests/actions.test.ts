// node --test tests/   (Node runs the TypeScript as is)
// The one list of what can be done with each thing (lib/actionModel.ts): every right-click, every
// "…" and the command palette read it, so no card or row can be missing an action again.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { actionsFor, LABELS, type ActionId } from '../src/lib/actionModel.ts'

const ids = (groups: ActionId[][]) => groups.flat()
const PLAY: ActionId[] = ['play', 'shuffle', 'next', 'queue']

test('albums, artists and playlists play the same four ways, first', () => {
  for (const g of [actionsFor('album', {}), actionsFor('artist', {}), actionsFor('playlist', {})]) {
    assert.deepEqual(g[0], PLAY)
  }
})

test('a song plays, plays next or joins the queue, first', () => {
  assert.deepEqual(actionsFor('song', {})[0], ['play', 'next', 'queue'])
})

test('everything with songs in it can be added to a playlist', () => {
  for (const kind of ['song', 'album', 'artist', 'playlist', 'queued'] as const) {
    assert.ok(ids(actionsFor(kind, {})).includes('playlist'), kind)
  }
})

test('a song is the same song wherever it is: the list, a playlist, Now Playing', () => {
  const base = ids(actionsFor('song', {}))
  const inPlaylist = ids(actionsFor('song', { inPlaylist: true }))
  const playing = ids(actionsFor('song', { nowPlaying: true }))
  for (const id of base) {
    assert.ok(inPlaylist.includes(id), `in a playlist: ${id}`)
    assert.ok(playing.includes(id), `in Now Playing: ${id}`)
  }
  assert.ok(base.includes('artist') && base.includes('album') && base.includes('details'))
})

test('a song in a playlist can be removed from it, but not from a smart playlist', () => {
  assert.equal(ids(actionsFor('song', { inPlaylist: true })).at(-1), 'remove')
  assert.ok(!ids(actionsFor('song', { inPlaylist: true, smart: true })).includes('remove'))
  assert.ok(!ids(actionsFor('song', {})).includes('remove'))
})

test('a song with no artist has no Go to artist', () => {
  assert.ok(!ids(actionsFor('song', { noArtist: true })).includes('artist'))
})

test('an artist card opens the artist and plays all of them', () => {
  const a = ids(actionsFor('artist', {}))
  assert.ok(a.includes('open'))
})

test('a playlist is managed the same from every card and its page', () => {
  const p = ids(actionsFor('playlist', {}))
  for (const id of ['rename', 'duplicate', 'export', 'delete'] as const) assert.ok(p.includes(id), id)
  assert.equal(p.at(-1), 'delete', 'deleting is last, on its own')
  const smart = ids(actionsFor('playlist', { smart: true }))
  assert.ok(!smart.includes('rename') && !smart.includes('delete'))
  assert.ok(smart.includes('duplicate'))
  assert.ok(ids(actionsFor('playlist', { sourced: true })).includes('update'))
  assert.ok(ids(actionsFor('playlist', { missing: true })).includes('refind'))
  assert.ok(ids(actionsFor('playlist', { missing: true })).includes('fillOnline'), 'songs to get can be filled online')
})

test('a song still to get is found, copied or looked up, and can be removed', () => {
  const w = ids(actionsFor('wanted', {}))
  assert.deepEqual(w, ['playOnline', 'useOnline', 'find', 'copy', 'web', 'remove'])
  assert.equal(ids(actionsFor('wanted', { maybe: true }))[0], 'use')
})

test('the playing song in the queue cannot be removed; the others can', () => {
  assert.ok(!ids(actionsFor('queued', { current: true })).includes('remove'))
  assert.equal(ids(actionsFor('queued', {})).at(-1), 'remove')
})

test('no action appears twice, and every one has words', () => {
  for (const kind of ['song', 'album', 'artist', 'playlist', 'wanted', 'queued'] as const) {
    for (const facts of [{}, { inPlaylist: true }, { nowPlaying: true }, { sourced: true, missing: true }, { maybe: true }]) {
      const list = ids(actionsFor(kind, facts))
      assert.equal(new Set(list).size, list.length, `${kind} ${JSON.stringify(facts)}`)
      for (const id of list) assert.ok(LABELS[id], id)
    }
  }
})

test('an online song plays and is added like any song, but has no file to show or edit', () => {
  const o = ids(actionsFor('song', { online: true, nowPlaying: true }))
  // Online albums have pages of their own, so an online song goes to its album too.
  for (const id of ['play', 'next', 'queue', 'playlist', 'artist', 'album', 'moreOnline', 'web'] as const) assert.ok(o.includes(id), id)
  for (const id of ['details', 'folder', 'lyrics'] as const) assert.ok(!o.includes(id), id)
  assert.ok(ids(actionsFor('song', { online: true, inPlaylist: true })).includes('find'), 'in a playlist: find it in your library')
})

test('an online album opens, plays and is added, with nothing local to edit', () => {
  const a = ids(actionsFor('album', { online: true }))
  assert.deepEqual(actionsFor('album', { online: true })[0], PLAY)
  assert.ok(a.includes('open') && a.includes('playlist'))
  assert.ok(!a.includes('details') && !a.includes('folder'))
})

test('a song can be liked; an online song can also be disliked and its link copied', () => {
  const local = ids(actionsFor('song', {}))
  const online = ids(actionsFor('song', { online: true }))
  assert.ok(local.includes('like') && online.includes('like'))
  assert.ok(online.includes('dislike') && !local.includes('dislike'), 'dislike steers what the catalog plays next')
  assert.ok(online.includes('share') && !local.includes('share'), 'a file of yours has no link')
  // Liking sits right after playing, as in a music app's menu.
  assert.deepEqual(actionsFor('song', { online: true })[1], ['radio', 'like', 'dislike'])
})

test('an online song goes to its artist and its album, like a song of yours', () => {
  const online = ids(actionsFor('song', { online: true }))
  assert.ok(online.includes('artist') && online.includes('album'))
  assert.ok(!online.includes('details') && !online.includes('folder'), 'no file to edit or show')
})

test('a song starts a radio; an online song can block its artist', () => {
  for (const facts of [{}, { online: true }]) assert.ok(ids(actionsFor('song', facts)).includes('radio'), JSON.stringify(facts))
  assert.ok(ids(actionsFor('song', { online: true })).includes('block'))
  assert.ok(!ids(actionsFor('song', { online: true, noArtist: true })).includes('block'))
  // Radio and liking come right after the ways to play.
  assert.deepEqual(actionsFor('song', {})[1], ['radio', 'like'])
})

test('an artist starts their radio', () => {
  assert.ok(ids(actionsFor('artist', {})).includes('radio'))
})

test('albums, artists and playlists can be added to Home', () => {
  for (const kind of ['album', 'artist', 'playlist'] as const) assert.ok(ids(actionsFor(kind, {})).includes('shortcut'), kind)
  assert.ok(ids(actionsFor('album', { online: true })).includes('shortcut'))
})

test('a song in the queue plays, plays next, starts its radio, goes to a playlist, its artist and album', () => {
  const q = ids(actionsFor('queued', {}))
  for (const id of ['play', 'next', 'radio', 'playlist', 'artist', 'album', 'remove'] as const) assert.ok(q.includes(id), id)
  // The one playing: nothing to play or put next, and it stays.
  const now = ids(actionsFor('queued', { current: true }))
  for (const id of ['play', 'next', 'remove'] as const) assert.ok(!now.includes(id), id)
  assert.ok(now.includes('radio'))
})
