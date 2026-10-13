import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useRef } from 'react'

/**
 * A notice at the bottom of the window. It closes by itself after a few seconds (not while the
 * pointer is over it), and always has a close button. Errors stay until they are closed.
 */
export function Toast(props: {
  text: string
  tone?: 'info' | 'error'
  onClose: () => void
  /** One action beside the words ("Undo"). Running it closes the notice. */
  action?: { label: string; run: () => void }
  /** Changes when the same words should show again. */
  stamp?: number
}) {
  const { text, tone = 'info', onClose, action } = props
  const hover = useRef(false)
  const undoable = !!action
  useEffect(() => {
    if (!text || tone === 'error') return
    // Long messages get a little longer to be read, and one with an Undo longer still.
    const ms = Math.min(14000, (undoable ? 8000 : 4000) + text.length * 40)
    let left = ms
    const tick = 250
    const t = window.setInterval(() => {
      if (hover.current) return
      left -= tick
      if (left <= 0) onClose()
    }, tick)
    return () => window.clearInterval(t)
  }, [text, tone, onClose, undoable, props.stamp])
  return (
    <AnimatePresence>
      {text && (
        <motion.div
          key={`${text}#${props.stamp ?? 0}`}
          role={tone === 'error' ? 'alert' : 'status'}
          initial={{ y: 40, opacity: 0 }}
          animate={{ y: 0, opacity: 1 }}
          exit={{ y: 40, opacity: 0 }}
          onMouseEnter={() => (hover.current = true)}
          onMouseLeave={() => (hover.current = false)}
          className="toast popover absolute bottom-5 left-1/2 z-40 flex max-w-[680px] -translate-x-1/2 items-start gap-3 rounded-2xl py-3 pl-5 pr-2 text-[13px]"
        >
          <span className="min-w-0 flex-1 pt-0.5">
            {tone === 'error' && <span className="text-problem">Something went wrong. </span>}
            <span className="text-ink/85">{text}</span>
          </span>
          {action && (
            <button
              onClick={() => {
                action.run()
                onClose()
              }}
              className="btn-ghost btn-sm -my-0.5 shrink-0 font-semibold text-accent-ink"
            >
              {action.label}
            </button>
          )}
          <button
            onClick={onClose}
            aria-label="Close"
            className="grid h-6 w-6 shrink-0 place-items-center rounded-full text-[15px] leading-none text-ink/55 hover:bg-ink/10 hover:text-ink"
          >
            ×
          </button>
        </motion.div>
      )}
    </AnimatePresence>
  )
}
