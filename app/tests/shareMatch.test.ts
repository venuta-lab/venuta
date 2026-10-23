// node --test tests/   (Node runs the TypeScript as is)
// Sharing a song of yours by its YouTube Music link (lib/shareMatch.ts): only the same song, the
// same version of it, is ever shared; anything less certain is offered, not chosen.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { shareMatch, versionWords } from '../src/lib/shareMatch.ts'

const online = (track_id: string, title: string, artists: string[], duration_ms: number | null, isrc: string | null = null) => ({
  provider: 'ytm',
  track_id,
  title,
  artists,
  album: null,
  album_id: null,
  duration_ms,
  artwork_url: null,
  isrc,
})

const tumHiHo = { title: 'Tum Hi Ho', artists: ['Arijit Singh'], duration_ms: 262_000 }

test('the same song by title, artist and length is chosen, the first such result', () => {
  const r = shareMatch(tumHiHo, [online('a', 'Tum Hi Ho', ['Arijit Singh'], 261_000), online('b', 'Tum Hi Ho', ['Arijit Singh'], 262_000)])
  assert.equal(r.pick?.track_id, 'a')
})

test('film credits and features do not change the song; the script it is written in does not either', () => {
  assert.equal(shareMatch(tumHiHo, [online('a', 'Tum Hi Ho (From "Aashiqui 2")', ['Arijit Singh', 'Mithoon'], 262_500)]).pick?.track_id, 'a')
  const kesariya = { title: 'केसरिया', artists: ['अरिजीत सिंह'], duration_ms: 268_000 }
  assert.equal(shareMatch(kesariya, [online('k', 'Kesariya', ['Arijit Singh'], 268_400)]).pick?.track_id, 'k')
})

test('another version of the song is never chosen: live, remix, acoustic, slowed, reprise', () => {
  for (const title of ['Tum Hi Ho (Live)', 'Tum Hi Ho - Remix', 'Tum Hi Ho (Acoustic)', 'Tum Hi Ho (Slowed + Reverb)', 'Tum Hi Ho (Reprise)', 'Tum Hi Ho (Unplugged)', 'Tum Hi Ho (Lofi)']) {
    assert.equal(shareMatch(tumHiHo, [online('v', title, ['Arijit Singh'], 262_000)]).pick, undefined, title)
  }
  // And the other way: your file is the live one.
  const live = { title: 'Tum Hi Ho (Live)', artists: ['Arijit Singh'], duration_ms: 262_000 }
  assert.equal(shareMatch(live, [online('s', 'Tum Hi Ho', ['Arijit Singh'], 262_000)]).pick, undefined)
  assert.equal(shareMatch(live, [online('l', 'Tum Hi Ho (Live)', ['Arijit Singh'], 262_000)]).pick?.track_id, 'l')
})

test('a length more than 4 s off, another artist, or no length online is not chosen but offered', () => {
  const results = [
    online('edit', 'Tum Hi Ho', ['Arijit Singh'], 230_000),
    online('cover', 'Tum Hi Ho', ['Someone Else'], 262_000),
    online('nolen', 'Tum Hi Ho', ['Arijit Singh'], null),
    online('other', 'Something Else', ['Arijit Singh'], 262_000),
  ]
  const r = shareMatch(tumHiHo, results)
  assert.equal(r.pick, undefined)
  assert.deepEqual(
    r.offered.map((t) => t.track_id),
    ['edit', 'cover', 'nolen'],
    'the top three to choose from'
  )
})

test('the same recording code is certain, and a different one is never the same', () => {
  const tagged = { ...tumHiHo, isrc: 'INT101300000' }
  assert.equal(shareMatch(tagged, [online('x', 'Tum Hi Ho (Lyric Video)', ['T-Series'], 300_000, 'INT101300000')]).pick?.track_id, 'x')
  assert.equal(shareMatch(tagged, [online('y', 'Tum Hi Ho', ['Arijit Singh'], 262_000, 'USXXX0000001')]).pick, undefined)
})

test('version words are read from brackets, dashes and plain words alike', () => {
  assert.deepEqual(versionWords('Song (Live at Wembley)'), ['live'])
  assert.deepEqual(versionWords('Song - Acoustic Version'), ['acoustic'])
  assert.deepEqual(versionWords('Song (Slowed + Reverb)'), ['reverb', 'slowed'])
  assert.deepEqual(versionWords('Song (Sped Up)'), ['sped up'])
  assert.deepEqual(versionWords('Song (Remastered 2009)'), [], 'a remaster is the same recording')
  assert.deepEqual(versionWords('Live Is Life'), [], 'a title that begins with the word is the song')
})
