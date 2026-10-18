import type { Availability } from '../lib/engine'
import { togetherView } from '../lib/together'

/**
 * Where a song plays from, in a word: "Online" (the online catalog) or "Local" (your file); in a
 * room, "From <host>" for a song the host streams from their file. Lists show it only when they
 * hold online songs, so a library-only list looks as it always did.
 */
export function AvailabilityBadge({ availability }: { availability: Availability | null | undefined }) {
  if (!availability) return null
  const online = availability === 'online'
  const room = availability === 'room'
  const host = room ? (togetherView().room?.host_name ?? 'the host') : ''
  return (
    <span
      title={
        room
          ? `Streamed to you from ${host}'s file`
          : online
            ? 'Plays from the online catalog'
            : availability === 'local_preferred'
              ? 'In your library: plays from your file, not online'
              : 'Plays from your library'
      }
      className={`inline-block shrink-0 rounded-full px-1.5 align-middle text-[10px] font-semibold uppercase leading-[16px] tracking-[0.04em] ${
        online ? 'bg-accent/15 text-accent-ink' : room ? 'bg-ink/[0.1] text-ink/70' : 'bg-ink/[0.07] text-ink/55'
      }`}
    >
      {room ? `From ${host}` : online ? 'Online' : 'Local'}
    </span>
  )
}
