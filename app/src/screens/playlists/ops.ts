import { save as saveDialog } from '@tauri-apps/plugin-dialog'
import { confirmAsk } from '../../components/Confirm'
import { playlistsChanged } from '../../components/AddToPlaylist'
import { playlistDelete, playlistDuplicate, playlistExport, type PlaylistSummary, type SmartRule } from '../../lib/engine'
import { notify } from '../../lib/notify'
import { playlistSongs, plural } from './common'
import type { PlaylistsPlace } from '../PlaylistsScreen'

/*
  Looking after a playlist: rename, duplicate, export, delete. The same from every place a
  playlist shows (its card in Playlists, on Home, its own page), so none can be missing one.
  Each says what happened, and tells every open playlist view to read again.
*/

const OPEN = 'venuta:open-playlists'

/** Go to Playlists (a playlist, renaming it, or the list), from anywhere. App listens. */
export function goToPlaylists(place: PlaylistsPlace) {
  window.dispatchEvent(new CustomEvent<PlaylistsPlace>(OPEN, { detail: place }))
}

/** For App: called with the place asked for. Returns how to stop listening. */
export function onGoToPlaylists(go: (place: PlaylistsPlace) => void): () => void {
  const on = (e: Event) => go((e as CustomEvent<PlaylistsPlace>).detail)
  window.addEventListener(OPEN, on)
  return () => window.removeEventListener(OPEN, on)
}

const why = (e: unknown) => (e instanceof Error ? e.message : String(e))

/** A smart playlist's file may not be written yet; reading its songs writes it. */
async function ready(root: string, p: PlaylistSummary, rule?: SmartRule) {
  if (rule) await playlistSongs(root, p.name, rule)
}

export const renamePlaylist = (p: PlaylistSummary) => goToPlaylists({ kind: 'playlist', name: p.name, rename: true })

export async function duplicatePlaylist(root: string, p: PlaylistSummary, rule?: SmartRule) {
  try {
    await ready(root, p, rule)
    const made = await playlistDuplicate(root, p.name)
    playlistsChanged()
    notify(`Made “${made}”, a copy you can change.`, { label: 'Open', run: () => goToPlaylists({ kind: 'playlist', name: made }) })
    return made
  } catch (e) {
    notify(`Couldn't copy ${p.name}: ${why(e)}`)
    return null
  }
}

export async function exportPlaylist(root: string, p: PlaylistSummary, rule?: SmartRule) {
  const to = await saveDialog({ defaultPath: `${p.name}.m3u8`, filters: [{ name: 'Playlist', extensions: ['m3u8'] }] })
  if (!to) return
  try {
    await ready(root, p, rule)
    const n = await playlistExport(root, p.name, to)
    notify(`Saved ${plural(n, 'song')} to ${to}.`)
  } catch (e) {
    notify(`Couldn't export ${p.name}: ${why(e)}`)
  }
}

/** Asks first; the file goes to Set aside beside your music, not destroyed. */
export async function deletePlaylist(root: string, p: Pick<PlaylistSummary, 'name'>): Promise<boolean> {
  const ok = await confirmAsk({
    title: `Delete “${p.name}”?`,
    text: 'The playlist file is moved to the Set aside folder beside your music, not destroyed. Your songs stay where they are.',
    confirm: 'Delete playlist',
    danger: true,
  })
  if (!ok) return false
  try {
    await playlistDelete(root, p.name)
    playlistsChanged()
    notify(`Deleted ${p.name}. Its file is in Set aside.`)
    return true
  } catch (e) {
    notify(`Couldn't delete ${p.name}: ${why(e)}`)
    return false
  }
}
