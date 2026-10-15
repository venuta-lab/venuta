// node --test tests/   (Node runs the TypeScript as is)
// The search quality eval set (tests/fixtures/search-set.json) through the ranker and the Top
// result picker. Each case fails with the ranked list, so a bad order is easy to read.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import type { OnlineAlbum, OnlineArtist, OnlineTrack } from '../src/lib/engine.ts'
import { partitionBestAndCovers, pickTop, rankOnlineTracks, versionClass, type TopLibrary, type VersionClass } from '../src/lib/searchRank.ts'

interface Row {
  t: string
  a: string
  al?: string
  vt?: string
  p?: number
  d?: string
  top?: boolean
  r?: number
}
interface Case {
  query: string
  note?: string
  /** Use the rows of the first case with this query. */
  same?: string
  tracks?: Row[]
  artists?: { name: string; audience?: number }[]
  albums?: string[]
  library?: Partial<TopLibrary>
  wantMs?: number
  expect: {
    topArtist?: string[]
    topTitle?: string
    topAlbum?: string
    topClass?: VersionClass
    notFirst?: VersionClass[]
    coversLast?: boolean
    once?: string[]
    pick?: { kind: string; where: string }
  }
}

const set = JSON.parse(readFileSync(new URL('./fixtures/search-set.json', import.meta.url), 'utf8')) as { cases: Case[] }

const ms = (d?: string) => (d ? d.split(':').reduce((acc, x) => acc * 60 + Number(x), 0) * 1000 : 240_000)
const toTrack = (r: Row, i: number): OnlineTrack => ({
  provider: 'remote_catalog',
  track_id: `row-${i}`,
  title: r.t,
  artists: r.a.split(/\s*,\s*/),
  album: r.al ?? null,
  album_id: r.al ? `album:${r.al}` : null,
  duration_ms: ms(r.d),
  artwork_url: null,
  isrc: null,
  video_type: r.vt ?? null,
  plays: r.p ?? null,
  is_top: !!r.top,
  shelf_rank: r.r ?? null,
})
const rowsFor = (c: Case): Row[] => {
  if (c.tracks) return c.tracks
  const base = set.cases.find((x) => x.query === c.same && x.tracks)
  assert.ok(base, `no rows for "${c.query}" (same: ${c.same})`)
  return base.tracks!
}
const show = (list: OnlineTrack[], q: string) =>
  list.map((t, i) => `  ${i + 1}. [${versionClass(t, q)}] ${t.title} — ${t.artists.join(', ')}${t.album ? ` (${t.album})` : ''}`).join('\n')

test('the eval set is big enough to mean something', () => {
  assert.ok(set.cases.length >= 25, `${set.cases.length} cases`)
})

for (const [n, c] of set.cases.entries()) {
  test(`#${n + 1} “${c.query}”${c.note ? `: ${c.note}` : ''}`, () => {
    const q = c.query
    const tracks = rowsFor(c).map(toTrack)
    const ranked = rankOnlineTracks(q, tracks, { wantDurationMs: c.wantMs })
    const why = `ranked for “${q}”:\n${show(ranked, q)}`
    const first = ranked[0]
    const e = c.expect

    if (e.topArtist) {
      const artists = first.artists.join(', ').toLowerCase()
      assert.ok(e.topArtist.some((a) => artists.includes(a)), `first should be by ${e.topArtist.join(' or ')}\n${why}`)
    }
    if (e.topTitle) assert.ok(first.title.toLowerCase().includes(e.topTitle), `first title should hold “${e.topTitle}”\n${why}`)
    if (e.topAlbum) assert.ok((first.album ?? '').toLowerCase().includes(e.topAlbum), `first album should hold “${e.topAlbum}”\n${why}`)
    if (e.topClass) assert.equal(versionClass(first, q), e.topClass, `first should be ${e.topClass}\n${why}`)
    for (const cls of e.notFirst ?? []) assert.notEqual(versionClass(first, q), cls, `first must not be ${cls}\n${why}`)
    if (e.coversLast) {
      const classes = ranked.map((t) => versionClass(t, q))
      const firstCover = classes.indexOf('cover')
      assert.ok(firstCover < 0 || classes.slice(firstCover).every((x) => x === 'cover'), `covers must come last\n${why}`)
    }
    for (const pair of e.once ?? []) {
      const [title, artist] = pair.split('|')
      const n = ranked.filter((t) => t.title.toLowerCase() === title && t.artists.join(', ').toLowerCase().includes(artist)).length
      assert.equal(n, 1, `“${title}” by ${artist} once\n${why}`)
    }
    if (e.pick) {
      const { best } = partitionBestAndCovers(tracks, q)
      const artists: OnlineArtist[] = (c.artists ?? []).map((a) => ({ provider: 'remote_catalog', artist_id: a.name, name: a.name, artwork_url: null, audience: a.audience ?? null }))
      const albums: OnlineAlbum[] = (c.albums ?? []).map((t) => ({ provider: 'remote_catalog', album_id: t, title: t, artists: [], track_count: null, release_date: null, artwork_url: null }))
      const library: TopLibrary = { tracks: [], albums: [], artists: [], ...c.library }
      const pick = pickTop(q, library, { songs: best, albums, artists })
      assert.deepEqual(pick && { kind: pick.kind, where: pick.where }, e.pick, `Top result for “${q}”\n${why}`)
    }
  })
}
