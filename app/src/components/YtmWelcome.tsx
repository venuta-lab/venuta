import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useRef, useState } from 'react'
import { useEscape } from '../lib/layers'
import { listening, loadListening, updateListening } from '../lib/listening'
import { notify } from '../lib/notify'
import { newLikesIn } from '../lib/ytmAccount'
import { playChoicesOf, SAFE, type PlayChoices, type YtmChoices } from '../lib/ytmChoiceWords'
import { saveYtmChoices, useYtmChoices, YTM_CHOICES_EVENT, ytmChoicesNow } from '../lib/ytmChoices'
import { YtmChoiceRows } from './YtmChoices'

/*
  Your YouTube Music choices, once for each account and channel: the first time it is in use
  (after a sign-in, or a switch to another account or channel), every switch is shown with the
  value that really holds, the safe ones for an account never asked. Done keeps what you set.
  Not now, Escape or a click outside keep them as shown, so you aren't asked again, and never
  turn anything on. The title bar's picture menu opens the same screen any time.
*/

interface Shown {
  account: string
  first: boolean
  choices: YtmChoices
  plays: PlayChoices
}

/** The account's name as the key has it ("email#channel"), for "For ...". */
const whoOf = (account: string) => {
  const at = account.lastIndexOf('#')
  const who = at > 0 ? account.slice(0, at) : account
  return at > 0 && account.slice(at + 1) !== 'own' ? `${who}, on a brand channel` : who
}

export function YtmWelcome() {
  const view = useYtmChoices()
  const [shown, setShown] = useState<Shown | null>(null)
  const [choices, setChoices] = useState<YtmChoices>(SAFE)
  const [plays, setPlays] = useState<PlayChoices>(() => playChoicesOf(listening()))
  // Asked this session already (kept or not): not shown again by itself until the next start.
  const asked = useRef(new Set<string>())
  const box = useRef<HTMLDivElement>(null)
  const back = useRef<HTMLElement | null>(null)

  const show = async (account: string, first: boolean, now: YtmChoices) => {
    await loadListening()
    back.current = document.activeElement as HTMLElement | null
    const how = playChoicesOf(listening())
    setChoices(now)
    setPlays(how)
    setShown({ account, first, choices: now, plays: how })
  }

  // The account in use was never asked: ask now.
  useEffect(() => {
    if (!view || view.asked || asked.current.has(view.account)) return
    asked.current.add(view.account)
    void show(view.account, true, view.choices)
  }, [view])

  // New likes come in once per start, for an account that chose it.
  useEffect(() => {
    if (view?.asked && view.choices.new_likes_in) void newLikesIn(view.account)
  }, [view])

  // From the title bar's picture menu.
  useEffect(() => {
    const open = () => {
      const v = ytmChoicesNow()
      if (v) void show(v.account, false, v.choices)
    }
    window.addEventListener(YTM_CHOICES_EVENT, open)
    return () => window.removeEventListener(YTM_CHOICES_EVENT, open)
  }, [])

  // Focus goes in when it opens, and back where it was when it closes.
  useEffect(() => {
    if (!shown) return
    const t = window.setTimeout(() => box.current?.querySelector<HTMLElement>('[data-done]')?.focus(), 30)
    return () => {
      window.clearTimeout(t)
      back.current?.focus?.()
    }
  }, [shown])

  const keep = async (s: Shown, mine: YtmChoices, how: PlayChoices) => {
    try {
      await saveYtmChoices(mine, s.account)
      const changed = (Object.keys(how) as (keyof PlayChoices)[]).filter((k) => how[k] !== s.plays[k])
      if (changed.length) await updateListening(Object.fromEntries(changed.map((k) => [k, how[k]])))
    } catch (e) {
      notify(`Couldn't keep your choices: ${String(e)}`)
    }
  }
  // Not now, Escape, a click outside: kept as shown, nothing turned on.
  const close = () => {
    if (!shown) return
    setShown(null)
    void keep(shown, shown.choices, shown.plays)
  }
  const done = () => {
    if (!shown) return
    setShown(null)
    void keep(shown, choices, plays)
  }
  useEscape(!!shown, close)

  // Tab stays inside while it is open.
  const trap = (e: React.KeyboardEvent) => {
    if (e.key !== 'Tab' || !box.current) return
    const all = [...box.current.querySelectorAll<HTMLElement>('button:not([disabled])')]
    if (!all.length) return
    const [first, last] = [all[0], all[all.length - 1]]
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault()
      last.focus()
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault()
      first.focus()
    }
  }

  return (
    <AnimatePresence>
      {shown && (
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
            aria-labelledby="ytm-choices-title"
            className="popover flex max-h-[86vh] w-[min(560px,92vw)] flex-col rounded-[22px] p-5"
            initial={{ y: -10, opacity: 0 }}
            animate={{ y: 0, opacity: 1 }}
            exit={{ y: -6, opacity: 0 }}
            onMouseDown={(e) => e.stopPropagation()}
            onKeyDown={trap}
          >
            <h2 id="ytm-choices-title" className="title text-[20px]">
              {shown.first ? 'You’re signed in to YouTube Music' : 'Your YouTube Music choices'}
            </h2>
            <p className="mt-1.5 text-[13px] leading-relaxed text-ink/60">
              {shown.first ? 'Choose what Venuta does with your account. What only reads is on; what writes to your account or your history is off until you turn it on. ' : ''}
              For {whoOf(shown.account)}. Each account and channel keeps its own; Settings, Online has them too.
            </p>
            <div className="-mx-2 mt-3 min-h-0 flex-1 overflow-y-auto">
              <YtmChoiceRows
                choices={choices}
                plays={plays}
                onChoice={(c) => setChoices((was) => ({ ...was, ...c }))}
                onPlay={(c) => setPlays((was) => ({ ...was, ...c }))}
              />
            </div>
            <div className="mt-4 flex justify-end gap-2">
              <button onClick={close} className="btn-ghost">
                Not now
              </button>
              <button data-done onClick={done} className="btn-primary">
                Done
              </button>
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  )
}
