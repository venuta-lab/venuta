import { useLayoutEffect, useRef, useState } from 'react'

/*
  Handlers that keep one identity for the component's whole life and always run their latest
  version. A parent can then hand them to a memoised page without the page being drawn again
  each time the parent is (a notice, a progress tick, the queue opening).
*/
// eslint-disable-next-line @typescript-eslint/no-explicit-any
type Fn = (...args: any[]) => any

export function useStable<T extends Record<string, Fn>>(fns: T): T {
  const latest = useRef(fns)
  useLayoutEffect(() => {
    latest.current = fns
  })
  const [stable] = useState(
    () => Object.fromEntries(Object.keys(fns).map((k) => [k, (...args: unknown[]) => latest.current[k](...args)])) as T,
  )
  return stable
}
