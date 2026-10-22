import type { CSSProperties } from 'react'
import type { Ground, LookId } from '../../lib/lyricLook'

/** Words not yet sung, and lines around the one being sung. */
const DIM: Record<Ground, string> = { dark: 'rgb(255 255 255 / 0.42)', light: 'rgb(0 0 0 / 0.36)' }
const SHADOW: Record<Ground, string> = {
  dark: '0 2px 14px rgb(0 0 0 / 0.55), 0 1px 3px rgb(0 0 0 / 0.5)',
  light: '0 1px 2px rgb(255 255 255 / 0.5)',
}

/**
 * A line filling in as it is sung: by letters, word after word, so a line that wraps fills its
 * first row and then its second, as it is read.
 */
function Sung(props: { text: string; progress: number; colour: string; dim: string }) {
  const parts = props.text.split(/(\s+)/)
  const letters = props.text.replace(/\s+/g, '').length || 1
  let done = props.progress * letters
  return (
    <>
      {parts.map((w, i) => {
        if (!w.trim()) return w
        const share = Math.min(1, Math.max(0, done / w.length))
        done -= w.length
        const pct = Math.round(share * 1000) / 10
        return (
          <span
            key={i}
            style={{
              color: 'transparent',
              backgroundImage: `linear-gradient(90deg, ${props.colour} ${pct}%, ${props.dim} ${pct}%)`,
              backgroundClip: 'text',
              WebkitBackgroundClip: 'text',
            }}
          >
            {w}
          </span>
        )
      })}
    </>
  )
}

/** A row under a line: its pronunciation or its translation ('' keeps the row, empty, so every
 *  line of a song has the same rows). `progress`: the pronunciation's own karaoke fill. */
export interface SecondRowView {
  kind: 'roman' | 'translation'
  text: string
  progress?: number
  /** Its language (BCP 47), for the type and for screen readers. */
  lang?: string
}

/** The second rows' sizes and gaps, as a share of the line's size (lib/lyricRows.ts counts them),
 *  never under a size that still reads (the windows' fit then makes room), never over the line's. */
const SECOND = { roman: { size: 0.52, gap: 0.22 }, translation: { size: 0.46, gap: 0.16 } }
const SECOND_MIN_PX = 11

/**
 * One line of lyrics in a look. `now` is the line being sung; `progress` (0 to 1) how far through
 * it the song is (the karaoke fill); `colour` the look's colour as CSS. `seconds`, when given,
 * are the rows under it, in order: pronunciation, then translation, smaller and dimmer, wrapped.
 */
export function LyricText(props: {
  text: string
  seconds?: SecondRowView[]
  /** The line's language (BCP 47). */
  lang?: string
  look: LookId
  now: boolean
  progress?: number
  colour: string
  /** Font size in px. */
  px: number
  align?: 'center' | 'left'
  className?: string
  /** What's behind the words (dark unless said). */
  ground?: Ground
  /** Break inside words where nothing else fits (a line already as small as it may be). */
  breakAnywhere?: boolean
}) {
  const { look, now, colour } = props
  const g = props.ground ?? 'dark'
  const dim = DIM[g]
  const p = Math.round((props.progress ?? (now ? 1 : 0)) * 1000) / 10
  const base: CSSProperties = {
    fontSize: props.px,
    lineHeight: 1.18,
    textAlign: props.align ?? 'center',
    textWrap: 'balance',
    overflowWrap: props.breakAnywhere ? 'anywhere' : undefined,
    letterSpacing: '-0.01em',
    fontWeight: 700,
    fontFamily: 'var(--font-ui)',
    color: now ? colour : dim,
    textShadow: SHADOW[g],
    transition: 'color 300ms ease, opacity 300ms ease',
  }
  const style: CSSProperties = { ...base }
  switch (look) {
    case 'karaoke':
      // Filled word by word in reading order (below), however the line wraps.
      if (now) Object.assign(style, { color: dim, textShadow: 'none', filter: g === 'dark' ? 'drop-shadow(0 2px 8px rgb(0 0 0 / 0.55))' : 'none', transition: 'none' })
      break
    case 'glow':
      if (now)
        style.textShadow = `0 0 18px ${colour}, 0 0 46px color-mix(in oklab, ${colour} 70%, transparent)${g === 'dark' ? ', 0 2px 4px rgb(0 0 0 / 0.5)' : ''}`
      // Lit from inside: towards white on dark, towards black on light (it still reads).
      if (now) style.color = `color-mix(in oklab, ${colour} ${g === 'dark' ? '35%, white' : '65%, black'})`
      break
    case 'outline':
      Object.assign(style, {
        color: now ? '#ffffff' : 'rgb(255 255 255 / 0.6)',
        WebkitTextStroke: `${Math.max(2, props.px / 16)}px ${now ? 'rgb(0 0 0 / 0.85)' : 'rgb(0 0 0 / 0.55)'}`,
        paintOrder: 'stroke fill',
        textShadow: now ? `0 0 0.5em color-mix(in oklab, ${colour} 55%, transparent)` : 'none',
      })
      break
    case 'serif':
      Object.assign(style, {
        fontFamily: '"Shippori Mincho B1", Georgia, "Times New Roman", serif',
        fontWeight: 800,
        letterSpacing: '0',
        lineHeight: 1.3,
      })
      break
    case 'theme':
      Object.assign(style, {
        fontFamily: 'var(--font-display)',
        fontWeight: 'var(--title-weight)' as unknown as number,
        letterSpacing: 'var(--title-tracking)',
        textTransform: 'var(--title-case)' as CSSProperties['textTransform'],
      })
      break
    case 'clean':
      break
  }
  return (
    <div className={props.className}>
      <div style={style} lang={props.lang}>
        {look === 'karaoke' && now && props.text ? <Sung text={props.text} progress={p / 100} colour={colour} dim={dim} /> : props.text || '♪'}
      </div>
      {props.seconds?.map((r) => {
        const roman = r.kind === 'roman'
        const { size, gap } = SECOND[r.kind]
        // Pronunciation a little brighter than the translation; both dimmer than the line.
        const tone = g === 'dark' ? (now ? (roman ? 0.85 : 0.66) : roman ? 0.4 : 0.32) : now ? (roman ? 0.72 : 0.58) : roman ? 0.36 : 0.3
        const ink = g === 'dark' ? `rgb(255 255 255 / ${tone})` : `rgb(0 0 0 / ${tone})`
        const fill = roman && look === 'karaoke' && now && !!r.text && r.progress !== undefined
        return (
          <div
            key={r.kind}
            data-second={r.kind}
            lang={r.lang}
            style={{
              ...base,
              fontSize: Math.min(props.px * 0.9, Math.max(SECOND_MIN_PX, props.px * size)),
              fontWeight: 500,
              marginTop: props.px * gap,
              color: ink,
              WebkitTextStroke: look === 'outline' ? `${Math.max(1.5, props.px / 26)}px rgb(0 0 0 / 0.8)` : undefined,
              paintOrder: 'stroke fill',
            }}
          >
            {/* An empty row keeps its height: the song's lines all have the same rows. */}
            {fill ? <Sung text={r.text} progress={r.progress ?? 0} colour={colour} dim={ink} /> : r.text || ' '}
          </div>
        )
      })}
    </div>
  )
}
