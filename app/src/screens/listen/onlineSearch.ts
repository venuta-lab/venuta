import { useCallback, useEffect, useState } from 'react'
import { isSearchCancelled, onlineAlbumTracks, onlineArtistPage, onlineSearch, onlineSearchTracks, type OnlineArtistPage, type OnlineAlbum, type OnlineResults as Results, type OnlineTrack, type SearchBox } from '../../lib/engine'
import { notify } from '../../lib/notify'
import { OFFLINE, onlineProblem, useNetworkOnline } from '../../lib/online'
import { isCoverClass, rankOnlineResults, rankOnlineTracks } from '../../lib/searchRank'
import { onlineEntryFrom, playItems, toRemoteItem, type From } from '../../lib/player'

/*
  Online catalog search for the window: the search hook every search box uses (ranked once
  when results arrive), and the small helpers the Online catalog section and the Top result
  share. No components here.
*/

const items = (tracks: OnlineTrack[]) => tracks.map(toRemoteItem)
/** Online songs as lines for a playlist (stable ids only). */
export const onlineSongs = (tracks: OnlineTrack[]) => tracks.map((t) => ({ online: onlineEntryFrom(t) }))
const say = onlineProblem

export const fromOnline = (query: string): From => ({ kind: 'search', ref: query, label: `Online: ${query}` })
/** An online album's songs, then `then` with them (a calm notice when it has none). */
export async function withOnlineAlbum(a: OnlineAlbum, then: (tracks: OnlineTrack[]) => void) {
  try {
    const tracks = await onlineAlbumTracks(a.album_id)
    if (!tracks.length) return notify(`“${a.title}” has no songs online.`)
    then(tracks)
  } catch (e) {
    notify(`Couldn't open “${a.title}”: ${say(e)}`)
  }
}

/** An artist page's songs in the order to play them: their top songs, then the rest, each
 *  ranked for the artist's name (so covers and others' uploads never lead their own page). */
export function artistSongs(page: Pick<OnlineArtistPage, 'name' | 'top_songs' | 'songs'>): OnlineTrack[] {
  return [...rankOnlineTracks(page.name, page.top_songs), ...rankOnlineTracks(page.name, page.songs)]
}

/** Play an online artist: the songs of their page (as the page plays them); if the page can't
 *  be read, a ranked search by their name. */
export async function playOnlineArtistPage(root: string, artist: { artist_id: string; name: string }) {
  try {
    const page = await onlineArtistPage(artist.artist_id)
    const list = artistSongs(page)
    if (list.length) return void playItems(root, items(list), 0, fromOnline(page.name || artist.name))
  } catch {
    // The page couldn't be read: their name, searched, is the next best thing.
  }
  return playOnlineArtist(root, artist.name)
}

/** Play an online artist's songs found by a ranked search by their name. */
export async function playOnlineArtist(root: string, name: string) {
  try {
    const found = await onlineSearch(name, 'more-by')
    if (!found.tracks.length) return notify(`No songs by ${name} online.`)
    void playItems(root, items(rankOnlineTracks(name, found.tracks)), 0, fromOnline(name))
  } catch (e) {
    if (!isSearchCancelled(e)) notify(`Couldn't play ${name}: ${say(e)}`)
  }
}

/** The online songs of an answer, split: the recordings people mean (Best) and covers,
 *  karaoke, slowed, loops and uploads. Words that ask for covers keep them all in Best. */
export function splitOnlineSongs(tracks: OnlineTrack[], query: string) {
  return { best: tracks.filter((t) => !isCoverClass(t, query)), covers: tracks.filter((t) => isCoverClass(t, query)) }
}

/** Queue rows for online songs (ids only; the stream resolves at play time). */
export const onlineItems = (tracks: OnlineTrack[]) => items(tracks)

export type OnlineSearchState = {
  /** Words currently requested (the search box). */
  query: string
  /** Which query `results` answered; may lag while loading. */
  resultsFor: string
  results: Results | null
  error: string
  /** True while a request for the current query is outstanding (including debounce). */
  loading: boolean
  /** The computer is offline: nothing was asked, and `error` says so. */
  offline: boolean
  /** More songs are being fetched (the rows already shown stay). */
  loadingMore: boolean
  /** Fetching more failed, in words (the rows already shown stay). */
  moreError: string
}

/** The search state, and asking for more songs for the same words (when there are more). */
export type OnlineSearch = OnlineSearchState & {
  /** More songs exist for these words. */
  canLoadMore: boolean
  loadMore: () => void
}

/** The online search for `query` (asked a moment after typing stops). Keeps prior hits while loading. */
export function useOnlineSearch(query: string, searchBox: SearchBox = 'music'): OnlineSearch {
  const connected = useNetworkOnline()
  const [state, setState] = useState<OnlineSearchState>({
    query: '',
    resultsFor: '',
    results: null,
    error: '',
    loading: false,
    offline: false,
    loadingMore: false,
    moreError: '',
  })
  useEffect(() => {
    const q = query.trim()
    // Nothing typed; or offline, when nothing is asked (asked again by itself once connected).
    if (!q || !connected) {
      const offline = !!q && !connected
      setState({ query: q, resultsFor: q, results: null, error: offline ? OFFLINE : '', loading: false, offline, loadingMore: false, moreError: '' })
      return
    }
    // Mark loading immediately; keep last results (resultsFor) until the new ones land.
    setState((s) => ({ ...s, query: q, loading: true, error: '', offline: false, loadingMore: false, moreError: '' }))
    let live = true
    const t = window.setTimeout(() => {
      onlineSearch(q, searchBox)
        // Ranked once here, for every box that searches through this hook.
        .then(
          (results) =>
            live &&
            setState({ query: q, resultsFor: q, results: rankOnlineResults(q, results), error: '', loading: false, offline: false, loadingMore: false, moreError: '' }),
        )
        // Replaced by a newer search from this box: that one answers instead.
        .catch(
          (e) =>
            live &&
            !isSearchCancelled(e) &&
            setState({ query: q, resultsFor: q, results: null, error: say(e), loading: false, offline: false, loadingMore: false, moreError: '' }),
        )
    }, 350)
    return () => {
      live = false
      window.clearTimeout(t)
    }
  }, [query, connected, searchBox])

  // More songs for the words shown: the next page joins the songs already there, and the whole
  // list is ranked again (so Best and Covers split as one list, and a recording found twice
  // stays once). The rows already shown stay while it loads.
  const cursor = state.results?.tracks_cursor ?? null
  const loadMore = useCallback(() => {
    const q = state.resultsFor
    if (!cursor || !q || state.loadingMore || state.loading) return
    setState((s) => ({ ...s, loadingMore: true, moreError: '' }))
    onlineSearchTracks(q, cursor, searchBox)
      .then((page) =>
        setState((s) => {
          // Other words by now, or already given: this page is for nothing shown.
          if (s.resultsFor !== q || !s.results || s.results.tracks_cursor !== cursor) return s
          const seen = new Set(s.results.tracks.map((t) => `${t.provider}:${t.track_id}`))
          const fresh = page.tracks.filter((t) => !seen.has(`${t.provider}:${t.track_id}`))
          return {
            ...s,
            loadingMore: false,
            results: { ...s.results, tracks: rankOnlineTracks(q, [...s.results.tracks, ...fresh]), tracks_cursor: page.next_cursor },
          }
        }),
      )
      .catch((e) =>
        setState((s) =>
          s.resultsFor !== q ? s : { ...s, loadingMore: false, moreError: isSearchCancelled(e) ? '' : say(e) },
        ),
      )
  }, [cursor, searchBox, state.resultsFor, state.loadingMore, state.loading])

  return { ...state, canLoadMore: !!cursor && !state.loading, loadMore }
}
