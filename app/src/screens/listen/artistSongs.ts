import { useEffect, useMemo, useState } from 'react'
import { onlineArtistPage, type OnlineArtistPage, type OnlineShelf } from '../../lib/engine'
import { ownedIndex } from '../../lib/owned'
import { queueKey, type From } from '../../lib/player'
import { rankOnlineTracks } from '../../lib/searchRank'
import type { Library } from './model'
import { findOnlineArtist, itemOf, type MixedSong } from './onlineSearch'
import { useLocalOnly } from '../../lib/localOnly'

/*
  What an online artist page plays and shows, shared by their online page and the online part of
  their page in your library. No components here.
*/

/**
 * Their songs, each once: top songs, then the rest, each ranked for their name (their own
 * recordings lead, not covers), and as yours when you have the song (it plays your file).
 */
export function useArtistSongs(page: OnlineArtistPage | null, name: string, lib: Library) {
  const top = useMemo(() => rankOnlineTracks(name, page?.top_songs ?? []), [name, page])
  const more = useMemo(() => rankOnlineTracks(name, page?.songs ?? []), [name, page])
  const owns = useMemo(() => ownedIndex(lib.tracks), [lib.tracks])
  const songs: MixedSong[] = useMemo(() => {
    const seen = new Set<string>()
    return [...top, ...more].flatMap((t): MixedSong[] => {
      const mine = owns(t)
      const s: MixedSong = mine ? { where: 'library', track: mine } : { where: 'online', track: t }
      const key = queueKey(itemOf(s))
      if (seen.has(key)) return []
      seen.add(key)
      return [s]
    })
  }, [top, more, owns])
  const items = useMemo(() => songs.map(itemOf), [songs])
  const from: From = useMemo(() => ({ kind: 'search', ref: name, label: `Online: ${name}` }), [name])
  return { songs, items, from, topCount: top.length }
}

/** Names that are not one artist, so have no page of their own online. */
const NOT_AN_ARTIST = /^(various artists?|va|unknown artist|soundtrack)$/i

/**
 * The online page of the artist with this name, for their page in your library: found by name,
 * then read. Null until it is (and when offline, or the catalog has no such artist).
 */
export function useOnlineArtistFor(name: string): OnlineArtistPage | null {
  const [found, setFound] = useState<{ name: string; page: OnlineArtistPage } | null>(null)
  const localOnly = useLocalOnly()
  useEffect(() => {
    const wanted = name.trim()
    if (!wanted || NOT_AN_ARTIST.test(wanted) || localOnly) return
    let live = true
    findOnlineArtist(wanted, 'artist-page')
      .then(({ artist }) => (artist ? onlineArtistPage(artist.artist_id) : null))
      .then((page) => {
        if (live && page) setFound({ name, page })
      })
      .catch(() => {
        // Offline, or the catalog has no page: their library page is complete without it.
      })
    return () => {
      live = false
    }
  }, [name, localOnly])
  return found?.name === name && !localOnly ? found.page : null
}

/** Every row of their page; a catalog that sends none gets the rows it does send. */
export function artistShelves(page: OnlineArtistPage | null): OnlineShelf[] {
  if (!page) return []
  if (page.shelves?.length) return page.shelves
  return [
    { title: 'Albums', items: page.albums.map((album) => ({ kind: 'album' as const, album, subtitle: null })), more: null },
    { title: 'Singles & EPs', items: page.singles.map((album) => ({ kind: 'album' as const, album, subtitle: null })), more: null },
    { title: 'Fans might also like', items: page.similar.map((artist) => ({ kind: 'artist' as const, artist, subtitle: null })), more: null },
  ].filter((s) => s.items.length)
}
