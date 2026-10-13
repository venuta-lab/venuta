import { useState } from 'react'
import { rate } from '../../lib/player'

/** Five stars: click to rate, click the same star again to clear. */
export function Stars(props: { path: string; stars: number; size?: number }) {
  const [hover, setHover] = useState(0)
  const size = props.size ?? 14
  const shown = hover || props.stars
  return (
    <span className="inline-flex items-center gap-0.5" onMouseLeave={() => setHover(0)} role="radiogroup" aria-label="Your rating">
      {[1, 2, 3, 4, 5].map((n) => (
        <button
          key={n}
          role="radio"
          aria-checked={props.stars === n}
          aria-label={`${n} star${n === 1 ? '' : 's'}`}
          onMouseEnter={() => setHover(n)}
          onClick={() => void rate(props.path, props.stars === n ? 0 : n)}
          className={`leading-none transition-colors ${n <= shown ? 'text-accent-ink' : 'text-ink/25 hover:text-ink/45'}`}
        >
          <svg width={size} height={size} viewBox="0 0 24 24" aria-hidden>
            <path
              d="M12 3.5l2.6 5.3 5.9.9-4.3 4.1 1 5.8L12 16.9l-5.2 2.7 1-5.8-4.3-4.1 5.9-.9L12 3.5Z"
              fill={n <= shown ? 'currentColor' : 'none'}
              stroke="currentColor"
              strokeWidth="1.5"
              strokeLinejoin="round"
            />
          </svg>
        </button>
      ))}
    </span>
  )
}
