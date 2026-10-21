import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useRef, useState } from 'react'
import { ytmSetTasteProfile, ytmTasteProfile, type TasteArtist } from '../lib/engine'
import { useEscape } from '../lib/layers'
import { notify } from '../lib/notify'
import { onlineProblem } from '../lib/online'
import { YTM_TASTE_EVENT } from '../lib/ytmChoices'
import { firstChosen, tasteAnswer, toggleChoice } from '../lib/ytmTaste'
import { RemoteArt } from './RemoteArt'

/*
  Choose artists you like: YouTube Music's own taste builder, the artists it offers as round
  pictures to pick. Nothing is sent while you pick; Done tells YouTube Music the ones chosen, and
  its recommendations start from them. Opened from Settings, Online and from Home, signed in.
*/

export function YtmTaste() {
  const [open, setOpen] = useState(false)
  const [artists, setArtists] = useState<TasteArtist[] | null>(null)
  const [problem, setProblem] = useState('')
  const [chosen, setChosen] = useState<Set<string>>(new Set())
  const [busy, setBusy] = useState(false)
  const back = useRef<HTMLElement | null>(null)
  const box = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const show = () => {
      back.current = document.activeElement as HTMLElement | null
      setOpen(true)
      setArtists(null)
      setProblem('')
      ytmTasteProfile()
        .then((a) => {
          setArtists(a)
          setChosen(firstChosen(a))
        })
        .catch((e) => {
          setArtists([])
          setProblem(onlineProblem(e))
        })
    }
    window.addEventListener(YTM_TASTE_EVENT, show)
    return () => window.removeEventListener(YTM_TASTE_EVENT, show)
  }, [])

  useEffect(() => {
    if (!open) return
    const t = window.setTimeout(() => box.current?.querySelector<HTMLElement>('button')?.focus(), 30)
    return () => {
      window.clearTimeout(t)
      back.current?.focus?.()
    }
  }, [open])

  const close = () => setOpen(false)
  useEscape(open, close)

  const done = async () => {
    if (!artists || !chosen.size) return
    setBusy(true)
    try {
      const a = tasteAnswer(artists, chosen)
      await ytmSetTasteProfile(a.impressions, a.selected)
      notify(`YouTube Music has the ${chosen.size === 1 ? 'artist' : `${chosen.size} artists`} you chose. Home and radio start from them.`)
      setOpen(false)
    } catch (e) {
      notify(onlineProblem(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <AnimatePresence>
      {open && (
        <motion.div
          className="fixed inset-0 z-50 flex items-start justify-center bg-[var(--scrim)] pt-[7vh]"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          onMouseDown={close}
        >
          <motion.div
            ref={box}
            role="dialog"
            aria-modal="true"
            aria-labelledby="ytm-taste-title"
            className="popover flex max-h-[86vh] w-[min(640px,92vw)] flex-col rounded-[22px] p-5"
            initial={{ y: -10, opacity: 0 }}
            animate={{ y: 0, opacity: 1 }}
            exit={{ y: -6, opacity: 0 }}
            onMouseDown={(e) => e.stopPropagation()}
          >
            <h2 id="ytm-taste-title" className="title text-[20px]">
              Choose artists you like
            </h2>
            <p className="mt-1.5 text-[13px] leading-relaxed text-ink/60">
              YouTube Music recommends from the artists you pick. Nothing is sent until you press Done.
            </p>
            <div className="-mx-2 mt-4 min-h-0 flex-1 overflow-y-auto px-2">
              {artists === null ? (
                <p className="py-8 text-center text-[13px] text-ink/45">Reading the artists YouTube Music offers…</p>
              ) : problem ? (
                <p role="status" className="py-8 text-center text-[13px] text-ink/55">
                  {problem}
                </p>
              ) : !artists.length ? (
                <p className="py-8 text-center text-[13px] text-ink/55">YouTube Music offers no artists to choose right now.</p>
              ) : (
                <div className="grid grid-cols-[repeat(auto-fill,minmax(104px,1fr))] gap-x-3 gap-y-4 pb-2">
                  {artists.map((a) => {
                    const on = chosen.has(a.selection_value)
                    return (
                      <button
                        key={a.selection_value}
                        aria-pressed={on}
                        onClick={() => setChosen((c) => toggleChoice(c, a.selection_value))}
                        className="group flex min-w-0 flex-col items-center gap-2 rounded-2xl p-1.5 text-center hover:bg-ink/[0.05]"
                      >
                        <span
                          className={`relative grid aspect-square w-full max-w-[96px] place-items-center overflow-hidden rounded-full bg-ink/[0.07] text-[28px] text-ink/30 transition-shadow ${on ? 'shadow-[0_0_0_3px_var(--accent)]' : ''}`}
                        >
                          <RemoteArt url={a.picture} fallback={a.name.slice(0, 1)} size={96} imgClassName="h-full w-full object-cover" />
                          {on && (
                            <span className="absolute inset-0 grid place-items-center bg-[var(--scrim)] text-ink">
                              <svg width="26" height="26" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
                                <path d="m5 12.5 4.5 4.5L19 7.5" />
                              </svg>
                            </span>
                          )}
                        </span>
                        <span className="w-full truncate text-[12.5px] font-medium">{a.name}</span>
                      </button>
                    )
                  })}
                </div>
              )}
            </div>
            <div className="mt-4 flex items-center justify-end gap-2">
              <span className="mr-auto text-[12.5px] text-ink/55">{chosen.size ? `${chosen.size} chosen` : 'Pick one or more'}</span>
              <button onClick={close} className="btn-ghost">
                Cancel
              </button>
              <button onClick={() => void done()} disabled={busy || !chosen.size} className="btn-primary">
                {busy ? 'Telling YouTube Music…' : 'Done'}
              </button>
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  )
}
