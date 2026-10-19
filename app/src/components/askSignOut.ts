import { ytmSignOut } from '../lib/engine'
import { notify } from '../lib/notify'
import { confirmAsk } from './Confirm'

/** Sign out of YouTube Music, asked first (Settings, Online and the title bar's switcher). */
export async function askSignOut() {
  const ok = await confirmAsk({
    title: 'Sign out of YouTube Music?',
    text: 'Venuta forgets this sign-in on this computer. Online music keeps playing, for everyone rather than for you. Your playlists and likes in Venuta stay, and linked playlists wait until you sign in again.',
    confirm: 'Sign out',
  })
  if (ok) await ytmSignOut().catch((e) => notify(String(e)))
}
