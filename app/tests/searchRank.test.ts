// node --test tests/   (Node runs the TypeScript as is)
import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { OnlineAlbum, OnlineArtist, OnlineTrack } from '../src/lib/engine.ts'
import {
  cleanTitle,
  isCoverClass,
  partitionBestAndCovers,
  pickTop,
  rankOnlineAlbums,
  rankOnlineArtists,
  rankOnlineTracks,
  searchIntent,
  songKey,
  versionClass,
} from '../src/lib/searchRank.ts'

let n = 0
const song = (title: string, artists: string[], more: Partial<OnlineTrack> = {}): OnlineTrack => ({
  provider: 'remote_catalog',
  track_id: `t${++n}`,
  title,
  artists,
  album: null,
  album_id: null,
  duration_ms: 240_000,
  artwork_url: null,
  isrc: null,
  ...more,
})
const atv = (title: string, artists: string[], album: string, plays: number | null, more: Partial<OnlineTrack> = {}) =>
  song(title, artists, { video_type: 'ATV', album, album_id: `MPRE_${album}`, plays, ...more })
const titles = (list: OnlineTrack[]) => list.map((t) => `${t.title} — ${t.artists.join(', ')}`)

// Shaped like the live catalog's answer for "wonderwall" (P1 check, 2026-10-15).
const oasis = atv('Wonderwall', ['Oasis'], "(What's The Story) Morning Glory?", 1_700_000_000, { shelf_rank: 3, duration_ms: 258_000 })
const oasisDeluxe = atv('Wonderwall', ['Oasis'], "(What's The Story) Morning Glory? (Deluxe Remastered Edition)", 1_700_000_000, { shelf_rank: 5, duration_ms: 258_500 })
const robyn = atv('Wonderwall', ['Robyn Adele Anderson'], 'Wonderwall', 288_000, { shelf_rank: 4, duration_ms: 242_000 })
const trailer = atv('Wonderwall (Epic Trailer Version) (feat. Miranda Dianne)', ['J2'], 'The Iconic Series, Vol. 7', 14_000)
const metal = atv('Wonderwall (Metal Cover)', ['Frog Leap Studios'], 'Wonderwall (Metal Cover)', 228_000)
const unplugged = atv('Wonderwall (Unplugged)', ['Oasis'], "(What's The Story) Morning Glory? (30th Anniversary Deluxe Edition)", 9_600_000)
const lofi = atv('Wonderwall', ['Lofi Fruits Music', 'Chill Fruits Music'], '90s Oldschool Lofi Hip Hop', 626_000)
const lullaby = atv('Wonderwall', ['Twinkle Twinkle Little Rock Star'], 'Lullaby Versions of Oasis', 14_000)
const orchestra = atv('Wonderwall', ['The Royal Philharmonic Orchestra'], 'Plays the Music of Oasis', 168_000)
const goot = atv('Wonderwall', ['Alex Goot'], 'Songs I Wish I Wrote: Volume 2', 11_000_000)
const slowed = song('Oasis - Wonderwall (slowed + reverb)', ['everydayslowed'], { video_type: 'UGC', plays: 308_000, shelf_rank: 0, duration_ms: 287_000 })
const fanCover = song('Wonderwall cover', ['Guitar Guy'], { video_type: 'UGC', plays: 288_000, shelf_rank: 1 })
const topCover = song('Wonderwall', ['Robyn Adele Anderson'], { video_type: 'OMV', plays: 234_000, is_top: true, duration_ms: 242_500 })
const oasisVideo = song('Oasis - Wonderwall (Official Video)', ['Oasis'], { video_type: 'OMV', plays: 707_000_000, shelf_rank: 2, duration_ms: 258_000 })
const WONDERWALL = [slowed, fanCover, topCover, robyn, oasis, oasisDeluxe, trailer, metal, unplugged, lofi, lullaby, orchestra, goot, oasisVideo]

test('version class: the artist’s audio, its video, other versions, remixes and copies', () => {
  assert.equal(versionClass(oasis), 'official')
  assert.equal(versionClass(oasisVideo), 'video')
  assert.equal(versionClass(unplugged), 'alternate')
  assert.equal(versionClass(song('Kesariya (Remix)', ['DJ'], { video_type: 'ATV', album: 'x' })), 'remix')
  for (const t of [metal, lofi, lullaby, orchestra, slowed, fanCover]) assert.equal(versionClass(t), 'cover', t.title)
  assert.equal(versionClass(song('Tum Hi Ho Lyrics', ['Fan Uploads'], { video_type: 'UGC' })), 'cover')
  assert.equal(versionClass(song('Tum Hi Ho', ['Someone'], { video_type: 'UGC' })), 'cover')
  // An official video whose title says cover is still a cover.
  assert.equal(versionClass(song('Wonderwall (Acoustic Cover)', ['Band'], { video_type: 'OMV' })), 'cover')
  // An upload of a live performance, when live is what was asked.
  assert.equal(versionClass(song('Wonderwall', ['A Fan'], { video_type: 'UGC' }), 'wonderwall live'), 'alternate')
  // Kind unknown: an album says it's the recording.
  assert.equal(versionClass(song('Kesariya', ['Arijit Singh'], { album: 'Brahmastra' })), 'official')
  assert.equal(searchIntent('kesariya karaoke'), 'cover')
  assert.equal(searchIntent('wonderwall remix'), 'remix')
  assert.equal(searchIntent('wonderwall unplugged'), 'alternate')
  assert.equal(searchIntent('wonderwall'), null)
  assert.equal(cleanTitle('Kesariya (From "Brahmastra")'), 'Kesariya')
  assert.equal(cleanTitle('Oasis - Wonderwall (Official Video)'), 'Oasis - Wonderwall')
  assert.equal(cleanTitle('Raabta (feat. Arijit Singh)'), 'Raabta')
  assert.equal(songKey('Oasis - Wonderwall (slowed + reverb)'), songKey('Wonderwall (Unplugged)'))
  assert.equal(versionClass(trailer), 'cover')
})

test('wonderwall: the Oasis recording first, covers after every non-cover, one copy of each', () => {
  const ranked = rankOnlineTracks('wonderwall', WONDERWALL)
  assert.equal(ranked[0], oasis, titles(ranked).join('\n'))
  // The same recording twice (album and deluxe edition, same length): kept once.
  assert.equal(ranked.filter((t) => t.artists[0] === 'Oasis' && t.title === 'Wonderwall').length, 1)
  const at = (t: OnlineTrack) => ranked.indexOf(t)
  for (const cover of [fanCover, slowed, metal, lofi, lullaby, orchestra]) assert.ok(at(cover) > at(oasisVideo), cover.title)
  // The catalog's top card (a cover's video) never beats the official recording: here it is
  // the same recording as the cover's audio track (same artist, length), so it is merged away.
  assert.ok(at(topCover) === -1 || at(topCover) > at(oasisVideo))
  // Every cover-kind after every other row.
  const firstCover = ranked.findIndex((t) => versionClass(t, 'wonderwall') === 'cover')
  assert.ok(ranked.slice(firstCover).every((t) => versionClass(t, 'wonderwall') === 'cover'), titles(ranked).join('\n'))
  // Another artist's recording of the same title, with a sliver of the plays, follows Oasis.
  assert.ok(at(robyn) > at(unplugged) && at(goot) > at(oasisVideo))
})

test('kesariya and tum hi ho: the film recording on its album first', () => {
  const arijit = atv('Kesariya (From "Brahmastra")', ['Arijit Singh'], 'Kesariya (From "Brahmastra")', 966_000_000, { shelf_rank: 3 })
  const kesariya = [
    song('Kesariya Lofi', ['Daarrrsssshhhhh'], { video_type: 'ATV', album: 'Kesariya Lofi', plays: 1_800 }),
    song('Kesariya (Slowed + Reverb)', ['Lofi World'], { video_type: 'UGC', plays: 4_000_000, shelf_rank: 0, is_top: true }),
    atv('Kesariya Balam', ['Ali'], 'Kesariya Balam', 2_500),
    song('Kesariya Karaoke With Lyrics', ['Karaoke Point'], { video_type: 'UGC', plays: 90_000 }),
    arijit,
  ]
  const ranked = rankOnlineTracks('kesariya', kesariya)
  assert.equal(ranked[0], arijit, titles(ranked).join('\n'))

  const tumHiHo = atv('Tum Hi Ho', ['Arijit Singh'], 'Aashiqui 2', 1_200_000_000, { shelf_rank: 3 })
  const ranked2 = rankOnlineTracks('tum hi ho', [
    song('Tum Hi Ho Lyrics', ['Lyrics Hub'], { video_type: 'UGC', plays: 50_000_000, shelf_rank: 0 }),
    song('Tum Hi Ho Full Video Song | Aashiqui 2', ['T-Series'], { video_type: 'OMV', plays: 400_000_000, shelf_rank: 1 }),
    atv('Tum Hi Ho (Female Version)', ['Someone'], 'Covers', 20_000),
    tumHiHo,
  ])
  assert.equal(ranked2[0], tumHiHo, titles(ranked2).join('\n'))
  // The title and its artist typed together still find it first.
  assert.equal(rankOnlineTracks('tum hi ho arijit', [song('Tum Hi Ho', ['Fan'], { video_type: 'UGC' }), tumHiHo])[0], tumHiHo)
})

test('asking for a cover, karaoke or a version lifts it', () => {
  const ranked = rankOnlineTracks('wonderwall cover', WONDERWALL)
  assert.equal(versionClass(ranked[0], 'wonderwall cover'), 'cover', titles(ranked).join('\n'))
  assert.ok(ranked.indexOf(oasis) > ranked.indexOf(metal))

  const karaoke = song('Kesariya Karaoke With Lyrics', ['Karaoke Point'], { video_type: 'UGC', plays: 90_000 })
  const arijit = atv('Kesariya (From "Brahmastra")', ['Arijit Singh'], 'Brahmastra', 966_000_000)
  assert.equal(rankOnlineTracks('kesariya karaoke', [arijit, karaoke])[0], karaoke)

  assert.equal(rankOnlineTracks('wonderwall unplugged', WONDERWALL)[0], unplugged)
})

test('the same recording under two ids is one row; different lengths or artists are not', () => {
  const a = atv('Raabta', ['Pritam', 'Arijit Singh'], 'Agent Vinod', 89_000_000, { duration_ms: 243_000 })
  const b = atv('Raabta (From "Agent Vinod")', ['Arijit Singh'], 'Best of 2012', 1_000, { duration_ms: 244_500 })
  const ranked = rankOnlineTracks('raabta', [b, a])
  assert.deepEqual(ranked, [a])
  const longer = atv('Raabta', ['Arijit Singh'], 'Live', 5_000, { duration_ms: 300_000 })
  assert.equal(rankOnlineTracks('raabta', [a, longer]).length, 2)
  const noArtistA = song('Raabta', [], { video_type: 'ATV', album: 'x' })
  const noArtistB = song('Raabta', [], { video_type: 'ATV', album: 'y' })
  assert.equal(rankOnlineTracks('raabta', [noArtistA, noArtistB]).length, 2)
})

test('the catalog’s top result only nudges, and never lifts a cover', () => {
  const plain = rankOnlineTracks('wonderwall', [topCover, oasis])
  assert.deepEqual(plain, [oasis, topCover])
  // Between two official rows otherwise alike, the top one goes first.
  const a = atv('Song', ['X'], 'A', 1_000_000)
  const b = atv('Song', ['X'], 'B', 1_000_000, { is_top: true, duration_ms: 200_000 })
  assert.equal(rankOnlineTracks('song', [a, b])[0], b)
})

test('odd lengths sink unless a long listen is asked for; a wanted length lifts the closest', () => {
  const hour = song('Wonderwall 1 Hour', ['Loops'], { video_type: 'UGC', duration_ms: 3_600_000, plays: 2_000_000 })
  const clip = atv('Wonderwall', ['Oasis'], 'Clips', 5_000_000, { duration_ms: 30_000 })
  assert.equal(rankOnlineTracks('wonderwall', [hour, clip, oasis])[0], oasis)
  const short = atv('Wonderwall', ['Oasis'], 'Morning Glory', 1_700_000_000, { duration_ms: 258_000 })
  const live = atv('Wonderwall', ['Oasis'], 'Live at Knebworth', 1_000_000, { duration_ms: 290_000 })
  // A playlist that remembers the live recording's length.
  const picked = rankOnlineTracks('wonderwall oasis', [short, live], { wantDurationMs: 289_500 })
  assert.equal(picked[0], live)
})

test('best and covers split for the Covers & other view', () => {
  const { best, covers } = partitionBestAndCovers(WONDERWALL, 'wonderwall')
  assert.equal(best[0], oasis)
  assert.ok(covers.includes(fanCover) && covers.includes(slowed) && !best.includes(fanCover))
  assert.ok(covers.every((t) => isCoverClass(t, 'wonderwall')))
  // Asked for: nothing is set aside as a cover.
  assert.equal(partitionBestAndCovers(WONDERWALL, 'wonderwall cover').covers.length, 0)
})

test('albums and artists: the name first, then real albums and big audiences', () => {
  const album = (title: string, artists: string[], kind: string | null): OnlineAlbum => ({
    provider: 'remote_catalog',
    album_id: title,
    title,
    artists,
    track_count: null,
    release_date: null,
    artwork_url: null,
    kind,
  })
  const ranked = rankOnlineAlbums('brahmastra', [
    album('Brahmastra Karaoke Tracks', ['Karaoke Point'], 'Album'),
    album('Kesariya', ['Arijit Singh'], 'Single'),
    album('Brahmastra', ['Pritam'], 'Album'),
  ])
  assert.equal(ranked[0].title, 'Brahmastra')
  assert.equal(ranked[2].title, 'Kesariya')
  const artist = (name: string, audience: number | null): OnlineArtist => ({ provider: 'remote_catalog', artist_id: name, name, artwork_url: null, audience })
  const artists = rankOnlineArtists('arijit singh', [artist('Arijit Singh Fan Club', 1_000), artist('Atif Aslam', 300_000_000), artist('Arijit Singh', 642_000_000)])
  assert.equal(artists[0].name, 'Arijit Singh')
  assert.equal(artists[2].name, 'Atif Aslam')
})

test('the offline fake catalog splits into Best and Covers & other', () => {
  // The rows FakeMediaAccess returns for any query (crates/media-access/src/fake.rs).
  const q = 'hello'
  const fake = [
    atv(`${q} (fixture)`, ['Fixture Artist'], 'Fixture Album', 12_000_000, { shelf_rank: 1, duration_ms: 215_000 }),
    song(`${q} (fixture) (Official Video)`, ['Fixture Artist'], { video_type: 'OMV', plays: 3_400_000, shelf_rank: 2, duration_ms: 221_000 }),
    song(`${q} (fixture) (acoustic cover)`, ['A Fan'], { video_type: 'UGC', plays: 25_000, is_top: true, shelf_rank: 0, duration_ms: 230_000 }),
    song(`${q} (fixture) (Karaoke Version)`, ['Karaoke Hits'], { video_type: 'UGC', duration_ms: 218_000 }),
  ]
  const { best, covers } = partitionBestAndCovers(fake, q)
  assert.deepEqual(best.map((t) => t.video_type), ['ATV', 'OMV'])
  assert.equal(covers.length, 2)
  // Asking for karaoke: nothing set aside, karaoke first.
  const asked = partitionBestAndCovers(fake, `${q} karaoke`)
  assert.equal(asked.covers.length, 0)
  assert.match(asked.best[0].title, /Karaoke/)
})

test('top result: an artist named exactly, else a song (yours first), else an album', () => {
  const none = { tracks: [], albums: [], artists: [] }
  const onlineArtist = (name: string, audience = 600_000_000): OnlineArtist => ({ provider: 'p', artist_id: name, name, artwork_url: null, audience })
  const onlineAlbum = (title: string): OnlineAlbum => ({ provider: 'p', album_id: title, title, artists: [], track_count: null, release_date: null, artwork_url: null })
  const online = (songs: OnlineTrack[], albums: OnlineAlbum[] = [], artists: OnlineArtist[] = []) => ({ songs, albums, artists })
  // An artist's name, with no widely played song of that name: the artist (online).
  assert.deepEqual(pickTop('arijit singh', none, online([atv('Raabta', ['Arijit Singh'], 'Agent Vinod', 89_000_000)], [], [onlineArtist('Arijit Singh')])), { kind: 'artist', where: 'online', index: 0 })
  // A band named like a famous song: the song wins.
  assert.deepEqual(pickTop('wonderwall', none, online([oasis], [], [onlineArtist('Wonderwall', 5_000)])), { kind: 'song', where: 'online', index: 0 })
  // Your own artist, your own file: always first.
  assert.deepEqual(pickTop('oasis', { ...none, artists: [{ name: 'Oasis' }] }, online([oasis], [], [onlineArtist('Oasis')])), { kind: 'artist', where: 'library', index: 0 })
  const lib = { tracks: [{ title: 'Some Other', artists: ['X'] }, { title: 'Wonderwall', artists: ['Oasis'] }], albums: [], artists: [] }
  assert.deepEqual(pickTop('wonderwall', lib, online([oasis])), { kind: 'song', where: 'library', index: 1 })
  // Title and artist typed together.
  assert.deepEqual(pickTop('wonderwall oasis', none, online([oasis])), { kind: 'song', where: 'online', index: 0 })
  // A film's name: its album.
  assert.deepEqual(pickTop('brahmastra', none, online([atv('Kesariya', ['Arijit Singh'], 'Brahmastra', 966_000_000)], [onlineAlbum('Brahmastra')])), { kind: 'album', where: 'online', index: 0 })
  assert.deepEqual(pickTop('brahmastra', { ...none, albums: [{ title: 'Brahmastra' }] }, online([], [onlineAlbum('Brahmastra')])), { kind: 'album', where: 'library', index: 0 })
  // Nothing named exactly: the closest song, the library winning ties; nothing at all: none.
  assert.deepEqual(pickTop('wonder', { ...none, tracks: [{ title: 'Wonderwall', artists: ['Oasis'] }] }, online([oasis])), { kind: 'song', where: 'library', index: 0 })
  assert.deepEqual(pickTop('wonder', none, null), null)
  assert.equal(pickTop('  ', none, online([oasis])), null)
})

test('a first name finds the artist people mean, not a small account named exactly that', () => {
  const artist = (name: string, audience: number | null): OnlineArtist => ({ provider: 'p', artist_id: name, name, artwork_url: null, audience })
  // As the catalog answered "arijit" (2026-10-18): a 59-subscriber profile is called just that.
  const ranked = rankOnlineArtists('arijit', [artist('Arijit', 59), artist('Arijit Singh', 647_000_000), artist('Arijit Barman', 2_000)])
  assert.equal(ranked[0].name, 'Arijit Singh')
  const none = { tracks: [], albums: [], artists: [] }
  assert.deepEqual(pickTop('arijit', none, { songs: [], albums: [], artists: ranked }), { kind: 'artist', where: 'online', index: 0 })
  // An exact name still wins when it is the one people listen to.
  const oasis = rankOnlineArtists('oasis', [artist('Oasis Tribute', 3_000), artist('Oasis', 30_000_000)])
  assert.equal(oasis[0].name, 'Oasis')
})
