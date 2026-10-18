// node --test tests/   (Node runs the TypeScript as is)
// "Your music": the tiles that open everything on this PC.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { albumHasGenre, genresOf, yourMusicTiles, type TileAlbum } from '../src/lib/yourMusic.ts'

const album = (dir: string, genre: string, category = 'International'): TileAlbum => ({ dir, genre, category })

test('a tag names its genres once each, tidied', () => {
  assert.deepEqual(genresOf('Bollywood; Pop'), ['Bollywood', 'Pop'])
  assert.deepEqual(genresOf(' Rock ; rock, Indie '), ['Rock', 'Indie'])
  assert.deepEqual(genresOf('Singer/Songwriter'), ['Singer/Songwriter'], 'a slash is part of the name')
  assert.deepEqual(genresOf(''), [])
  assert.ok(albumHasGenre(album('a', 'Jazz; Soul'), 'soul'))
})

test('tiles open albums, artists, songs, Hi-Res, categories, then the genres you have most', () => {
  const albums = [
    album('a', 'Jazz'),
    album('b', 'Jazz; Soul'),
    album('c', 'Soul'),
    album('d', 'Classical', 'Classical'),
    album('e', 'Classical', 'Classical'),
    album('f', 'Rock'),
  ]
  const tiles = yourMusicTiles({
    albums,
    artists: [{ albums: [albums[0]] }, { albums: [albums[3]] }],
    songs: 1,
    hiRes: [albums[3]],
    categories: [
      ['International', 4],
      ['Classical', 2],
      ['Scores', 1],
    ],
  })
  assert.deepEqual(
    tiles.map((t) => t.title),
    ['Albums', 'Artists', 'Songs', 'Hi-Res', 'International', 'Classical', 'Jazz', 'Soul'],
    'Classical is a category already; Rock has one album; Scores has one album',
  )
  assert.equal(tiles[2].count, '1 song')
  assert.equal(tiles[0].count, '6 albums')
  assert.deepEqual(tiles.find((t) => t.title === 'Jazz')?.opens, { view: 'albums', filter: 'genre:Jazz' })
  assert.ok(tiles.every((t) => t.covers.length > 0 && t.covers.length <= 3))
})

test('an empty library has no tiles', () => {
  assert.deepEqual(yourMusicTiles({ albums: [], artists: [], songs: 0, hiRes: [], categories: [] }), [])
})
