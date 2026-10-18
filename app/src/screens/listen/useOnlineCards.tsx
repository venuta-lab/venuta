import { useMemo } from 'react'
import type { OnlineSuggestion } from '../../lib/engine'
import { current, playItems, queueKey, toRemoteItem, usePlayerValue, type From } from '../../lib/player'
import { useNav } from './model'
import { CollectionCard, OnlineSongCard } from './OnlineCards'
import { isVideo } from './onlineSearch'
import { OnlineAlbumTile, OnlineArtistCard } from './SearchAll'

/*
  The cards of an online row or grid, each the kind it is (screens/listen/OnlineCards.tsx draws
  them). Its own file so the card files hold components only.
*/

const keyOfCard = (c: OnlineSuggestion): string => {
  switch (c.kind) {
    case 'song':
      return `song:${c.track.track_id}`
    case 'album':
      return `album:${c.album.album_id}`
    case 'artist':
      return `artist:${c.artist.artist_id}`
    case 'playlist':
      return `list:${c.collection.collection_id}`
  }
}

/**
 * The cards of an online row, each the kind it is: albums, artists and playlists open; songs and
 * videos play the row's songs from that one. `label` names the page for "Playing from".
 */
export function useOnlineCards(items: OnlineSuggestion[], label: string, width?: number) {
  const { root } = useNav()
  const songs = useMemo(() => items.flatMap((c) => (c.kind === 'song' ? [c.track] : [])), [items])
  const queue = useMemo(() => songs.map(toRemoteItem), [songs])
  // A row of videos draws its other cards wide too, so the row reads as one.
  const videoRow = songs.length > 0 && songs.every(isVideo)
  const nowKey = usePlayerValue((st) => {
    const c = current(st)
    return c ? queueKey(c) : ''
  })
  const from: From = { kind: 'search', ref: label, label: `Online: ${label}` }
  return items.map((c) => {
    const key = keyOfCard(c)
    let node: React.ReactNode
    switch (c.kind) {
      case 'song': {
        const at = songs.indexOf(c.track)
        node = <OnlineSongCard track={c.track} width={width} now={queueKey(queue[at]) === nowKey} onPlay={() => void playItems(root, queue, at, from)} />
        break
      }
      case 'album':
        node = <OnlineAlbumTile album={c.album} query={label} />
        break
      case 'artist':
        node = <OnlineArtistCard artist={c.artist} />
        break
      case 'playlist':
        node = <CollectionCard collection={c.collection} width={width} wide={videoRow} />
        break
    }
    return { key, node }
  })
}

