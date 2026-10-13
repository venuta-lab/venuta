import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useRef, useSyncExternalStore } from 'react'
import { useEscape } from '../lib/layers'

/*
  A question before something you can't simply take back ("Delete Road trip?"), in the app's own
  dialog. confirmAsk() resolves true or false; <ConfirmHost> (in App) shows it.
*/
export interface Ask {
  title: string
  text?: string
  /** The button that goes ahead ("Delete"). */
  confirm: string
  /** It removes or replaces something: the button says so in colour. */
  danger?: boolean
}
type Pending = Ask & { resolve: (ok: boolean) => void }

let pending: Pending | null = null
const listeners = new Set<() => void>()
const emit = () => listeners.forEach((l) => l())

export function confirmAsk(ask: Ask): Promise<boolean> {
  pending?.resolve(false)
  return new Promise((resolve) => {
    pending = { ...ask, resolve }
    emit()
  })
}

function answer(ok: boolean) {
  const p = pending
  pending = null
  emit()
  p?.resolve(ok)
}

export function ConfirmHost() {
  const p = useSyncExternalStore(
    (l) => {
      listeners.add(l)
      return () => {
        listeners.delete(l)
      }
    },
    () => pending,
  )
  useEscape(!!p, () => answer(false))
  const yes = useRef<HTMLButtonElement>(null)
  useEffect(() => {
    if (p) window.setTimeout(() => yes.current?.focus(), 30)
  }, [p])
  return (
    <AnimatePresence>
      {p && (
        <motion.div
          className="fixed inset-0 z-[60] grid place-items-center bg-[var(--scrim)] p-6"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.12 }}
          onMouseDown={() => answer(false)}
        >
          <motion.div
            role="alertdialog"
            aria-label={p.title}
            className="popover w-[min(420px,92vw)] rounded-[18px] p-6"
            initial={{ scale: 0.96, opacity: 0 }}
            animate={{ scale: 1, opacity: 1 }}
            exit={{ scale: 0.97, opacity: 0 }}
            transition={{ type: 'spring', stiffness: 520, damping: 36 }}
            onMouseDown={(e) => e.stopPropagation()}
          >
            <h2 className="text-[17px] font-semibold leading-snug">{p.title}</h2>
            {p.text && <p className="mt-2 text-[13.5px] leading-relaxed text-ink/65">{p.text}</p>}
            <div className="mt-6 flex justify-end gap-2">
              <button onClick={() => answer(false)} className="btn">
                Cancel
              </button>
              <button
                ref={yes}
                onClick={() => answer(true)}
                className={p.danger ? 'btn-danger' : 'btn-primary'}
              >
                {p.confirm}
              </button>
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  )
}
