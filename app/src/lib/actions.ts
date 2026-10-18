import { invoke } from '@tauri-apps/api/core'
import { askAddSongs, askAddToPlaylist } from '../components/AddToPlaylist'
import { actionsFor, LABELS, type ActionId, type Facts, type Kind } from './actionModel'
import { searchWeb, type CatalogTrack, type OnlineTrack, type PlaylistSummary, type SmartRule } from './engine'
import type { MenuGroups, MenuItem } from './menu'
import { notify } from './notify'
import { isLiked, setLiked } from './likes'
import { enqueue, enqueueItems, onlineEntryFrom, play, playAll, playItems, songItemOf, toItem, toRemoteItem, type QueueItem } from './player'
import { albumFrom, type Album, type Artist, type Nav } from '../screens/listen/model'
import { fromWhere, playlistSongs, playPlaylist } from '../screens/playlists/common'
import { deletePlaylist, duplicatePlaylist, exportPlaylist, goToPlaylists, renamePlaylist } from '../screens/playlists/ops'

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
      return [{ label: LABELS[id], run: w, danger: id === 'delete' }]
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

/** A song in your library. `playIt`: play it within the list it is in (from that song on). */
export function songWork(nav: Go, t: CatalogTrack, playIt?: () => void): Work {
  const artists = artistsOf(t)
  return {
    play: playIt ?? (() => void play(nav.root, [t], 0)),
    next: () => void enqueue(nav.root, [t], true),
    queue: () => void enqueue(nav.root, [t], false),
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

/** Put a song's link on the clipboard, and say so. */
export function copySongLink(t: Pick<OnlineTrack, 'track_id' | 'title'>) {
  navigator.clipboard
    .writeText(onlineSongLink(t))
    .then(() => notify(`Link to “${t.title}” copied.`))
    .catch(() => notify("Couldn't reach the clipboard."))
}

/** Like a song, or take it out of Liked songs when it is in. */
export function likeWork(root: string, q: QueueItem): MenuItem[] {
  const liked = isLiked(q)
  return [{ label: liked ? 'Remove from Liked songs' : 'Add to Liked songs', run: () => void setLiked(root, q, !liked) }]
}

/** What online song actions need: the library root, and how to open an artist or an album. */
type GoOnline = Pick<Nav, 'root' | 'openArtistNamed' | 'openOnlineAlbum'>

/** A song from the online catalog. `playIt`: play it within the list it is in (from that song on). */
export function onlineSongWork(nav: GoOnline, t: OnlineTrack, playIt?: () => void): Work {
  const item = toRemoteItem(t)
  const albumId = t.album_id
  return {
    play: playIt ?? (() => void playItems(nav.root, [item], 0)),
    next: () => void enqueueItems(nav.root, [item], true),
    queue: () => void enqueueItems(nav.root, [item], false),
    like: likeWork(nav.root, item),
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
  }
}

export function albumWork(nav: Go, a: Album): Work {
  const from = albumFrom(a)
  return {
    play: () => void playAll(nav.root, a.tracks, from),
    shuffle: () => void playAll(nav.root, a.tracks, from, 'shuffle'),
    next: () => void enqueue(nav.root, a.tracks, true),
    queue: () => void enqueue(nav.root, a.tracks, false),
    playlist: () => askAddToPlaylist(a.tracks.map((t) => t.path), a.title),
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
    playlist: () => askAddToPlaylist(tracks.map((t) => t.path), ar.name),
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
