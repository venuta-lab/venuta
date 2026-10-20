// What Venuta asks before a change to your YouTube Music account that can't be taken back from
// here, in one plain shape: "Delete “X” on YouTube Music?", what goes and what stays, and that it
// can't be undone from Venuta. Asked, never blocked: the person decides.
//
// Pure (no app imports), so tests/ytmAsk.test.ts reads it as it is. components/Confirm.tsx asks.

export interface AccountAsk {
  title: string
  text: string
  confirm: string
  danger: true
}

const UNDONE = 'This can’t be undone from Venuta.'

export const askDeletePlaylist = (name: string): AccountAsk => ({
  title: `Delete “${name}” on YouTube Music?`,
  text: `The playlist goes from your YouTube Music account, on every device. Songs saved elsewhere stay. ${UNDONE}`,
  confirm: 'Delete playlist',
  danger: true,
})

export const askDeleteUpload = (name: string, kind: 'song' | 'album'): AccountAsk => ({
  title: `Delete “${name}” on YouTube Music?`,
  text: `${kind === 'album' ? 'The uploaded album and its songs go' : 'The uploaded song goes'} from your YouTube Music account, on every device. The file on this computer stays. ${UNDONE}`,
  confirm: 'Delete upload',
  danger: true,
})

export const askRemoveFromHistory = (title: string): AccountAsk => ({
  title: `Remove “${title}” from your YouTube Music history?`,
  text: `This play goes from your history on every device, and your recommendations stop learning from it. ${UNDONE}`,
  confirm: 'Remove',
  danger: true,
})

export const askRemovePastSearch = (words: string): AccountAsk => ({
  title: `Remove “${words}” from your YouTube Music search history?`,
  text: `It stops showing as a past search on every device. ${UNDONE}`,
  confirm: 'Remove',
  danger: true,
})

export const askRemoveCover = (name: string): AccountAsk => ({
  title: `Remove the cover of “${name}” on YouTube Music?`,
  text: `YouTube Music makes one from its songs again. Venuta keeps no copy of the picture. ${UNDONE}`,
  confirm: 'Remove cover',
  danger: true,
})

/** Every ask, for one name (the tests read them side by side). */
export const ACCOUNT_ASKS = (name: string): AccountAsk[] => [
  askDeletePlaylist(name),
  askDeleteUpload(name, 'song'),
  askDeleteUpload(name, 'album'),
  askRemoveFromHistory(name),
  askRemovePastSearch(name),
  askRemoveCover(name),
]

/** One of your uploaded albums, by its page id (YouTube Music gives a privately owned release
 *  its own kind of id, not an album's). */
export const isUploadAlbum = (id: string) => id.replace(/^VL/, '').startsWith('FEmusic_library_privately_owned_release_detail')
