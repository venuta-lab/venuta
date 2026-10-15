import { useEffect, useRef, useState, type ReactNode } from 'react'

/*
  The picture of what is playing (the bar, Now Playing, the mini player), held steady across a
  change of song: the old picture stays until the new one is decoded, then the new one fades in
  over it. A song with no picture shows the placeholder once that is known, never a blank frame
  in between. Local covers and online artwork (already cleaned, see RemoteArt's cleanArtUrl)
  alike.
*/

/** How long a picture may take before the placeholder shows in its place. */
const PATIENCE_MS = 1500

export function SteadyArt(props: { src: string | null | undefined; fallback?: ReactNode; className?: string }) {
  const [front, setFront] = useState<string | null>(null)
  const [back, setBack] = useState<string | null>(null)
  const asked = useRef<string | null | undefined>(undefined)
  useEffect(() => {
    const src = props.src ?? null
    asked.current = src
    if (src === front) return
    let live = true
    // Nothing to show (no cover), or a picture that never comes: the placeholder, after a moment
    // (a cover arriving from the cache in that time is shown instead).
    const giveUp = window.setTimeout(() => {
      if (live && asked.current === src) {
        setBack(null)
        setFront(null)
      }
    }, src ? PATIENCE_MS : 250)
    if (src) {
      const img = new Image()
      img.src = src
      img
        .decode()
        .then(() => {
          if (!live || asked.current !== src) return
          window.clearTimeout(giveUp)
          setBack(front)
          setFront(src)
        })
        .catch(() => {
          if (!live || asked.current !== src) return
          window.clearTimeout(giveUp)
          setBack(null)
          setFront(null)
        })
    }
    return () => {
      live = false
      window.clearTimeout(giveUp)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [props.src])

  // The old picture goes once the new one has faded in over it.
  useEffect(() => {
    if (!back) return
    const t = window.setTimeout(() => setBack(null), 400)
    return () => window.clearTimeout(t)
  }, [back])

  const img = 'absolute inset-0 h-full w-full object-cover'
  return (
    <span className={`relative block h-full w-full ${props.className ?? ''}`}>
      {back && <img key={back} src={back} alt="" draggable={false} className={img} />}
      {front ? (
        <img key={front} src={front} alt="" draggable={false} className={`${img} art-in`} />
      ) : (
        <span className="absolute inset-0 grid place-items-center">{props.fallback}</span>
      )}
    </span>
  )
}
