import { openFrom, readFrom, usePlayerValue } from '../../lib/player'
import { roomWords, useTogether } from '../../lib/together'

/** "Playing from Hindi": the playlist, album or artist the queue came from, as a way back to it. */
export function PlayingFrom(props: { className?: string; onOpen?: () => void }) {
  const raw = usePlayerValue((s) => s.from)
  const v = useTogether()
  // In a room the music comes from the room.
  const room = roomWords(v)
  if (room) return <span className={props.className}>{room}</span>
  const f = readFrom(raw)
  if (!f?.label) return null
  const linked = f.kind === 'playlist' || f.kind === 'album' || f.kind === 'artist'
  return (
    <span className={props.className}>
      Playing from{' '}
      {linked ? (
        <button
          onClick={() => {
            openFrom(f)
            props.onOpen?.()
          }}
          className="font-medium text-ink/80 underline-offset-4 hover:text-ink hover:underline"
          title={f.kind === 'playlist' ? `Open the playlist “${f.label}”` : f.kind === 'album' ? `Open the album “${f.label}”` : `Open ${f.label}`}
        >
          {f.label}
        </button>
      ) : (
        <span className="text-ink/80">{f.label}</span>
      )}
    </span>
  )
}
