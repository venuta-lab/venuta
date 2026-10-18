/*
  What can be done with each thing in Venuta, in one place: a song, an album, an artist, a
  playlist, a song a playlist still wants (not in your library yet) and a song in the queue.
  Every right-click, every "…" button and the command palette ask `actionsFor`; lib/actions.ts
  gives each action its work. The groups are drawn with a line between them.

  Pure (no app imports), so tests/actions.test.ts reads it as it is.
*/

export type ActionId =
  | 'play'
  | 'shuffle'
  | 'next'
  | 'queue'
  | 'radio'
  | 'like'
  | 'dislike'
  | 'block'
  | 'shortcut'
  | 'share'
  | 'playlist'
  | 'open'
  | 'artist'
  | 'album'
  | 'moreOnline'
  | 'lyrics'
  | 'details'
  | 'folder'
  | 'web'
  | 'rename'
  | 'duplicate'
  | 'update'
  | 'refind'
  | 'fillOnline'
  | 'rate'
  | 'export'
  | 'use'
  | 'playOnline'
  | 'useOnline'
  | 'find'
  | 'copy'
  | 'remove'
  | 'delete'

export type Kind = 'song' | 'album' | 'artist' | 'playlist' | 'wanted' | 'queued'

/** What is true of the thing, where it matters to what can be done. */
export interface Facts {
  /** A song shown inside a playlist (so it can be taken out of it). */
  inPlaylist?: boolean
  /** A smart playlist, or a song in one: its songs come from its rules. */
  smart?: boolean
  /** The song in Now Playing (its lyrics can be timed there). */
  nowPlaying?: boolean
  /** A song with no artist to go to. */
  noArtist?: boolean
  /** A playlist brought in from somewhere it can be updated from. */
  sourced?: boolean
  /** A playlist with songs still to get. */
  missing?: boolean
  /** A wanted song with a likely match in your library. */
  maybe?: boolean
  /** An online song or album: no file of its own (no folder, details or lyrics timing). */
  online?: boolean
  /** The song playing now, in the queue. */
  current?: boolean
}

/** The words for each action, unless the thing says it better ("Go to A.R. Rahman"). */
export const LABELS: Record<ActionId, string> = {
  play: 'Play',
  shuffle: 'Shuffle',
  next: 'Play next',
  queue: 'Add to queue',
  radio: 'Start radio',
  like: 'Add to Liked songs',
  dislike: 'Dislike',
  block: 'Block artist',
  shortcut: 'Add to Home',
  share: 'Copy link',
  playlist: 'Add to playlist…',
  open: 'Open',
  artist: 'Go to artist',
  album: 'Go to album',
  moreOnline: 'More online by this artist',
  lyrics: 'Time the lyrics',
  details: 'Edit details',
  folder: 'Show in folder',
  web: 'Search the web',
  rename: 'Rename',
  duplicate: 'Duplicate',
  update: 'Update from where it came from…',
  refind: 'Look for missing songs again',
  fillOnline: 'Fill missing online…',
  rate: 'Give unrated songs four stars',
  export: 'Export…',
  use: 'Use the song from your library',
  playOnline: 'Play online…',
  useOnline: 'Use online in this playlist…',
  find: 'Find it in your library…',
  copy: 'Copy its name',
  remove: 'Remove',
  delete: 'Delete…',
}

const PLAY: ActionId[] = ['play', 'shuffle', 'next', 'queue']
const when = (yes: boolean | undefined, ...ids: ActionId[]): ActionId[] => (yes ? ids : [])

/** The actions for a thing, in groups, in the order they are shown. */
export function actionsFor(kind: Kind, f: Facts): ActionId[][] {
  const groups: ActionId[][] = (() => {
    switch (kind) {
      case 'song':
        return [
          ['play', 'next', 'queue'],
          ['radio', 'like', ...when(f.online, 'dislike')],
          ['playlist'],
          [...when(!f.noArtist, 'artist'), 'album', ...when(f.online && !f.noArtist, 'moreOnline', 'block')],
          [
            ...when(f.nowPlaying && !f.online, 'lyrics'),
            ...when(!f.online, 'details', 'folder'),
            ...when(f.online && f.inPlaylist, 'find'),
            ...when(f.online, 'share'),
            'web',
          ],
          when(f.inPlaylist && !f.smart, 'remove'),
        ]
      case 'album':
        return f.online ? [PLAY, ['playlist', 'shortcut'], ['open']] : [PLAY, ['playlist', 'shortcut'], ['artist'], ['details', 'folder']]
      case 'artist':
        return [PLAY, ['radio'], ['playlist', 'shortcut'], ['open']]
      case 'playlist':
        return [
          PLAY,
          ['playlist', 'shortcut'],
          [
            ...when(!f.smart, 'rename'),
            'duplicate',
            ...when(f.sourced && !f.smart, 'update'),
            ...when(f.missing && !f.smart, 'refind', 'fillOnline'),
            'rate',
            'export',
          ],
          when(!f.smart, 'delete'),
        ]
      case 'wanted':
        return [[...when(f.maybe, 'use'), 'playOnline', 'useOnline', 'find', 'copy', 'web'], when(!f.smart, 'remove')]
      case 'queued':
        return [
          ['play'],
          ['playlist'],
          [...when(!f.noArtist, 'artist'), ...when(!f.online, 'album')],
          when(!f.current, 'remove'),
        ]
    }
  })()
  return groups.filter((g) => g.length)
}
