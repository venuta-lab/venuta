// Each person's initial in the theme's own accent, a little stronger or softer per person so faces
// side by side stay apart; never colours of their own that clash with the theme.
const TINTS = [34, 22, 46, 28, 40, 18]

export function Initial({ m, size = 22 }: { m: { id: number; name: string }; size?: number }) {
  const tint = TINTS[m.id % TINTS.length]
  return (
    <span
      className="grid shrink-0 place-items-center rounded-full font-semibold text-accent-ink"
      style={{ width: size, height: size, fontSize: size * 0.48, background: `color-mix(in oklab, var(--accent) ${tint}%, transparent)` }}
      aria-hidden
    >
      {(m.name.trim()[0] ?? '?').toUpperCase()}
    </span>
  )
}
