import { useEffect, useRef } from 'react'

/*
  Escape closes the topmost thing only. Every overlay (palette, dialogs, the queue, Now Playing,
  an album page) registers while it is open; one key handler asks the newest one to close.
  Without this, one press closed every layer that happened to be listening.
*/
type Layer = { close: () => void; order: number }
const stack: Layer[] = []
let installed = false
let counter = 0

function install() {
  if (installed) return
  installed = true
  window.addEventListener('keydown', (e) => {
    if (e.key !== 'Escape' || e.defaultPrevented || !stack.length) return
    // Typing in a field: Escape belongs to the field first (it clears or blurs it).
    const t = e.target
    if (t instanceof HTMLElement && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA') && t.dataset.escape !== 'layer') {
      if ((t as HTMLInputElement).value) return
    }
    e.preventDefault()
    const top = stack.reduce((a, b) => (b.order > a.order ? b : a))
    top.close()
  })
}

/** While `open`, Escape (when this is the topmost layer) calls `onClose`. */
export function useEscape(open: boolean, onClose: () => void) {
  const latest = useRef(onClose)
  useEffect(() => {
    latest.current = onClose
  })
  useEffect(() => {
    if (!open) return
    install()
    const layer: Layer = { close: () => latest.current(), order: ++counter }
    stack.push(layer)
    return () => {
      const i = stack.indexOf(layer)
      if (i >= 0) stack.splice(i, 1)
    }
  }, [open])
}
