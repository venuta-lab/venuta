/**
 * Venuta's mark and name, drawn by the theme. The mark (a V with a peacock feather and a
 * flute's holes) is lines only: the theme gives it its colour, glow and line ends, so it
 * belongs to whichever theme, colour and light the app is in. The name is set in the theme's
 * own display type.
 */
export function Brand(props: {
  /** The mark's height in px. */
  size?: number
  /** The name's size in px (about two thirds of the mark by default). */
  word?: number
  /** Just the mark, without the name. */
  markOnly?: boolean
  className?: string
}) {
  const size = props.size ?? 26
  return (
    <span className={`brand ${props.className ?? ''}`} style={{ fontSize: props.word ?? size * 0.66 }} aria-label="Venuta">
      <Mark size={size} />
      {!props.markOnly && (
        <span className="brand-word" aria-hidden>
          Venuta
        </span>
      )}
    </span>
  )
}

/** The mark alone. Lines thicken as it gets smaller, and fine detail drops out under 40 px. */
export function Mark(props: { size: number; className?: string }) {
  const { size } = props
  // About 1.5 px on screen at the least, never thinner than the drawing's own line.
  const stroke = Math.max(34, (1.5 * 940) / size)
  const detail = size >= 40
  return (
    <svg
      className={`brand-mark ${props.className ?? ''}`}
      viewBox="180 30 680 940"
      width={(size * 680) / 940}
      height={size}
      fill="none"
      stroke="currentColor"
      strokeWidth={stroke}
      aria-hidden
    >
      {/* One line: the feather's edge, its tip, a sweep left, then the V, outside and in. */}
      <path d="M410 364 C362 348 346 304 350 256 C360 162 452 82 690 58 C700 172 680 292 602 366 C560 398 432 382 362 420 C320 442 300 458 305 486 L468 904 Q515 978 556 904 L798 392 L662 392 L503 770 L378 470 C372 440 392 412 424 402" />
      {detail && <path d="M396 318 C384 214 470 124 656 94 C660 188 626 270 556 326" />}
      {/* The feather's eye. */}
      <circle cx="512" cy="238" r="84" />
      <circle cx="512" cy="238" r={detail ? 32 : 40} fill="currentColor" stroke="none" />
      {/* The flute's holes. */}
      {detail && <path d="M414 556 l20 -9 M438 614 l20 -9 M462 672 l20 -9 M612 506 l-20 -9 M588 562 l-20 -9 M564 618 l-20 -9" />}
      {/* The small triangle beside it. */}
      <path d="M236 392 L300 392 L265 456 Z" />
    </svg>
  )
}
