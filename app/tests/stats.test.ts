// node --test tests/   (Node runs the TypeScript as is)
// Listening stats, from the plays the app keeps: your files and online songs counted alike.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { statsOf, type Play, type SongInfo } from '../src/lib/stats.ts'

const day = (d: number, h = 12) => new Date(2026, 9, d, h, 0, 0).getTime()
const MIN = 60_000
const play = (d: number, file: string, minutes: number, h = 12, extra: Partial<Play> = {}): Play => ({
  at: day(d, h),
  heard_ms: minutes * MIN,
  file,
  title: null,
  artists: [],
  album: null,
  art: null,
  ctx: null,
  ...extra,
})
const library: Record<string, SongInfo> = {
  'A/1.flac': { title: 'So What', artists: ['Miles Davis'], album: 'Kind of Blue', dir: 'A' },
  'A/2.flac': { title: 'Blue In Green', artists: ['Miles Davis'], album: 'Kind of Blue', dir: 'A' },
}
const online = (d: number, minutes: number, h = 20) =>
  play(d, 'online:remote_catalog:z1', minutes, h, { title: 'Zingaat', artists: ['Ajay-Atul'], album: 'Sairat', art: 'https://x/y.jpg' })

const plays: Play[] = [
  online(18, 4),
  play(18, 'A/1.flac', 9, 9),
  play(17, 'A/1.flac', 9, 9),
  play(17, 'A/2.flac', 5, 22),
  online(15, 4),
]
const s = statsOf(plays, (f) => library[f] ?? null, { from: day(12, 0), to: day(18, 23), now: day(18, 23) })

test('totals count your files and online songs alike', () => {
  assert.equal(s.plays, 5)
  assert.equal(s.ms, 31 * MIN)
  assert.equal(s.songs, 3)
  assert.equal(s.artists, 2)
})

test('the streak is the days in a row up to today with a play', () => {
  assert.equal(s.streak, 2, '17th and 18th; the 16th had none')
})

test('every day of the range is in the activity, with no plays as zero', () => {
  assert.equal(s.daily.length, 7)
  assert.deepEqual(
    s.daily.map((d) => d.ms / MIN),
    [0, 0, 0, 4, 0, 14, 13],
  )
  assert.equal(s.busiest?.ms, 14 * MIN)
})

test('top artists, songs and albums by time listened, with their share', () => {
  assert.equal(s.topArtists[0].name, 'Miles Davis')
  assert.equal(s.topArtists[0].plays, 3)
  assert.equal(Math.round(s.topArtists[0].share * 100), 74)
  assert.equal(s.topSongs[0].title, 'So What')
  assert.equal(s.topSongs[0].plays, 2)
  assert.equal(s.topAlbums[0].album, 'Kind of Blue')
  const z = s.topSongs.find((x) => x.title === 'Zingaat')
  assert.equal(z?.art, 'https://x/y.jpg')
  assert.equal(z?.file, 'online:remote_catalog:z1')
})

test('time of day: what was heard in each hour', () => {
  assert.equal(s.hours.length, 24)
  assert.equal(s.hours[9], 18 * MIN)
  assert.equal(s.hours[20], 8 * MIN)
  assert.equal(s.hours[22], 5 * MIN)
})

test('a library song that is gone still counts, by its file name', () => {
  const t = statsOf([play(18, 'Gone/x - Song.flac', 3)], () => null, { from: day(18, 0), to: day(18, 23), now: day(18, 23) })
  assert.equal(t.topSongs[0].title, 'x - Song')
  assert.equal(t.artists, 0)
})
