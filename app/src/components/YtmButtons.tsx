import { useEffect, useState } from 'react'
import { ytmAlbumSaved, ytmArtistFollowed, ytmSave, ytmSubscribe, type OnlineSuggestion } from '../lib/engine'
import { notify } from '../lib/notify'
import { onlineProblem } from '../lib/online'
import { useYtmPart, useYtmStatus, ytmLibraryChanged } from '../lib/ytmLibrary'

/*
  One press on your YouTube Music account, on the page of the thing it's about: save an album or a
  playlist to your library, follow an artist. Shown only when you're signed in; each press is
  the whole change, said back when YouTube doesn't take it.
*/

function Toggle(props: { on: boolean; busy: boolean; onLabel: string; offLabel: string; title: string; onClick: () => void }) {
  return (
    <button onClick={props.onClick} disabled={props.busy} aria-pressed={props.on} title={props.title} className={`${props.on ? 'btn' : 'btn-ghost'} btn-lg`}>
      <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
        {props.on ? <path d="m5 12.5 4.5 4.5L19 7.5" /> : <path d="M12 5v14M5 12h14" />}
      </svg>
      {props.on ? props.onLabel : props.offLabel}
    </button>
  )
}

/** Save an album to your YouTube Music library (or take it out). */
export function SaveAlbumButton({ albumId }: { albumId: string }) {
  const { signed_in } = useYtmStatus()
  const [state, setState] = useState<{ playlist_id: string; saved: boolean } | null>(null)
  const [busy, setBusy] = useState(false)
  useEffect(() => {
    if (!signed_in) return
    let live = true
    ytmAlbumSaved(albumId)
      .then((s) => live && setState(s))
      .catch(() => {})
    return () => {
      live = false
    }
  }, [albumId, signed_in])
  if (!signed_in || !state) return null
  const press = async () => {
    setBusy(true)
    try {
      await ytmSave(state.playlist_id, !state.saved)
      setState({ ...state, saved: !state.saved })
      ytmLibraryChanged(['albums'])
      notify(state.saved ? 'Taken out of your YouTube Music library.' : 'Saved to your YouTube Music library.')
    } catch (e) {
      notify(onlineProblem(e))
    } finally {
      setBusy(false)
    }
  }
  return <Toggle on={state.saved} busy={busy} onLabel="Saved" offLabel="Save" title="In your YouTube Music library" onClick={() => void press()} />
}

/** Save someone's playlist to your YouTube Music library (or take it out). Not for your own. */
export function SavePlaylistButton({ playlistId }: { playlistId: string }) {
  const { signed_in } = useYtmStatus()
  const { value } = useYtmPart<OnlineSuggestion[]>('playlists')
  const [busy, setBusy] = useState(false)
  const bare = playlistId.replace(/^VL/, '')
  const saved = (value ?? []).some((c) => c.kind === 'playlist' && c.collection.collection_id.replace(/^VL/, '') === bare)
  if (!signed_in || value === null) return null
  const press = async () => {
    setBusy(true)
    try {
      await ytmSave(bare, !saved)
      ytmLibraryChanged(['playlists'])
      notify(saved ? 'Taken out of your YouTube Music library.' : 'Saved to your YouTube Music library.')
    } catch (e) {
      notify(onlineProblem(e))
    } finally {
      setBusy(false)
    }
  }
  return <Toggle on={saved} busy={busy} onLabel="Saved" offLabel="Save" title="In your YouTube Music library" onClick={() => void press()} />
}

/** Follow an artist on YouTube Music (or stop). */
export function FollowButton({ artistId }: { artistId: string }) {
  const { signed_in } = useYtmStatus()
  const [state, setState] = useState<{ channel_id: string; subscribed: boolean } | null>(null)
  const [busy, setBusy] = useState(false)
  useEffect(() => {
    if (!signed_in) return
    let live = true
    ytmArtistFollowed(artistId)
      .then((s) => live && setState(s))
      .catch(() => {})
    return () => {
      live = false
    }
  }, [artistId, signed_in])
  if (!signed_in || !state) return null
  const press = async () => {
    setBusy(true)
    try {
      await ytmSubscribe(state.channel_id, !state.subscribed)
      setState({ ...state, subscribed: !state.subscribed })
      ytmLibraryChanged(['subscriptions'])
      notify(state.subscribed ? 'You no longer follow them on YouTube Music.' : 'Following on YouTube Music.')
    } catch (e) {
      notify(onlineProblem(e))
    } finally {
      setBusy(false)
    }
  }
  return <Toggle on={state.subscribed} busy={busy} onLabel="Following" offLabel="Follow" title="Follow on YouTube Music" onClick={() => void press()} />
}
