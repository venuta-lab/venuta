import type { Availability } from '../lib/engine'

/**
 * Where a song plays from, in a word: "Online" (the online catalog) or "Local" (your file).
 * Lists show it only when they hold online songs, so a library-only list looks as it always did.
 */
export function AvailabilityBadge({ availability }: { availability: Availability | null | undefined }) {
  if (!availability) return null
  const online = availability === 'online'
  return (
    <span
      title={
        online ? 'Plays from the online catalog' : availability === 'local_preferred' ? 'In your library: plays from your file, not online' : 'Plays from your library'
      }
      className={`inline-block shrink-0 rounded-full px-1.5 align-middle text-[10px] font-semibold uppercase leading-[16px] tracking-[0.04em] ${
        online ? 'bg-accent/15 text-accent-ink' : 'bg-ink/[0.07] text-ink/55'
      }`}
    >
      {online ? 'Online' : 'Local'}
    </span>
  )
}
