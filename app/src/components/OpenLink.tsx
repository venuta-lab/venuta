import { invoke } from '@tauri-apps/api/core'
import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useRef, useState } from 'react'
import { useEscape } from '../lib/layers'
import { openLink, type Opened } from '../lib/openLink'

/*
  Open link (Ctrl+O, the palette): paste a YouTube Music or Spotify link to a song, album,
  artist or playlist, and it opens. What is on the clipboard is offered when it is a link.
*/

const spring = { type: 'spring', stiffness: 460, damping: 36 } as const

export function OpenLink(props: { open: boolean; root: string; onClose: () => void; onOpened: (where: Opened) => void }) {
  return <AnimatePresence>{props.open && <Dialog key="open-link" {...props} />}</AnimatePresence>
}

function Dialog(props: { root: string; onClose: () => void; onOpened: (where: Opened) => void }) {
  useEscape(true, props.onClose)
  const [link, setLink] = useState('')
  const [busy, setBusy] = useState(false)
  const [problem, setProblem] = useState('')
  const field = useRef<HTMLInputElement>(null)
  // A link on the clipboard is filled in, ready to open.
  useEffect(() => {
    void invoke<string>('clipboard_text')
      .then((t) => {
        const v = (t ?? '').trim()
        if (/^(https?:\/\/|spotify:)/i.test(v) && v.length < 500) setLink((now) => now || v)
      })
      .catch(() => {})
    field.current?.focus()
  }, [])
  const go = async () => {
    if (!link.trim() || busy) return
    setBusy(true)
    setProblem('')
    try {
      const where = await openLink(props.root, link.trim())
      props.onOpened(where)
      props.onClose()
    } catch (e) {
      setProblem(String(e).replace(/^Error: /, ''))
    } finally {
      setBusy(false)
    }
  }
  return (
    <motion.div className="fixed inset-0 z-50 grid place-items-start justify-center bg-black/30 pt-[14vh]" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} onMouseDown={props.onClose}>
      <motion.form
        role="dialog"
        aria-label="Open a link"
        initial={{ y: -10, opacity: 0, scale: 0.98 }}
        animate={{ y: 0, opacity: 1, scale: 1 }}
        exit={{ y: -6, opacity: 0, scale: 0.98 }}
        transition={spring}
        onMouseDown={(e) => e.stopPropagation()}
        onSubmit={(e) => {
          e.preventDefault()
          void go()
        }}
        className="popover w-[min(560px,92vw)] rounded-[20px] p-5"
      >
        <h2 className="text-[17px] font-semibold tracking-[-0.01em]">Open a link</h2>
        <p className="mt-1 text-[12.5px] text-ink/55">A song, album, artist or playlist on YouTube Music or Spotify. A song you have plays from your file.</p>
        <input
          ref={field}
          value={link}
          onChange={(e) => setLink(e.target.value)}
          placeholder="https://music.youtube.com/… or https://open.spotify.com/…"
          aria-label="Link"
          spellCheck={false}
          className="field mt-4 h-10 w-full rounded-full px-4 text-[13px]"
        />
        {problem && (
          <p role="alert" className="mt-2 text-[12.5px] text-problem">
            {problem}
          </p>
        )}
        <div className="mt-4 flex justify-end gap-2">
          <button type="button" onClick={props.onClose} className="btn-ghost btn-sm">
            Cancel
          </button>
          <button type="submit" disabled={!link.trim() || busy} className="btn-primary btn-sm">
            {busy ? 'Opening…' : 'Open'}
          </button>
        </div>
      </motion.form>
    </motion.div>
  )
}
