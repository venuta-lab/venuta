import { invoke } from '@tauri-apps/api/core'
import { catalogNow } from './catalog'
import { onlineSearch, ytmJoin, type OnlineTrack } from './engine'
import { notify } from './notify'
import { ytmLibraryChanged, ytmReady } from './ytmLibrary'
import { confirmAsk } from '../components/Confirm'
import { ownedIndex, titleCore } from './owned'
import { startRadio } from './radio'
import { toItem, toRemoteItem } from './player'
import { searchKey } from './textsearch'
import { searchMusic, showMusicPage } from '../screens/ListenScreen'
import { goToPlaylists } from '../screens/playlists/ops'
import { findOnlineArtist } from '../screens/listen/onlineSearch'

/*
  Open a pasted link: a song, album, artist or playlist on YouTube Music or Spotify. A song
  plays (and its radio after it); a page opens in Music. Yours first: a Spotify song you have
  plays from your file, an album you have opens yours. A playlist on another service is brought
  in through Import, as Playlists does.
*/

interface LinkOpen {
  target:
    | { kind: 'song' | 'album' | 'artist' | 'playlist' | 'spotify_song' | 'spotify_album' | 'spotify_artist'; id: string }
    | { kind: 'import'; link: string }
  track: OnlineTrack | null
  name: string | null
  artists: string[]
  duration_ms: number | null
}

/** Ask the window to show Open link (from the palette or a button). */
export const askOpenLink = () => window.dispatchEvent(new Event('venuta:open-link'))

/** Where opening the link went: 'music' (a page there, or a song playing), 'playlists'. */
export type Opened = 'music' | 'playlists'

export async function openLink(root: string, link: string): Promise<Opened> {
  // A collaborative playlist's invite: joined when you say so (else it opens to look at).
  if (/[?&]jct=/.test(link) && /youtube\.com/i.test(link)) {
    if (!ytmReady()) notify('Sign in to YouTube Music (Settings, Online) to join a shared playlist. It opens here to look at.')
    else {
      const ok = await confirmAsk({
        title: 'Join this shared playlist?',
        text: 'It joins your YouTube Music library, and you can add songs to it. Each person can take out only what they added.',
        confirm: 'Join',
      })
      if (ok) {
        const id = await ytmJoin(link)
        ytmLibraryChanged(['playlists'])
        notify('Joined. It’s in your playlists.')
        showMusicPage({ kind: 'online-collection', collection: { provider: 'remote_catalog', collection_id: id, title: '', subtitle: null, artwork_url: null, kind: 'playlist' } })
        return 'music'
      }
    }
  }
  const r = await invoke<LinkOpen>('open_link', { link })
  const t = r.target
  const lead = r.artists[0] ?? ''
  switch (t.kind) {
    case 'import':
      goToPlaylists({ kind: 'import', start: { kind: 'link', link: t.link } })
      return 'playlists'
    case 'song':
      if (r.track) await startRadio(root, toRemoteItem(r.track))
      return 'music'
    case 'album':
      showMusicPage({ kind: 'online-album', album: { provider: 'remote_catalog', album_id: t.id, title: '', artists: [], track_count: null, release_date: null, artwork_url: null } })
      return 'music'
    case 'artist':
      showMusicPage({ kind: 'online-artist', artist: { artist_id: t.id, name: '' } })
      return 'music'
    case 'playlist':
      showMusicPage({ kind: 'online-collection', collection: { provider: 'remote_catalog', collection_id: t.id, title: '', subtitle: null, artwork_url: null, kind: 'playlist' } })
      return 'music'
    case 'spotify_song': {
      const song = { title: r.name ?? '', artists: r.artists, duration_ms: r.duration_ms }
      const mine = catalogNow(root)
      const yours = mine ? ownedIndex(mine)(song) : undefined
      if (yours) {
        await startRadio(root, toItem(yours))
        return 'music'
      }
      const found = await onlineSearch(`${song.title} ${lead}`.trim(), 'pick')
      const same = ownedIndex([{ ...song }])
      const best = found.tracks.find((x) => same(x)) ?? found.tracks[0]
      if (!best) {
        searchMusic(`${song.title} ${lead}`.trim())
        return 'music'
      }
      await startRadio(root, toRemoteItem(best))
      return 'music'
    }
    case 'spotify_album': {
      const name = r.name ?? ''
      const mine = catalogNow(root)
      const key = titleCore(name)
      const yours = mine?.find((x) => titleCore(x.album) === key && (!lead || searchKey(x.album_artist || x.artists[0] || '') === searchKey(lead)))
      if (yours) {
        showMusicPage({ kind: 'album', dir: yours.dir })
        return 'music'
      }
      const found = await onlineSearch(`${name} ${lead}`.trim(), 'pick')
      const album = found.albums.find((a) => titleCore(a.title) === key) ?? found.albums[0]
      if (album) showMusicPage({ kind: 'online-album', album })
      else searchMusic(`${name} ${lead}`.trim())
      return 'music'
    }
    case 'spotify_artist': {
      const name = r.name ?? ''
      const mine = catalogNow(root)
      if (mine?.some((x) => searchKey(x.album_artist || x.artists[0] || '') === searchKey(name))) {
        showMusicPage({ kind: 'artist', name })
        return 'music'
      }
      const { artist } = await findOnlineArtist(name, 'pick')
      if (artist) showMusicPage({ kind: 'online-artist', artist })
      else searchMusic(name)
      return 'music'
    }
  }
}
