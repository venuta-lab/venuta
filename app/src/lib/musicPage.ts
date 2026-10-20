import type { MusicPage } from '../screens/ListenScreen'

/*
  A page of Music opened from outside Music (the queue, Now Playing): App hears the ask, goes to
  Music and opens the page there, and what was shown over Music closes, as when the palette
  opens a page.
*/

/** The event that asks the app to go to Music and open a page there. */
export const OPEN_MUSIC_PAGE = 'venuta:open-page'

/** Go to Music and open this page there. */
export const openMusicPage = (page: MusicPage) => window.dispatchEvent(new CustomEvent<MusicPage>(OPEN_MUSIC_PAGE, { detail: page }))
