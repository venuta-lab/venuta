import { invoke } from '@tauri-apps/api/core'
import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { inApp } from '../lib/engine'
import { closeMenu, openMenu, useMenuOpen, type MenuGroups, type Open } from '../lib/menu'

/*
  The one menu of the window. Right-click anything, or press its "…": what can be done with it
  (lib/actions.ts) opens here, at the pointer or under the button. The window owns right-click:
  the browser's own menu (Back, Refresh, Print, Inspect) never shows. A text field gets Cut,
  Copy, Paste and Select all; selected words get Copy; empty space shows nothing.
*/

/* ---- Text fields and selected words -------------------------------------------------------- */

type Field = HTMLInputElement | HTMLTextAreaElement
const TEXT_INPUTS = new Set(['text', 'search', 'url', 'email', 'tel', 'number', ''])

function fieldOf(t: EventTarget | null): Field | HTMLElement | null {
  if (!(t instanceof HTMLElement)) return null
  if (t instanceof HTMLTextAreaElement) return t
  if (t instanceof HTMLInputElement) return TEXT_INPUTS.has(t.type) ? t : null
  return t.closest<HTMLElement>('[contenteditable=""], [contenteditable="true"]')
}

function textMenu(field: Field | HTMLElement): MenuGroups {
  const isField = field instanceof HTMLInputElement || field instanceof HTMLTextAreaElement
  const range = isField ? [field.selectionStart ?? 0, field.selectionEnd ?? 0] : null
  const picked = isField ? range![1] > range![0] : !!window.getSelection()?.toString()
  const locked = isField ? field.readOnly || field.disabled : false
  // Back into the field, with what was selected selected again, then do it.
  const inField = (run: () => void) => () => {
    field.focus({ preventScroll: true })
    if (isField && range) field.setSelectionRange(range[0], range[1])
    run()
  }
  return [
    [
      { label: 'Cut', disabled: !picked || locked, run: inField(() => document.execCommand('cut')) },
      { label: 'Copy', disabled: !picked, run: inField(() => document.execCommand('copy')) },
      {
        label: 'Paste',
        disabled: locked || !inApp(),
        run: () => {
          void invoke<string>('clipboard_text').then((text) => {
            if (text) inField(() => document.execCommand('insertText', false, text))()
          })
        },
      },
    ],
    [
      {
        label: 'Select all',
        run: inField(() => (isField ? field.select() : document.execCommand('selectAll'))),
      },
    ],
  ]
}

function onWindowMenu(e: MouseEvent) {
  // A row, a card or a button handled it (menuFromEvent), or a development build asked for the
  // inspector with Shift held.
  if (e.defaultPrevented || (import.meta.env.DEV && e.shiftKey)) return
  e.preventDefault()
  const field = fieldOf(e.target)
  if (field) {
    openMenu(textMenu(field), { x: e.clientX, y: e.clientY }, field)
    return
  }
  const words = window.getSelection()?.toString() ?? ''
  if (words.trim()) openMenu([[{ label: 'Copy', run: () => void navigator.clipboard.writeText(words) }]], { x: e.clientX, y: e.clientY })
}

/* ---- The menu ------------------------------------------------------------------------------ */


/** Put once in each window, at its root. */
export function ContextMenuHost() {
  const o = useMenuOpen()
  useEffect(() => {
    window.addEventListener('contextmenu', onWindowMenu)
    return () => window.removeEventListener('contextmenu', onWindowMenu)
  }, [])
  return createPortal(<AnimatePresence>{o && <Menu key={o.id} open={o} />}</AnimatePresence>, document.body)
}

const EDGE = 8

function Menu({ open: o }: { open: Open }) {
  const ref = useRef<HTMLDivElement>(null)
  // Where it goes, kept inside the window: at the pointer (right and down from it, or flipped),
  // or under the button with right edges lined up (or above it, near the bottom).
  const [pos, setPos] = useState<{ left: number; top: number; origin: string } | null>(null)
  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    const w = el.offsetWidth
    const h = el.offsetHeight
    const W = window.innerWidth
    const H = window.innerHeight
    let left: number
    let top: number
    let fromTop = true
    let fromLeft = true
    if ('anchor' in o.place) {
      const r = o.place.anchor.getBoundingClientRect()
      left = r.right - w
      fromLeft = false
      top = r.bottom + 6
      if (top + h > H - EDGE && r.top - 6 - h >= EDGE) {
        top = r.top - 6 - h
        fromTop = false
      }
    } else {
      left = o.place.x
      top = o.place.y
      if (left + w > W - EDGE) {
        left = o.place.x - w
        fromLeft = false
      }
      if (top + h > H - EDGE) {
        top = o.place.y - h
        fromTop = false
      }
    }
    left = Math.max(EDGE, Math.min(left, W - EDGE - w))
    top = Math.max(EDGE, Math.min(top, H - EDGE - h))
    setPos({ left, top, origin: `${fromTop ? 'top' : 'bottom'} ${fromLeft ? 'left' : 'right'}` })
  }, [o])

  useEffect(() => {
    ref.current?.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus({ preventScroll: true })
    const away = (e: PointerEvent) => {
      if (!ref.current?.contains(e.target as Node)) closeMenu(false)
    }
    const gone = () => closeMenu(false)
    document.addEventListener('pointerdown', away, true)
    window.addEventListener('scroll', gone, true)
    window.addEventListener('resize', gone)
    window.addEventListener('blur', gone)
    return () => {
      document.removeEventListener('pointerdown', away, true)
      window.removeEventListener('scroll', gone, true)
      window.removeEventListener('resize', gone)
      window.removeEventListener('blur', gone)
    }
  }, [])

  const onKey = (e: React.KeyboardEvent) => {
    const buttons = [...(ref.current?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)') ?? [])]
    const i = buttons.indexOf(document.activeElement as HTMLButtonElement)
    const n = buttons.length
    if (e.key === 'Escape' || e.key === 'Tab') {
      e.preventDefault()
      closeMenu()
    } else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault()
      buttons[(i + (e.key === 'ArrowDown' ? 1 : -1) + n) % n]?.focus()
    } else if (e.key === 'Home' || e.key === 'End') {
      e.preventDefault()
      buttons[e.key === 'Home' ? 0 : n - 1]?.focus()
    } else if (e.key.length === 1 && /\S/.test(e.key)) {
      // A letter goes to the next item starting with it.
      const k = e.key.toLowerCase()
      const from = i < 0 ? 0 : i + 1
      const hit = [...buttons.slice(from), ...buttons.slice(0, from)].find((b) => b.textContent?.trim().toLowerCase().startsWith(k))
      hit?.focus()
    }
    // Keys used here never reach the rows or the screen behind the menu.
    e.stopPropagation()
  }

  return (
    <motion.div
      ref={ref}
      role="menu"
      onKeyDown={onKey}
      onContextMenu={(e) => e.preventDefault()}
      initial={{ opacity: 0, scale: 0.96 }}
      animate={{ opacity: pos ? 1 : 0, scale: 1 }}
      exit={{ opacity: 0, scale: 0.96, transition: { duration: 0.12 } }}
      transition={{ type: 'spring', stiffness: 520, damping: 38 }}
      className="popover fixed z-[60] min-w-[200px] max-w-[320px] rounded-2xl p-1.5"
      style={{ left: pos?.left ?? 0, top: pos?.top ?? 0, transformOrigin: pos?.origin ?? 'top left' }}
    >
      {o.groups.map((g, gi) => (
        <div key={gi} role="group" className={gi > 0 ? 'mt-1.5 border-t border-[var(--edge)] pt-1.5' : ''}>
          {g.map((item) => (
            <button
              key={item.label}
              role="menuitem"
              disabled={item.disabled}
              onClick={() => {
                closeMenu()
                item.run()
              }}
              className={`block w-full truncate rounded-xl px-3 py-2 text-left text-[13px] focus-visible:outline-none disabled:opacity-40 enabled:hover:bg-ink/[0.08] enabled:focus-visible:bg-ink/[0.08] ${
                item.danger ? 'text-problem' : 'text-ink/85 enabled:hover:text-ink'
              }`}
            >
              {item.label}
            </button>
          ))}
        </div>
      ))}
    </motion.div>
  )
}
