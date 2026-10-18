/*
  "Your music": the way into everything on this PC, as tiles beside the catalog's moods and
  genres (Home and the Search page): your albums, artists and songs, Hi-Res, your categories,
  and the genres your tags name most. Pure, so tests/yourMusic.test.ts reads it.
*/

/** What a tile needs of an album. */
export interface TileAlbum {
  dir: string
  genre: string
  category: string
}

export interface YourMusicTile {
  id: string
  title: string
  /** "26 albums", "303 songs". */
  count: string
  /** What it opens: a Library view, or Albums with this filter. */
  opens: { view: 'albums' | 'artists' | 'songs'; filter?: string }
  /** Up to three album folders whose covers dress the tile. */
  covers: string[]
}

/** The genres one tag names ("Bollywood; Pop"), tidied, without repeats. A slash stays inside a
 *  genre ("Singer/Songwriter"). */
export function genresOf(tag: string): string[] {
  const seen = new Set<string>()
  const out: string[] = []
  for (const part of tag.split(/[;,|]/)) {
    const g = part.trim().replace(/\s+/g, ' ')
    const k = g.toLowerCase()
    if (!g || seen.has(k)) continue
    seen.add(k)
    out.push(g)
  }
  return out
}

/** Albums of a genre: Albums' filter for a genre tile (case does not matter). */
export const GENRE_FILTER = 'genre:'
export const albumHasGenre = (a: TileAlbum, genre: string) => genresOf(a.genre).some((g) => g.toLowerCase() === genre.toLowerCase())

const plural = (n: number, one: string, many: string) => `${n.toLocaleString('en-IN')} ${n === 1 ? one : many}`

/** Covers for a tile: the first albums of the set, by a stable spread so tiles differ. */
function coversOf(albums: TileAlbum[], salt: number): string[] {
  if (albums.length <= 3) return albums.map((a) => a.dir)
  const step = Math.max(1, Math.floor(albums.length / 3))
  return [0, 1, 2].map((i) => albums[(salt + i * step) % albums.length].dir)
}

/**
 * The tiles, in order: Albums, Artists, Songs, Hi-Res, each category with two albums or more,
 * then up to `genres` genres with two albums or more that aren't already a category. (Every
 * category is in Library's chips; a one-album one would only crowd the tiles.)
 */
export function yourMusicTiles(
  lib: { albums: TileAlbum[]; artists: { albums: TileAlbum[] }[]; songs: number; hiRes: TileAlbum[]; categories: [string, number][] },
  genres = 6,
): YourMusicTile[] {
  if (!lib.albums.length) return []
  const tiles: YourMusicTile[] = [
    { id: 'albums', title: 'Albums', count: plural(lib.albums.length, 'album', 'albums'), opens: { view: 'albums' }, covers: coversOf(lib.albums, 0) },
    { id: 'artists', title: 'Artists', count: plural(lib.artists.length, 'artist', 'artists'), opens: { view: 'artists' }, covers: coversOf(lib.artists.map((a) => a.albums[0]).filter(Boolean), 1) },
    { id: 'songs', title: 'Songs', count: plural(lib.songs, 'song', 'songs'), opens: { view: 'songs' }, covers: coversOf(lib.albums, 2) },
  ]
  if (lib.hiRes.length) tiles.push({ id: 'hires', title: 'Hi-Res', count: plural(lib.hiRes.length, 'album', 'albums'), opens: { view: 'albums', filter: 'Hi-Res' }, covers: coversOf(lib.hiRes, 0) })
  const taken = new Set(['hi-res'])
  lib.categories.forEach(([c, n], i) => {
    taken.add(c.toLowerCase())
    if (n < 2) return
    const albums = lib.albums.filter((a) => a.category === c)
    tiles.push({ id: `category:${c}`, title: c, count: plural(n, 'album', 'albums'), opens: { view: 'albums', filter: c }, covers: coversOf(albums, i) })
  })
  const byGenre = new Map<string, { name: string; albums: TileAlbum[] }>()
  for (const a of lib.albums)
    for (const g of genresOf(a.genre)) {
      const k = g.toLowerCase()
      if (taken.has(k)) continue
      const e = byGenre.get(k) ?? { name: g, albums: [] }
      e.albums.push(a)
      byGenre.set(k, e)
    }
  ;[...byGenre.values()]
    .filter((g) => g.albums.length >= 2)
    .sort((a, b) => b.albums.length - a.albums.length || a.name.localeCompare(b.name))
    .slice(0, genres)
    .forEach((g, i) =>
      tiles.push({ id: `genre:${g.name.toLowerCase()}`, title: g.name, count: plural(g.albums.length, 'album', 'albums'), opens: { view: 'albums', filter: GENRE_FILTER + g.name }, covers: coversOf(g.albums, i) }),
    )
  return tiles
}

/*
  Where your music meets the catalog's genres: a catalog genre ("R&B & soul") and the words your
  tags and categories use for it ("R&B", "Soul", "Funk"). Exact words only, so Pop never takes
  K-Pop. Moods (Chill, Sad) have no tags to meet.
*/
const SAME_AS: Record<string, string[]> = {
  'bollywood & indian': ['bollywood', 'hindi', 'indian', 'filmi', 'punjabi', 'marathi', 'tamil', 'telugu', 'bengali', 'kannada', 'malayalam', 'desi', 'indian classical', 'ghazal', 'sufi'],
  'hip-hop': ['hip-hop', 'hip hop', 'rap', 'trap'],
  'dance & electronic': ['dance', 'electronic', 'electronica', 'edm', 'house', 'techno', 'trance'],
  'r&b & soul': ['r&b', 'rnb', 'soul', 'funk'],
  'indie & alternative': ['indie', 'alternative', 'alternative rock', 'indie rock'],
  'folk & acoustic': ['folk', 'acoustic', 'singer/songwriter', 'singer-songwriter'],
  'country & americana': ['country', 'americana', 'bluegrass'],
  'soundtracks & musicals': ['soundtrack', 'soundtracks', 'score', 'scores', 'musical', 'ost', 'film & series'],
  'christian & gospel': ['christian', 'gospel', 'worship'],
  'reggae & caribbean': ['reggae', 'caribbean', 'ska', 'dancehall'],
  'mandopop & cantopop': ['mandopop', 'cantopop', 'c-pop'],
  classical: ['classical', 'orchestral', 'opera', 'baroque'],
  metal: ['metal', 'heavy metal'],
  rock: ['rock', 'hard rock', 'classic rock'],
  latin: ['latin', 'reggaeton', 'salsa'],
  'k-pop': ['k-pop', 'kpop'],
  'j-pop': ['j-pop', 'jpop'],
  african: ['african', 'afrobeats', 'afrobeat'],
  brazilian: ['brazilian', 'mpb', 'bossa nova', 'samba'],
}

/** The words your tags may use for a catalog genre (its own name, and its parts, at least). */
export function wordsFor(catalogGenre: string): string[] {
  const k = catalogGenre.trim().toLowerCase()
  const parts = k.split(/\s+(?:&|and)\s+/).filter(Boolean)
  return [...new Set([k, ...parts, ...(SAME_AS[k] ?? [])])]
}

/** Your albums that belong to a catalog genre: by a genre in their tags, or their category. */
export function albumsForGenre<A extends TileAlbum>(albums: A[], catalogGenre: string): A[] {
  const words = new Set(wordsFor(catalogGenre))
  return albums.filter((a) => words.has(a.category.trim().toLowerCase()) || genresOf(a.genre).some((g) => words.has(g.toLowerCase())))
}

/** The catalog genre one of yours (a tag or a category) belongs to, among these tiles. */
export function catalogGenreFor<T extends { title: string }>(tiles: T[], yours: string): T | undefined {
  const k = yours.trim().toLowerCase()
  return tiles.find((t) => wordsFor(t.title).includes(k))
}
