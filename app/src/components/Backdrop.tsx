import { useEffect, useRef, useState } from 'react'

/*
  The living backdrop: the library's own covers as a soft field of colour, with a glow in the
  current accent.

  Performance: the covers are painted once onto a tiny canvas (blurred while painting), and
  the canvas is stretched to fill the window. Upscaling a tiny image is a free blur, and the
  slow drift is a transform, so the compositor does all per-frame work. No live CSS blur.
*/
const COLS = 6
const ROWS = 4
const W = 72
const H = 48

export function Backdrop({ covers }: { covers: string[] }) {
  const canvas = useRef<HTMLCanvasElement>(null)
  const [ready, setReady] = useState(false)
  const key = covers.slice(0, 6).join('|')

  useEffect(() => {
    if (!covers.length || !canvas.current) return
    let cancelled = false
    const ctx = canvas.current.getContext('2d')
    if (!ctx) return
    const tiles = Array.from({ length: COLS * ROWS }, (_, i) => covers[i % covers.length])
    Promise.all(
      tiles.map(
        (src) =>
          new Promise<HTMLImageElement | null>((resolve) => {
            const img = new Image()
            img.onload = () => resolve(img)
            img.onerror = () => resolve(null)
            img.src = src
          }),
      ),
    ).then((imgs) => {
      if (cancelled) return
      ctx.clearRect(0, 0, W, H)
      ctx.filter = 'blur(3px) saturate(1.7) brightness(0.8)'
      const tw = W / COLS
      const th = H / ROWS
      imgs.forEach((img, i) => {
        if (img) ctx.drawImage(img, (i % COLS) * tw - 2, Math.floor(i / COLS) * th - 2, tw + 4, th + 4)
      })
      setReady(true)
    })
    return () => {
      cancelled = true
    }
  }, [key, covers])

  return (
    <div
      aria-hidden
      className="pointer-events-none fixed inset-0 overflow-hidden transition-opacity duration-500"
      style={{ opacity: 'var(--backdrop-opacity)' }}
    >
      <canvas
        ref={canvas}
        width={W}
        height={H}
        className="backdrop-drift absolute inset-0 h-full w-full transition-opacity duration-[1400ms]"
        style={{ opacity: ready ? 1 : 0 }}
      />
      <div
        className="absolute inset-0"
        style={{
          background:
            'radial-gradient(120% 80% at 0% 0%, color-mix(in oklab, var(--accent) 34%, transparent), transparent 60%), radial-gradient(90% 70% at 100% 100%, color-mix(in oklab, var(--accent) 16%, transparent), transparent 70%)',
        }}
      />
      <div className="absolute inset-0 bg-scrim" />
    </div>
  )
}
