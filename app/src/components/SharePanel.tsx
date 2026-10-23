import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useState } from 'react'
import { useEscape } from '../lib/layers'
import { notify } from '../lib/notify'
import { chooseShared, closeShare, shareQr, shareToApp, useShareRequest, type ShareRequest } from '../lib/share'
import { IconShare } from './Icons'

/*
  The share panel: the link, Copy, Windows' own share panel (Share to an app) and a QR code to
  scan with a phone; for a song of yours with no sure match, the closest online songs to choose
  from (the choice is kept for that file). Opened by "More ways" after a link is copied.
*/

const length = (ms: number | null) => {
  if (!ms) return ''
  const s = Math.round(ms / 1000)
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`
}

export function ShareHost() {
  const r = useShareRequest()
  useEscape(!!r, closeShare)
  return (
    <AnimatePresence>
      {r && (
        <motion.div
          className="fixed inset-0 z-[60] grid place-items-center bg-[var(--scrim)] p-6"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.12 }}
          onMouseDown={closeShare}
        >
          <motion.div
            role="dialog"
            aria-label={`Share ${r.name}`}
            className="popover w-[min(440px,92vw)] rounded-[18px] p-6"
            initial={{ scale: 0.96, opacity: 0 }}
            animate={{ scale: 1, opacity: 1 }}
            exit={{ scale: 0.97, opacity: 0 }}
            transition={{ type: 'spring', stiffness: 520, damping: 36 }}
            onMouseDown={(e) => e.stopPropagation()}
          >
            <Panel r={r} />
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  )
}

function Panel({ r }: { r: ShareRequest }) {
  return (
    <>
      <div className="flex items-start gap-3">
        <h2 className="min-w-0 flex-1 text-[17px] font-semibold leading-snug">Share “{r.name}”</h2>
        <button aria-label="Close" title="Close" onClick={closeShare} className="btn-ghost -mr-2 -mt-1 h-8 w-8 rounded-full p-0 text-[18px] leading-none">
          ×
        </button>
      </div>
      {r.note && <p className="mt-1.5 text-[13px] leading-relaxed text-ink/60">{r.note}</p>}
      {r.url ? <Ways name={r.name} url={r.url} /> : <Offered r={r} />}
    </>
  )
}

function Ways({ name, url }: { name: string; url: string }) {
  const [qr, setQr] = useState('')
  useEffect(() => {
    let live = true
    shareQr(url)
      .then((svg) => live && setQr(svg))
      .catch(() => {})
    return () => {
      live = false
    }
  }, [url])
  const copy = () =>
    navigator.clipboard
      .writeText(url)
      .then(() => notify(`Link to “${name}” copied.`))
      .catch(() => notify("Couldn't reach the clipboard."))
  return (
    <>
      <div className="mt-4 flex items-center gap-2">
        <input readOnly value={url} aria-label="Link" onFocus={(e) => e.currentTarget.select()} className="field h-9 min-w-0 flex-1 rounded-full px-3 text-[12.5px]" />
        <button onClick={copy} className="btn">
          Copy
        </button>
      </div>
      <button
        onClick={() =>
          void shareToApp(name, url).catch((e) => notify(`Couldn't open Windows' share panel: ${String(e)}`))
        }
        className="btn-primary mt-3 flex w-full items-center justify-center gap-2"
      >
        <IconShare width={16} height={16} />
        Share to an app
      </button>
      <p className="mt-1.5 text-center text-[12px] text-ink/50">WhatsApp, Mail, Phone Link, Nearby Share and other apps on this PC.</p>
      {qr && (
        <div className="mt-4 flex flex-col items-center gap-1.5">
          <div className="h-[168px] w-[168px] overflow-hidden rounded-xl bg-white p-1.5 [&>svg]:h-full [&>svg]:w-full" dangerouslySetInnerHTML={{ __html: qr }} />
          <p className="text-[12px] text-ink/50">Scan with a phone to open it there.</p>
        </div>
      )}
    </>
  )
}

function Offered({ r }: { r: ShareRequest }) {
  const offered = r.offered ?? []
  if (!offered.length || !r.path) {
    return (
      <div className="mt-5 flex justify-end">
        <button onClick={closeShare} className="btn">
          Close
        </button>
      </div>
    )
  }
  const path = r.path
  return (
    <>
      <ul className="mt-4 grid gap-1.5">
        {offered.map((t) => (
          <li key={t.track_id} className="flex items-center gap-3 rounded-xl bg-ink/[0.05] px-3 py-2">
            <span className="min-w-0 flex-1">
              <span className="block truncate text-[13.5px] font-medium">{t.title}</span>
              <span className="block truncate text-[12px] text-ink/55">{[t.artists.join(', '), length(t.duration_ms)].filter(Boolean).join(' · ')}</span>
            </span>
            <button onClick={() => chooseShared(path, r.name, t)} className="btn btn-sm shrink-0">
              This one
            </button>
          </li>
        ))}
      </ul>
      <div className="mt-4 flex justify-end">
        <button onClick={closeShare} className="btn-ghost">
          None of these
        </button>
      </div>
    </>
  )
}
