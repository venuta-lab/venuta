// Your YouTube Music choices in words: each switch, safe for a new account, in the four groups
// the sign-in screen and Settings, Online show; and what is on, said the way it really is.

/** What Venuta does with one account and channel (crates/engine settings.rs `YtmChoices`). */
export interface YtmChoices {
  /** Recommendations for me: Home, radio, Up next and mixes asked for as the account. */
  personal: boolean
  /** My library in Venuta: the account's library shows in Library. */
  library: boolean
  /** Count plays: an online song heard 30 seconds is told as played (the account's history). */
  count_plays: boolean
  /** How much I listened: with Count plays, the seconds of each online song heard too. */
  listening_time: boolean
  /** Likes and dislikes: the same on the account. */
  likes: boolean
  /** Signed-in search: searches and suggestions go as the account. */
  signed_search: boolean
  /** New YouTube likes come in, when Venuta starts. */
  new_likes_in: boolean
  /** Linked playlists stay in step on their own; off, only when you press Sync. */
  links_in_step: boolean
}

/** How it plays: kept for everyone, not per account (`Listening`). */
export interface PlayChoices {
  autoplay: boolean
  play_my_copy: boolean
  hide_videos: boolean
  hide_blocked: boolean
}

/** Just the "How it plays" choices out of everything kept for listening. */
export const playChoicesOf = (l: PlayChoices): PlayChoices => ({
  autoplay: l.autoplay,
  play_my_copy: l.play_my_copy,
  hide_videos: l.hide_videos,
  hide_blocked: l.hide_blocked,
})

/** Safe: what only reads is on, because that is why you sign in; what writes to your account or
 *  your history is off until you turn it on. */
export const SAFE: YtmChoices = {
  personal: true,
  library: true,
  count_plays: false,
  listening_time: false,
  likes: false,
  signed_search: false,
  new_likes_in: false,
  links_in_step: true,
}

/** The same choices, with one that needs another off without it. */
export const settle = (c: YtmChoices): YtmChoices => ({ ...c, listening_time: c.listening_time && c.count_plays })

/** Every switch about the account at once: all on ("Turn everything on") or all off
 *  ("Everything off"). How it plays is not about the account and stays as it is. */
export const everything = (on: boolean): YtmChoices => ({
  personal: on,
  library: on,
  count_plays: on,
  listening_time: on,
  likes: on,
  signed_search: on,
  new_likes_in: on,
  links_in_step: on,
})

/** Every account switch stands `on` already (that quick button has nothing to do). */
export const allAre = (c: YtmChoices, on: boolean): boolean => (Object.keys(SAFE) as (keyof YtmChoices)[]).every((k) => c[k] === on)

export type Row =
  | { kind: 'account'; key: keyof YtmChoices; name: string; line: string; needs?: keyof YtmChoices }
  | { kind: 'play'; key: keyof PlayChoices; name: string; line: string }
  | { kind: 'action'; key: 'bring_in'; name: string; line: string }

export interface Group {
  title: string
  rows: Row[]
}

export const GROUPS: Group[] = [
  {
    title: 'What Venuta reads from your account',
    rows: [
      {
        kind: 'account',
        key: 'personal',
        name: 'Recommendations for me',
        line: 'Home, radio, Up next and mixes come from your account, personal to you. Off: YouTube’s general ones, as when signed out.',
      },
      {
        kind: 'account',
        key: 'library',
        name: 'My library in Venuta',
        line: 'Your playlists, artists, albums, songs, uploads and podcasts show in Library, marked ONLINE.',
      },
    ],
  },
  {
    title: 'What Venuta tells YouTube Music',
    rows: [
      {
        kind: 'account',
        key: 'count_plays',
        name: 'Count plays',
        line: 'After 30 seconds of an online song, Venuta tells YouTube Music you played it, so it shows in your YouTube Music history and recommendations learn from it. If Venuta can’t pass YouTube’s security check, it tells YouTube through its web client instead, and the play shows in your YouTube watch history. Your files are never sent.',
      },
      {
        kind: 'account',
        key: 'listening_time',
        needs: 'count_plays',
        name: 'How much I listened',
        line: 'With Count plays: also how many seconds of each online song you heard, sent as you listen and when it ends or you skip it, as YouTube’s app does. Needs Count plays.',
      },
      {
        kind: 'account',
        key: 'likes',
        name: 'Likes and dislikes',
        line: 'A like, unlike or thumbs down on an online song here does the same on your account, from now on. Unliking here takes it out of Liked Music there too.',
      },
      {
        kind: 'account',
        key: 'signed_search',
        name: 'Signed-in search',
        line: 'Searches use your account: results get personal and teach recommendations, and they appear in your YouTube search history.',
      },
    ],
  },
  {
    title: 'What Venuta keeps in step',
    rows: [
      { kind: 'action', key: 'bring_in', name: 'Bring my Liked Music in', line: 'Once, now: the songs you liked on YouTube Music join Liked songs here.' },
      {
        kind: 'account',
        key: 'new_likes_in',
        name: 'New YouTube likes come in',
        line: 'Songs you like on your phone join Liked songs here when you open Venuta. Liked videos are left out, and nothing is taken out.',
      },
      {
        kind: 'account',
        key: 'links_in_step',
        name: 'Linked playlists stay in step',
        line: 'A playlist you linked syncs when you open it, after you change it, and at most every 30 minutes. Off: only when you press Sync.',
      },
    ],
  },
  {
    title: 'How it plays',
    rows: [
      { kind: 'play', key: 'autoplay', name: 'Autoplay', line: 'When a list ends, keep playing similar songs from YouTube Music.' },
      { kind: 'play', key: 'play_my_copy', name: 'Play my copy when I have it', line: 'Hybrid: your file in place of the same online song.' },
      { kind: 'play', key: 'hide_videos', name: 'Songs, not music videos', line: 'Music videos are left out of search and of what plays next.' },
      {
        kind: 'play',
        key: 'hide_blocked',
        name: 'Hide artists I blocked',
        line: 'Artists you blocked stay out of radio, autoplay and Home. Off: they can come again; your list is kept.',
      },
    ],
  },
]

/** A row's switch: on or off, and whether it can be changed now. */
export function rowOn(row: Row, c: YtmChoices, plays: PlayChoices): { on: boolean; disabled: boolean } {
  if (row.kind === 'action') return { on: false, disabled: false }
  if (row.kind === 'play') return { on: plays[row.key], disabled: false }
  const blocked = !!row.needs && !c[row.needs]
  return { on: c[row.key] && !blocked, disabled: blocked }
}

/** What is on, in a few words (the title bar's picture menu). */
export function summary(c: YtmChoices): string {
  const parts = [c.count_plays ? 'Counting plays' : 'Plays not counted', c.likes ? 'likes on' : 'likes off']
  if (c.signed_search) parts.push('signed-in search')
  if (!c.personal) parts.push('general recommendations')
  return parts.join(', ')
}

/** The account card's line under the name, true to the switches. */
export function accountLine(c: YtmChoices): string {
  return [
    c.personal ? 'Home and radios are yours.' : 'Home and radios are YouTube’s general ones.',
    c.signed_search ? 'Searches go as you.' : 'Searches stay anonymous.',
    c.library ? 'Your playlists are in Library.' : 'Your YouTube Music library stays out of Venuta.',
  ].join(' ')
}
