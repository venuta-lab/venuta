import type { CSSProperties, MouseEvent, ReactNode } from 'react'
import type { Style } from '../../lib/themes'

/**
 * A theme as a small live window: its rail, title, covers and player, in its own tokens. A
 * theme of yours shows as the theme it extends with its own values over it (`tokens`).
 */
export function ThemeCard(props: {
  name: string
  /** The built-in theme whose look the preview starts from. */
  preview: Style
  /** A theme of yours: its values, over the preview's. */
  tokens?: Record<string, string>
  /** It follows the album through glass: a cover's light behind the preview. */
  albumLight: boolean
  on: boolean
  onPick: () => void
  onContextMenu?: (e: MouseEvent) => void
}) {
  const own: CSSProperties = Object.fromEntries(Object.entries(props.tokens ?? {}).map(([k, v]) => [`--${k}`, v]))
  return (
    <button
      role="radio"
      aria-checked={props.on}
      onClick={props.onPick}
      onContextMenu={props.onContextMenu}
      className="group min-w-0 text-left transition-transform duration-200 hover:-translate-y-0.5"
    >
      {/* The ring is the app's own accent, outside the preview (which has the theme's). */}
      <div
        className={`keep-round rounded-xl transition-shadow ${
          props.on ? 'shadow-[0_0_0_2px_var(--color-accent)]' : 'shadow-[0_0_0_1px_var(--edge)] group-hover:shadow-[0_0_0_1px_var(--edge-strong)]'
        }`}
      >
      <div
        data-preview={props.preview}
        className="keep-round relative aspect-[16/10] overflow-hidden rounded-xl"
        style={{ ...own, backgroundColor: 'var(--app-bg)', backgroundImage: 'var(--app-image)' }}
      >
        {props.albumLight && (
          <div
            aria-hidden
            className="absolute inset-0"
            style={{
              background:
                'radial-gradient(70% 80% at 20% 20%, #c0602e 0, transparent 60%), radial-gradient(60% 70% at 85% 70%, #2d4f8a 0, transparent 65%), #14121a',
              filter: 'blur(8px) saturate(1.3)',
              opacity: 0.85,
            }}
          />
        )}
        <div className="rail absolute bottom-1.5 left-1.5 top-1.5 flex w-[14%] flex-col items-center gap-1.5 pt-2" style={{ borderRadius: 'min(var(--r-rail), 8px)' }}>
          <span className="rail-pill h-[13%] w-[70%]" style={{ borderRadius: 'min(var(--r-rail), 6px)' }} />
          <span className="keep-round h-1 w-[45%] rounded-full bg-ink/25" />
          <span className="keep-round h-1 w-[45%] rounded-full bg-ink/25" />
        </div>
        <div className="absolute left-[21%] right-2 top-2">
          <div className="title truncate text-[15px] leading-tight text-ink">Music</div>
          <div className="mt-1.5 flex gap-1.5">
            {[0, 1, 2].map((i) => (
              <div
                key={i}
                className="aspect-square flex-1"
                style={{
                  borderRadius: 'min(var(--r-np), 6px)',
                  background: i === 0 ? 'color-mix(in oklab, var(--accent) 55%, var(--glass))' : 'var(--glass)',
                  boxShadow: 'inset 0 0 0 1px var(--edge)',
                }}
              />
            ))}
          </div>
        </div>
        <div className="np absolute bottom-1.5 left-[21%] right-1.5 flex h-[19%] items-center gap-1.5 px-2" style={{ borderRadius: 'min(var(--r-np), 8px)' }}>
          <span className="keep-round h-[55%] shrink-0 rounded-full bg-accent" style={{ aspectRatio: '1' }} />
          <span className="keep-round h-[3px] flex-1 rounded-full bg-ink/20">
            <span className="keep-round block h-full w-2/5 rounded-full bg-accent" />
          </span>
        </div>
      </div>
      </div>
      <div className="mt-2 flex items-baseline gap-2 px-0.5">
        <span className={`truncate text-[13.5px] font-medium ${props.on ? 'text-ink' : 'text-ink/80'}`}>{props.name}</span>
        {props.on && <span className="shrink-0 text-[11.5px] text-accent-ink">In use</span>}
      </div>
    </button>
  )
}

/** A card with what belongs under it (a theme of yours: its menu and notes). */
export function CardWith(props: { card: ReactNode; children?: ReactNode }) {
  return (
    <div className="relative grid min-w-0 content-start">
      {props.card}
      {props.children}
    </div>
  )
}
