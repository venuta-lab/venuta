import { useSpectrum } from '../../lib/player'

/**
 * A segmented spectrum analyser on a green LCD (Skin '99): sixteen bands from the engine, lit
 * green to yellow to red with the dark "ghost" segments of a real display behind them. Still
 * (all ghosts) while nothing plays.
 */
export function LcdSpectrum({ className = '', segments = 10 }: { className?: string; segments?: number }) {
  const bands = useSpectrum()
  return (
    <div className={`lcd flex items-end gap-[2px] p-1.5 ${className}`} aria-label="Spectrum" role="img">
      {Array.from({ length: 16 }, (_, b) => {
        const lit = Math.round(((bands?.[b] ?? 0) / 255) * segments)
        return (
          <div key={b} className="flex flex-1 flex-col-reverse gap-[1px]">
            {Array.from({ length: segments }, (_, s) => {
              const on = s < lit
              const colour = s >= segments - 2 ? '#f01000' : s >= segments - 4 ? '#f0e000' : '#00e000'
              return (
                <span
                  key={s}
                  className="h-[2px] w-full"
                  style={{ background: on ? colour : 'rgb(0 224 0 / 0.08)', boxShadow: on ? `0 0 4px ${colour}` : 'none' }}
                />
              )
            })}
          </div>
        )
      })}
    </div>
  )
}
