import { askAddSongs } from '../../components/AddToPlaylist'
import { copyLink, ytmMenuWork, type Work } from '../../lib/actions'
import { onlineAlbumPage, onlineArtistPage, onlineCollection, type OnlineAlbum, type OnlineArtist, type OnlineCollection } from '../../lib/engine'
import { radioOfCard } from '../../lib/mixes'
import { notify } from '../../lib/notify'
import { onlineProblem } from '../../lib/online'
import { enqueueItems, onlineEntryFrom, playAllItems, toRemoteItem, type From } from '../../lib/player'
import { listIdOf, playRadioAt } from '../../lib/radio'
import { listRadio } from '../../lib/radioSession'
import { albumLink, artistLink, collectionLink } from '../../lib/shareLinks'
import { artistSongs, fromOnline, onlineItems } from './onlineSearch'

/*
  What an online album, playlist or artist card can do beyond opening and playing it, as YouTube
  Music's own cards do: shuffle, start its radio, add it to a playlist whole, copy its link, and
  Home's Not interested. Each reads the page it needs only when pressed.
*/

/** An online album card's: shuffle, its radio, the whole album to a playlist, its link, Not interested. */
export function onlineAlbumCardWork(root: string, a: OnlineAlbum, from: From): Work {
  const page = () => onlineAlbumPage(a.album_id)
  const link = albumLink(a.album_id)
  return {
    shuffle: () =>
      void page()
        .then((p) => (p.tracks.length ? playAllItems(root, onlineItems(p.tracks), from, 'shuffle') : notify(`“${a.title}” has no songs online.`)))
        .catch((e) => notify(onlineProblem(e))),
    radio: () =>
      void page()
        .then((p) => {
          const r = listRadio(p.radio, p.playlist_id)
          return r ? playRadioAt(root, r, `${a.title} radio`) : notify(`YouTube Music has no radio for “${a.title}”.`)
        })
        .catch((e) => notify(onlineProblem(e))),
    playlist: () =>
      void page()
        .then((p) => p.tracks.length && askAddSongs(p.tracks.map((t) => ({ online: onlineEntryFrom(t) })), a.title, undefined, p.playlist_id ?? undefined))
        .catch((e) => notify(onlineProblem(e))),
    share: link ? () => copyLink(link, a.title) : undefined,
    ...ytmMenuWork(`album:${a.album_id}`, a.title, { feedback: a.feedback }),
  }
}

/** An online playlist or podcast card's: shuffle, its radio, the whole list to a playlist, its
 *  link, Not interested. */
export function collectionCardWork(root: string, c: OnlineCollection, from: From): Work {
  const songs = () => onlineCollection(c.collection_id).then((p) => p.tracks)
  // Your own uploads' pages are no list of YouTube Music's: no radio, no copy, no link.
  const own = c.collection_id.replace(/^VL/, '').startsWith('FEmusic_')
  const list = c.kind === 'podcast' || own ? null : listIdOf(c.collection_id)
  const link = collectionLink(c.collection_id)
  const radio = own ? null : (radioOfCard(c) ?? (list ? listRadio(null, list) : null))
  return {
    shuffle: () =>
      void songs()
        .then((t) => (t.length ? playAllItems(root, t.map(toRemoteItem), from, 'shuffle') : notify('This playlist has no songs to play.')))
        .catch((e) => notify(onlineProblem(e))),
    radio: radio ? () => void playRadioAt(root, radio, `${c.title} radio`) : undefined,
    playlist: () =>
      void songs()
        .then((t) => t.length && askAddSongs(t.map((x) => ({ online: onlineEntryFrom(x) })), c.title, undefined, list ?? undefined))
        .catch((e) => notify(onlineProblem(e))),
    share: link ? () => copyLink(link, c.title) : undefined,
    ...ytmMenuWork(`list:${c.collection_id}`, c.title, { feedback: c.feedback }),
  }
}

/** An online artist card's: shuffle their songs, put them next or last, copy their link. */
export function onlineArtistCardWork(root: string, ar: OnlineArtist): Work {
  const songs = () => onlineArtistPage(ar.artist_id).then((p) => ({ page: p, tracks: artistSongs(p) }))
  const link = artistLink(ar.artist_id)
  const queue = (next: boolean) =>
    void songs()
      .then(({ tracks }) => (tracks.length ? enqueueItems(root, onlineItems(tracks), next) : notify(`No online songs for ${ar.name}.`)))
      .catch((e) => notify(onlineProblem(e)))
  return {
    shuffle: () =>
      void songs()
        .then(({ page, tracks }) =>
          page.shuffle
            ? playRadioAt(root, page.shuffle, `${ar.name} shuffle`)
            : tracks.length
              ? playAllItems(root, onlineItems(tracks), fromOnline(ar.name), 'shuffle')
              : notify(`No online songs for ${ar.name}.`),
        )
        .catch((e) => notify(onlineProblem(e))),
    next: () => queue(true),
    queue: () => queue(false),
    share: link ? () => copyLink(link, ar.name) : undefined,
  }
}
