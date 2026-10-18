import { useEffect, useSyncExternalStore } from 'react'
import { listening, listeningVersion, loadListening, subscribeListening, updateListening } from '../../lib/listening'
import { jumpTo, readFrom, usePlayerValue } from '../../lib/player'
import { Switch } from '../ui'

/**
 * Under a queue's heading: Autoplay (when the queue runs out, songs like the last one go on),
 * and while a radio plays, the way back to the song it started from.
 */
export function QueueRadio({ className = '' }: { className?: string }) {
  useSyncExternalStore(subscribeListening, listeningVersion)
  useEffect(() => {
    void loadListening()
  }, [])
  const from = readFrom(usePlayerValue((s) => s.from))
  const index = usePlayerValue((s) => s.index)
  const seed = from?.kind === 'radio' && from.seed && index > 0 ? from.seed : ''
  return (
    <div className={`flex items-center gap-3 text-[12px] text-ink/60 ${className}`}>
      <label className="flex items-center gap-2" title="When the queue runs out, keep playing songs like the last one">
        <Switch on={listening().autoplay} onChange={(on) => void updateListening({ autoplay: on })} label="Autoplay" />
        Autoplay
      </label>
      {seed && (
        <button onClick={() => void jumpTo(0)} className="ml-auto truncate rounded-full px-2 py-0.5 text-accent-ink hover:bg-ink/[0.08]" title={`Play “${seed}” again, and the radio from it`}>
          Back to {seed}
        </button>
      )}
    </div>
  )
}
