import { motion } from 'motion/react'
import { LOOKS, SIZES, setLyricLook, useLyricLook, type Ground, type LookId, type LyricColour, type LyricWindow } from '../../lib/lyricLook'
import { LyricText } from './LyricText'

const spring = { type: 'spring', stiffness: 420, damping: 34 } as const

/** What the look tiles say: short, so each shows its whole character at a glance. */
const SAMPLE = 'Sing along'

/** The plain colour's swatch and name: white on dark, the theme's ink on a light theme. */
const plainOf = (ground: Ground) => (ground === 'dark' ? { swatch: '#ffffff', name: 'White' } : { swatch: 'var(--ink)', name: 'Ink' })

/**
 * How lyrics look in one lyrics window (the big screen or the desktop lyrics, each its own): the
 * look (each shown as itself), the colour, the size, and the script. One panel wherever it's
 * changed: in Now Playing (the app's own glass, `tone="app"`) and over the lyrics themselves
 * (dark glass, `tone="dark"`).
 */
export function LookPanel(props: {
  /** The window whose look this is. */
  win: LyricWindow
  /** What's behind that window's words (the plain colour follows it). */
  ground?: Ground
  tone: 'app' | 'dark'
  /** The playing cover's colour, for its swatch and the tiles' light. */
  coverColour: string
  /** The words are in an Indian script: offer them in Roman letters too. */
  indic: boolean
  /** The desktop lyrics: one line, or the next under it. */
  lines?: boolean
}) {
  const look = useLyricLook(props.win)
  const set = (change: Parameters<typeof setLyricLook>[1]) => setLyricLook(props.win, change)
  const plain = plainOf(props.ground ?? 'dark')
  const dark = props.tone === 'dark'
  const ink = dark ? 'text-white' : 'text-ink'
  const quiet = dark ? 'text-white/55' : 'text-ink/55'
  const well = dark ? 'bg-white/[0.08] shadow-[inset_0_0_0_1px_rgb(255_255_255/0.1)]' : 'bg-ink/[0.07] shadow-[inset_0_0_0_1px_var(--edge)]'
  const on = dark ? 'bg-white/[0.2] shadow-[inset_0_1px_0_rgb(255_255_255/0.25)]' : 'bg-ink/[0.13] shadow-[inset_0_1px_0_var(--highlight),0_1px_6px_-2px_rgb(0_0_0/0.3)]'
  const colourOf = (c: LyricColour) => (c === 'cover' ? props.coverColour : c === 'accent' ? 'var(--accent)' : plain.swatch)
  const tileColour = colourOf(look.colour)

  return (
    <div className={`grid gap-4 ${ink}`}>
      {/* The looks, each drawn in itself on a little dark stage lit by the cover. */}
      <div role="radiogroup" aria-label="Look" className="grid grid-cols-3 gap-2">
        {LOOKS.map((l) => {
          const chosen = look.look === l.id
          return (
            <button
              key={l.id}
              role="radio"
              aria-checked={chosen}
              title={l.line}
              onClick={() => set({ look: l.id as LookId })}
              className="group grid gap-1.5 text-left"
            >
              <span
                className="relative grid h-[58px] place-items-center overflow-hidden rounded-[14px] px-1.5 transition-transform duration-200 group-hover:-translate-y-0.5"
                style={{
                  background: `radial-gradient(120% 110% at 30% 0%, color-mix(in oklab, ${props.coverColour} 34%, #0d0d12), #07070a 78%)`,
                  boxShadow: chosen ? `0 0 0 2px ${dark ? '#fff' : 'var(--accent)'}, 0 8px 20px -10px rgb(0 0 0 / 0.7)` : 'inset 0 0 0 1px rgb(255 255 255 / 0.08)',
                }}
              >
                <LyricText text={SAMPLE} look={l.id as LookId} now progress={0.55} colour={tileColour} px={15} />
              </span>
              <span className={`px-0.5 text-[12px] font-medium ${chosen ? ink : quiet}`}>{l.name}</span>
            </button>
          )
        })}
      </div>

      <div className="grid grid-cols-[auto_1fr] items-center gap-x-4 gap-y-3 text-[12.5px]">
        <span className={quiet}>Colour</span>
        <div role="radiogroup" aria-label="Colour" className="flex items-center gap-2.5">
          {(['cover', 'accent', 'white'] as LyricColour[]).map((c) => {
            const chosen = look.colour === c
            const name = c === 'cover' ? "The cover's colour" : c === 'accent' ? 'The accent' : plain.name
            return (
              <button
                key={c}
                role="radio"
                aria-checked={chosen}
                aria-label={name}
                title={name}
                onClick={() => set({ colour: c })}
                className="relative grid h-7 w-7 place-items-center rounded-full transition-transform hover:scale-110"
                style={{
                  background: colourOf(c),
                  boxShadow: chosen
                    ? `0 0 0 2px ${dark ? '#000' : 'var(--glass)'}, 0 0 0 4px ${dark ? '#fff' : 'var(--ink)'}`
                    : 'inset 0 0 0 1px rgb(0 0 0 / 0.25)',
                }}
              >
                {c === 'cover' && (
                  // A tiny record: this colour comes from the playing cover.
                  <span className="h-2 w-2 rounded-full bg-black/35 ring-2 ring-white/40" aria-hidden />
                )}
              </button>
            )
          })}
          <span className={`ml-1 truncate ${quiet}`}>{look.colour === 'cover' ? 'From the cover' : look.colour === 'accent' ? 'The accent' : plain.name}</span>
        </div>

        <span className={quiet}>Size</span>
        <div role="radiogroup" aria-label="Size" className={`relative inline-flex w-max rounded-full p-1 ${well}`}>
          {SIZES.map((s, i) => {
            const chosen = look.size === s.id
            return (
              <button
                key={s.id}
                role="radio"
                aria-checked={chosen}
                aria-label={s.name}
                title={s.name}
                onClick={() => set({ size: s.id })}
                className={`relative grid h-7 w-9 place-items-end justify-center rounded-full pb-[5px] font-semibold leading-none ${chosen ? ink : quiet}`}
              >
                {chosen && <motion.span layoutId={`size-${props.tone}-${props.win}`} transition={spring} className={`absolute inset-0 rounded-full ${on}`} />}
                <span className="relative" style={{ fontSize: 10 + i * 2.5 }}>
                  A
                </span>
              </button>
            )
          })}
        </div>

        {props.lines && (
          <>
            <span className={quiet}>Lines</span>
            <div role="radiogroup" aria-label="Lines" className={`relative inline-flex w-max rounded-full p-1 ${well}`}>
              {([1, 2] as const).map((n) => {
                const chosen = look.lines === n
                return (
                  <button
                    key={n}
                    role="radio"
                    aria-checked={chosen}
                    onClick={() => set({ lines: n })}
                    className={`relative rounded-full px-3 py-1 font-medium ${chosen ? ink : quiet}`}
                  >
                    {chosen && <motion.span layoutId={`lines-${props.tone}-${props.win}`} transition={spring} className={`absolute inset-0 rounded-full ${on}`} />}
                    <span className="relative">{n === 1 ? 'The line' : 'And the next'}</span>
                  </button>
                )
              })}
            </div>
          </>
        )}

        {props.indic && (
          <>
            <span className={quiet}>Script</span>
            <button
              role="switch"
              aria-checked={look.latin}
              onClick={() => set({ latin: !look.latin })}
              className="flex w-max items-center gap-2.5"
              title="Lines in an Indian script also shown in Roman letters (T on the big screen)"
            >
              <span
                className={`relative h-5 w-9 rounded-full transition-colors ${look.latin ? (dark ? 'bg-white' : 'accent-fill') : dark ? 'bg-white/20' : 'bg-ink/[0.16]'}`}
              >
                <motion.span
                  animate={{ x: look.latin ? 16 : 0 }}
                  transition={spring}
                  className={`absolute left-0.5 top-0.5 h-4 w-4 rounded-full shadow ${look.latin && dark ? 'bg-black' : 'bg-white'}`}
                />
              </span>
              <span className={look.latin ? ink : quiet}>Roman letters too</span>
            </button>
          </>
        )}
      </div>
    </div>
  )
}

/**
 * The same choices in one row, for the desktop lyrics (a short window): the look stepped
 * through, the colour, the size, one line or two.
 */
export function LookStrip(props: { win: LyricWindow; ground?: Ground; coverColour: string; indic: boolean }) {
  const look = useLyricLook(props.win)
  const set = (change: Parameters<typeof setLyricLook>[1]) => setLyricLook(props.win, change)
  const plain = plainOf(props.ground ?? 'dark')
  const s = SIZES.findIndex((x) => x.id === look.size)
  const btn = 'grid h-7 min-w-7 place-items-center rounded-full px-1.5 text-white/75 transition-colors hover:bg-white/15 hover:text-white disabled:opacity-30'
  const rule = <span className="mx-1 h-4 w-px bg-white/15" aria-hidden />
  return (
    <div className="flex items-center gap-0.5 text-[12.5px] text-white">
      <select
        value={look.look}
        onChange={(e) => set({ look: e.target.value as LookId })}
        aria-label="Look"
        title={LOOKS.find((l) => l.id === look.look)?.line}
        className="h-7 cursor-pointer rounded-full bg-white/10 px-2 text-[12.5px] font-medium text-white outline-none hover:bg-white/15"
      >
        {LOOKS.map((l) => (
          <option key={l.id} value={l.id} className="bg-neutral-900 text-white">
            {l.name}
          </option>
        ))}
      </select>
      {rule}
      {(['cover', 'accent', 'white'] as LyricColour[]).map((c) => (
        <button
          key={c}
          aria-label={c === 'cover' ? "The cover's colour" : c === 'accent' ? 'The accent' : plain.name}
          aria-pressed={look.colour === c}
          onClick={() => set({ colour: c })}
          className="mx-0.5 h-[18px] w-[18px] rounded-full transition-transform hover:scale-110"
          style={{
            background: c === 'cover' ? props.coverColour : c === 'accent' ? 'var(--accent)' : plain.swatch,
            boxShadow: look.colour === c ? '0 0 0 2px #000, 0 0 0 3.5px #fff' : 'inset 0 0 0 1px rgb(0 0 0 / 0.3)',
          }}
        />
      ))}
      {rule}
      <button onClick={() => set({ size: SIZES[s - 1].id })} disabled={s <= 0} className={`${btn} text-[11px]`} aria-label="Smaller">
        A
      </button>
      <button onClick={() => set({ size: SIZES[s + 1].id })} disabled={s >= SIZES.length - 1} className={`${btn} text-[15px]`} aria-label="Larger">
        A
      </button>
      {rule}
      <button onClick={() => set({ lines: look.lines === 1 ? 2 : 1 })} className={`${btn} px-2.5`} title="One line, or the next one under it">
        {look.lines === 1 ? '1 line' : '2 lines'}
      </button>
      {props.indic && (
        <button
          onClick={() => set({ latin: !look.latin })}
          aria-pressed={look.latin}
          className={`${btn} px-2.5 ${look.latin ? 'bg-white/20 text-white' : ''}`}
          title="Also in Roman letters"
        >
          Abc
        </button>
      )}
    </div>
  )
}
