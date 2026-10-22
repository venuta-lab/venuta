import { useLayoutEffect, useRef, useState } from 'react'
import { nextFit, shrinkFor } from '../../lib/lyricFit'

/**
 * Words in a box of fixed height (the mini player's slots): wrapped, and made smaller as much as
 * they must to fit it, never cut off. `key` says when to measure afresh (the line, the rows, the
 * size). Returns the refs to put on the box and on the words inside it, and the share of the
 * size to draw them at.
 */
export function useShrinkToFit<O extends HTMLElement, I extends HTMLElement>(key: string, min = 0.55) {
  const box = useRef<O>(null)
  const inner = useRef<I>(null)
  const [fit, setFit] = useState(1)
  const [size, setSize] = useState({ w: 0, h: 0 })
  useLayoutEffect(() => {
    const b = box.current
    if (!b) return
    const o = new ResizeObserver(() => setSize({ w: b.clientWidth, h: b.clientHeight }))
    o.observe(b)
    return () => o.disconnect()
  }, [])
  const measureKey = `${key}|${size.w}|${size.h}`
  const [measured, setMeasured] = useState(measureKey)
  if (measured !== measureKey) {
    setMeasured(measureKey)
    setFit(1)
  }
  useLayoutEffect(() => {
    const b = box.current
    const i = inner.current
    if (!b || !i) return
    const want = Math.max(min, nextFit(fit, shrinkFor({ rows: [{ scroll: i.scrollWidth, client: b.clientWidth }], height: i.scrollHeight, room: b.clientHeight })))
    if (want !== fit) setFit(want)
  }, [fit, measureKey, min])
  return { box, inner, fit }
}
