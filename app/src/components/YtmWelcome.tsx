import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useState } from 'react'
import { useEscape } from '../lib/layers'
import { listening, loadListening, updateListening } from '../lib/listening'
import { notify } from '../lib/notify'
import { bringInYtmLikes } from '../lib/ytmAccount'
import { useYtmStatus } from '../lib/ytmLibrary'
import { Switch } from './ui'

/*
  The first time you sign in to YouTube Music: three offers, made once, each off until you say
  yes. Nothing is brought in, mirrored or reported without it; Settings, Online has the same
  three afterwards.
*/

export function YtmWelcome() {
  const { signed_in } = useYtmStatus()
  const [open, setOpen] = useState(false)
  const [likesIn, setLikesIn] = useState(false)
  const [likesOut, setLikesOut] = useState(false)
  const [plays, setPlays] = useState(false)
  useEffect(() => {
    if (!signed_in) return
    void loadListening().then(() => {
      if (!listening().ytm_offered) setOpen(true)
    })
  }, [signed_in])
  const close = () => {
    setOpen(false)
    void updateListening({ ytm_offered: true }).catch(() => {})
  }
  useEscape(open, close)
  const done = async () => {
    close()
    if (likesOut || plays) await updateListening({ sync_likes: likesOut, report_plays: plays }).catch((e) => notify(String(e)))
    if (likesIn) notify(await bringInYtmLikes())
  }
  const row = (on: boolean, set: (v: boolean) => void, title: string, text: string) => (
    <label className="flex cursor-pointer items-start gap-4 rounded-xl px-3 py-3 hover:bg-ink/[0.04]">
      <span className="min-w-0 flex-1">
        <span className="block text-[14px] font-medium">{title}</span>
        <span className="mt-0.5 block text-[12.5px] leading-relaxed text-ink/55">{text}</span>
      </span>
      <Switch on={on} onChange={set} label={title} />
    </label>
  )
  return (
    <AnimatePresence>
      {open && (
        <motion.div
          className="fixed inset-0 z-50 flex items-start justify-center bg-[var(--scrim)] pt-[12vh]"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          onMouseDown={close}
        >
          <motion.div
            role="dialog"
            aria-label="Signed in to YouTube Music"
            className="popover w-[min(520px,92vw)] rounded-[22px] p-5"
            initial={{ y: -10, opacity: 0 }}
            animate={{ y: 0, opacity: 1 }}
            exit={{ y: -6, opacity: 0 }}
            onMouseDown={(e) => e.stopPropagation()}
          >
            <h2 className="title text-[20px]">You&rsquo;re signed in to YouTube Music</h2>
            <p className="mt-1.5 text-[13px] leading-relaxed text-ink/60">
              Your playlists, albums, artists and songs are in Library now, beside your own music. Three more things, if you want them; each can be changed later in Settings, Online.
            </p>
            <div className="mt-3 grid gap-0.5">
              {row(likesIn, setLikesIn, 'Bring your Liked Music in', 'The songs you liked on YouTube Music join Liked songs here, once.')}
              {row(likesOut, setLikesOut, 'Like on YouTube Music too', 'Online songs you like here are liked on your account too.')}
              {row(plays, setPlays, 'Count plays on YouTube Music', 'After 30 seconds of an online song, your account is told it was played, so your recommendations learn from Venuta. Skips and your files are never sent.')}
            </div>
            <div className="mt-4 flex justify-end gap-2">
              <button onClick={close} className="btn-ghost">
                Not now
              </button>
              <button onClick={() => void done()} className="btn-primary">
                Done
              </button>
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  )
}
