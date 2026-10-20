import { invoke } from '@tauri-apps/api/core'
import { askAddSongs, askAddToPlaylist } from '../components/AddToPlaylist'
import { actionsFor, LABELS, type ActionId, type Facts, type Kind } from './actionModel'
import { searchWeb, ytmDeleteUpload, ytmMenuPress, ytmPlaylistEdit, ytmRemovePastSearch, ytmSongMenu, type CatalogTrack, type OnlineTrack, type PlaylistSummary, type SmartRule } from './engine'
import { rebuildMenu, type MenuGroups, type MenuItem } from './menu'
import { notify } from './notify'
import { isLiked, setLiked } from './likes'
import { blockArtist, dislike, playWithRadio, startRadio } from './radio'
import { shortcutWork } from './shortcuts'
import { enqueue, enqueueItems, onlineEntryFrom, playAll, songItemOf, toItem, toRemoteItem, type QueueItem } from './player'
import { albumFrom, type Album, type Artist, type Nav } from '../screens/listen/model'
import { fromWhere, playlistSongs, playPlaylist } from '../screens/playlists/common'
import { deletePlaylist, duplicatePlaylist, exportPlaylist, goToPlaylists, renamePlaylist } from '../screens/playlists/ops'
import { confirmAsk } from '../components/Confirm'
import { onlineProblem } from './online'
import { askDeleteUpload, askRemovePastSearch } from './ytmAsk'
import { ytmLibraryChanged, ytmReady } from './ytmLibrary'
import { songMenuItems, songMenus, type Press, type SongMenuItem, type TrackMenu } from './ytmSongMenu'

/*
  What each action does, for one thing. lib/actionModel.ts says which actions a thing has and in
  what order; this gives them their work. A screen asks `menuFor(kind, facts, work)`, usually
  with the work built here (songWork, albumWork…) plus anything only it can do (remove from
  this playlist). An action with no work is left out, so a menu never offers what can't be done.
*/

/** The work for each action: one thing to do, or several items (Go to each of the artists). */
export type Work = Partial<Record<ActionId, (() => void) | MenuItem[]>>

export function menuFor(kind: Kind, facts: Facts, work: Work): MenuGroups {
  return actionsFor(kind, facts).map((group) =>
    group.flatMap((id): MenuItem[] => {
      const w = work[id]
      if (!w) return []
      if (Array.isArray(w)) return w
      return [{ label: LABELS[id], run: w, danger: id === 'delete' || id === 'deleteUpload' }]
    }),
  )
}

export const showInFolder = (root: string, path: string) =>
  void invoke('show_in_folder', { root, path }).catch((e) => notify(String(e)))

/** "Go to A.R. Rahman", one for each of the first three names. */
export const goToEach = (names: string[], open: (name: string) => void): MenuItem[] =>
  [...new Set(names.filter(Boolean))].slice(0, 3).map((n) => ({ label: `Go to ${n}`, run: () => open(n) }))

/** What the actions need to go places: the library, and how to open an album, an artist or the editor. */
type Go = Pick<Nav, 'root' | 'openAlbum' | 'openArtist' | 'editDetails'>

const artistsOf = (t: CatalogTrack) => (t.artists.length ? t.artists : t.album_artist ? [t.album_artist] : [])

/** A song in your library. `playIt`: play it within the list it is in (from that song on).
 *  Played on its own, it plays as YouTube Music plays one song: it, then (Autoplay on, and only
 *  when it is the same song there) its YouTube Music radio. */
export function songWork(nav: Go, t: CatalogTrack, playIt?: () => void): Work {
  const artists = artistsOf(t)
  return {
    play: playIt ?? (() => void playWithRadio(nav.root, toItem(t))),
    next: () => void enqueue(nav.root, [t], true),
    queue: () => void enqueue(nav.root, [t], false),
    radio: () => void startRadio(nav.root, toItem(t)),
    like: likeWork(nav.root, toItem(t)),
    playlist: () => askAddToPlaylist([t.path], t.title),
    artist: goToEach(artists, nav.openArtist),
    album: () => nav.openAlbum(t.dir),
    details: () => nav.editDetails(t.dir),
    folder: () => showInFolder(nav.root, t.path),
    web: () => void searchWeb(`${t.title} ${artists[0] ?? ''}`.trim()),
  }
}
export const songFacts = (t: CatalogTrack): Facts => ({ noArtist: !artistsOf(t).length })

/** The catalog's own page for an online song, to share. */
export const onlineSongLink = (t: Pick<OnlineTrack, 'track_id'>) => `https://music.youtube.com/watch?v=${encodeURIComponent(t.track_id)}`

/** Put a link on the clipboard, and say so. */
export function copyLink(url: string, name: string) {
  navigator.clipboard
    .writeText(url)
    .then(() => notify(`Link to “${name}” copied.`))
    .catch(() => notify("Couldn't reach the clipboard."))
}

/** Put a song's link on the clipboard, and say so. */
export const copySongLink = (t: Pick<OnlineTrack, 'track_id' | 'title'>) => copyLink(onlineSongLink(t), t.title)

/** Like a song, or take it out of Liked songs when it is in. */
export function likeWork(root: string, q: QueueItem): MenuItem[] {
  const liked = isLiked(q)
  return [{ label: liked ? 'Remove from Liked songs' : 'Add to Liked songs', run: () => void setLiked(root, q, !liked) }]
}

/** What a press on a song's own YouTube Music menu did, in words. */
function pressedWords(i: SongMenuItem, title: string): string {
  switch (i.press) {
    case 'library':
      return i.next ? `Saved “${title}” to your YouTube Music library.` : `Took “${title}” out of your YouTube Music library.`
    case 'pin':
      return i.next ? `Pinned “${title}” to Listen again.` : `Unpinned “${title}” from Listen again.`
    case 'feedback':
      return `Told YouTube Music “${i.label}” for “${title}”.`
  }
}

async function pressSongMenu(key: string, title: string, i: SongMenuItem) {
  try {
    await ytmMenuPress(i.token, i.press)
    if (i.press === 'feedback') songMenus.sent(key, i.token)
    else songMenus.pressed(key, i.press, i.next)
    if (i.press === 'library') ytmLibraryChanged(['songs'])
    notify(pressedWords(i, title))
  } catch (e) {
    notify(onlineProblem(e))
  }
}

/** A catalog song's queue key, and its id. */
const SONG_KEY = /^online:remote_catalog:(.+)$/
/** Songs whose own menu is being read, or was read and YouTube Music gave none. */
const lazyMenus = new Map<string, 'reading' | 'none'>()

/** One song's menu, read as the account by its id (only when its menu shows), then the menu
 *  built again with what it offers. */
function readSongMenu(key: string, id: string) {
  if (lazyMenus.has(key)) return
  lazyMenus.set(key, 'reading')
  ytmSongMenu(id)
    .then((m) => {
      if (m) {
        songMenus.remember(key, m)
        lazyMenus.delete(key)
      } else lazyMenus.set(key, 'none')
    })
    .catch(() => lazyMenus.set(key, 'none'))
    .finally(rebuildMenu)
}

/** An online song's own YouTube Music menu (key: its queue key; `came`: the menu it was listed
 *  with here): Save to that library or Remove from it, Pin to Listen again or Unpin, Not
 *  interested. Only what its listing carried; each press sends that one token. */
export function ytmMenuWork(key: string, title: string, came?: TrackMenu | null): Work {
  const known = songMenus.menuOf(key, came)
  // Listed with no menu (a search sent signed out): read when the menu shows, by its id alone.
  const id = SONG_KEY.exec(key)?.[1]
  if (!known && id && ytmReady() && lazyMenus.get(key) !== 'none') return { ytmLibrary: [{ label: 'Reading YouTube Music options…', disabled: true, run: () => {}, load: () => readSongMenu(key, id) }] }
  const items = songMenuItems(known)
  const of = (press: Press): MenuItem[] | undefined => {
    const list = items.filter((i) => i.press === press).map((i) => ({ label: i.label, run: () => void pressSongMenu(key, title, i) }))
    return list.length ? list : undefined
  }
  return { ytmLibrary: of('library'), ytmPin: of('pin'), notInterested: of('feedback') }
}

/** Delete one of your uploads on YouTube Music, a song or an album, after asking. True when it
 *  went: the lists that held it are read again. */
export async function deleteYtmUpload(entityId: string, name: string, kind: 'song' | 'album'): Promise<boolean> {
  if (!(await confirmAsk(askDeleteUpload(name, kind)))) return false
  try {
    await ytmDeleteUpload(entityId)
    ytmLibraryChanged(['upload_songs', 'upload_albums'])
    notify(`Deleted “${name}” on YouTube Music.`)
    return true
  } catch (e) {
    notify(onlineProblem(e))
    return false
  }
}

/** Take one of your past searches out of your YouTube Music search history, after asking. True
 *  when it went. */
export async function removePastSearch(text: string, token: string): Promise<boolean> {
  if (!(await confirmAsk(askRemovePastSearch(text)))) return false
  try {
    await ytmRemovePastSearch(token)
    notify(`Took “${text}” out of your YouTube Music search history.`)
    return true
  } catch (e) {
    notify(onlineProblem(e))
    return false
  }
}

/** YouTube Music's own list of podcast episodes saved for later. */
export const EPISODES_FOR_LATER = 'SE'

/** An online song that is a podcast episode. */
export const isEpisode = (t: Pick<OnlineTrack, 'video_type'>) => t.video_type === 'PODCAST_EPISODE'

/** Save a podcast episode to your Episodes for later on YouTube Music. */
export async function saveEpisodeForLater(t: Pick<OnlineTrack, 'track_id' | 'title'>) {
  try {
    await ytmPlaylistEdit(EPISODES_FOR_LATER, [{ kind: 'add', video_ids: [t.track_id] }])
    ytmLibraryChanged(['podcasts'])
    notify(`Saved “${t.title}” to Episodes for later.`)
  } catch (e) {
    const said = onlineProblem(e)
    notify(said === 'Already in the playlist.' ? `“${t.title}” is in Episodes for later already.` : said)
  }
}

/** What online song actions need: the library root, and how to open an artist or an album. */
type GoOnline = Pick<Nav, 'root' | 'openArtistNamed' | 'openOnlineAlbum'>

/** A song from the online catalog. `playIt`: play it within the list it is in (from that song on). */
export function onlineSongWork(nav: GoOnline, t: OnlineTrack, playIt?: () => void): Work {
  const item = toRemoteItem(t)
  const albumId = t.album_id
  return {
    play: playIt ?? (() => void playWithRadio(nav.root, item)),
    next: () => void enqueueItems(nav.root, [item], true),
    queue: () => void enqueueItems(nav.root, [item], false),
    radio: () => void startRadio(nav.root, item),
    like: likeWork(nav.root, item),
    dislike: () => void dislike(nav.root, item),
    block: t.artists.slice(0, 3).map((name) => ({ label: `Block ${name}`, run: () => void blockArtist(name) })),
    playlist: () => askAddSongs([{ online: onlineEntryFrom(t) }], t.title),
    artist: goToEach(t.artists, nav.openArtistNamed),
    album: albumId
      ? () =>
          nav.openOnlineAlbum({
            provider: t.provider,
            album_id: albumId,
            title: t.album ?? t.title,
            artists: t.artists,
            track_count: null,
            release_date: null,
            artwork_url: t.artwork_url,
          })
      : undefined,
    share: () => copySongLink(t),
    web: () => void searchWeb(`${t.title} ${t.artists[0] ?? ''}`.trim()),
    saveEpisode: isEpisode(t) && ytmReady() ? () => void saveEpisodeForLater(t) : undefined,
    ...ytmMenuWork(`online:${t.provider}:${t.track_id}`, t.title, t.menu),
  }
}

/** What is true of an online song, for its menu. */
export const onlineSongFacts = (t: Pick<OnlineTrack, 'artists' | 'video_type'>): Facts => ({ online: true, noArtist: !t.artists.length, episode: isEpisode(t) })

export function albumWork(nav: Go, a: Album): Work {
  const from = albumFrom(a)
  return {
    play: () => void playAll(nav.root, a.tracks, from),
    shuffle: () => void playAll(nav.root, a.tracks, from, 'shuffle'),
    next: () => void enqueue(nav.root, a.tracks, true),
    queue: () => void enqueue(nav.root, a.tracks, false),
    playlist: () => askAddToPlaylist(a.tracks.map((t) => t.path), a.title),
    shortcut: shortcutWork({ kind: 'album', dir: a.dir, title: a.title }, `“${a.title}”`),
    artist: goToEach([a.artist], nav.openArtist),
    details: () => nav.editDetails(a.dir),
    folder: a.tracks[0] ? () => showInFolder(nav.root, a.tracks[0].path) : undefined,
  }
}

/** All of an artist's songs: their own albums, newest first. */
export function artistWork(nav: Go, ar: Artist): Work {
  const tracks = ar.albums.flatMap((a) => a.tracks)
  const from = { kind: 'artist' as const, ref: ar.name, label: ar.name }
  return {
    play: () => void playAll(nav.root, tracks, from),
    shuffle: () => void playAll(nav.root, tracks, from, 'shuffle'),
    next: () => void enqueue(nav.root, tracks, true),
    queue: () => void enqueue(nav.root, tracks, false),
    radio: tracks[0] ? () => void startRadio(nav.root, toItem(tracks[0])) : undefined,
    playlist: () => askAddToPlaylist(tracks.map((t) => t.path), ar.name),
    shortcut: shortcutWork({ kind: 'artist', name: ar.name }, ar.name),
    open: () => nav.openArtist(ar.name),
  }
}

/** A playlist, from any card or its page. `rule`: a smart playlist's rules (its songs come from them). */
export function playlistWork(root: string, p: PlaylistSummary, rule?: SmartRule): Work {
  return {
    play: () => void playPlaylist(root, p, 'play', rule),
    shuffle: () => void playPlaylist(root, p, 'shuffle', rule),
    next: () => void playPlaylist(root, p, 'next', rule),
    queue: () => void playPlaylist(root, p, 'queue', rule),
    playlist: [
      {
        label: 'Add all to another playlist…',
        run: () => void playlistSongs(root, p.name, rule).then((songs) => askAddSongs(songs.map(songItemOf), p.name)),
      },
    ],
    shortcut: shortcutWork({ kind: 'playlist', name: p.name }, `“${p.name}”`),
    rename: () => renamePlaylist(p),
    duplicate: p.smart
      ? [{ label: 'Duplicate as an ordinary playlist', run: () => void duplicatePlaylist(root, p, rule) }]
      : () => void duplicatePlaylist(root, p, rule),
    update: p.source_id
      ? [{ label: `Update from ${fromWhere(p.source ?? '') || 'where it came from'}…`, run: () => goToPlaylists({ kind: 'import', start: { kind: 'update', name: p.name } }) }]
      : undefined,
    export: () => void exportPlaylist(root, p, rule),
    delete: () => void deletePlaylist(root, p),
  }
}
export const playlistFacts = (p: PlaylistSummary): Facts => ({ smart: p.smart, sourced: !!p.source_id, missing: p.missing > 0 })
