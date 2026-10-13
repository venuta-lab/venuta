import { motion } from 'motion/react'
import { useEffect, useRef, useState } from 'react'
import { setSleep, setSound, setStopAfter, usePlayer, SOUND_OFF, type Sound } from '../../lib/player'

const spring = { type: 'spring', stiffness: 380, damping: 34 } as const
const BAND_LABELS = ['31', '62', '125', '250', '500', '1k', '2k', '4k', '8k', '16k']

/** Equalizer starting points; any slider moved makes it "Custom". */
const PRESETS: [string, number[]][] = [
  ['Flat', [0, 0, 0, 0, 0, 0, 0, 0, 0, 0]],
  ['Bass boost', [6, 5, 4, 2, 0, 0, 0, 0, 0, 0]],
  ['Vocal', [-2, -1, 0, 2, 4, 4, 3, 1, 0, -1]],
  ['Treble boost', [0, 0, 0, 0, 0, 1, 2, 4, 5, 6]],
  ['Loudness', [5, 4, 2, 0, -1, -1, 0, 2, 4, 5]],
  ['Classical', [3, 2, 1, 0, 0, 0, -1, 0, 2, 3]],
  ['Electronic', [5, 4, 1, 0, -2, 1, 0, 2, 4, 5]],
  ['Acoustic', [3, 3, 2, 1, 1, 1, 2, 2, 2, 1]],
]

function Switch(props: { on: boolean; onChange: (on: boolean) => void; label: string }) {
  return (
    <button
      role="switch"
      aria-checked={props.on}
      aria-label={props.label}
      onClick={() => props.onChange(!props.on)}
      className={`relative h-6 w-11 shrink-0 rounded-full transition-colors ${props.on ? 'bg-accent' : 'bg-ink/20'}`}
    >
      <motion.span layout transition={spring} className={`absolute top-0.5 h-5 w-5 rounded-full bg-white shadow ${props.on ? 'right-0.5' : 'left-0.5'}`} />
    </button>
  )
}

function Segmented<T extends string>(props: { value: T; options: [T, string][]; onChange: (v: T) => void; name: string }) {
  return (
    <div role="radiogroup" aria-label={props.name} className="inline-flex flex-wrap rounded-full bg-ink/[0.08] p-1">
      {props.options.map(([id, label]) => (
        <button
          key={id}
          role="radio"
          aria-checked={props.value === id}
          onClick={() => props.onChange(id)}
          className={`relative rounded-full px-3 py-1 text-[12px] ${props.value === id ? 'text-ink' : 'text-ink/60 hover:text-ink'}`}
        >
          {props.value === id && <motion.span layoutId={`seg-${props.name}`} transition={spring} className="absolute inset-0 rounded-full bg-ink/[0.14]" />}
          <span className="relative">{label}</span>
        </button>
      ))}
    </div>
  )
}

/** A dB slider: -12 to +12, double-click to reset. */
function Db(props: { label: string; value: number; onChange: (v: number) => void; vertical?: boolean; disabled?: boolean }) {
  const shown = `${props.value > 0 ? '+' : ''}${props.value.toFixed(1)}`
  if (props.vertical)
    return (
      <label className={`flex flex-col items-center gap-1.5 ${props.disabled ? 'opacity-40' : ''}`}>
        <span className="text-[10.5px] tabular-nums text-ink/60">{shown}</span>
        <input
          type="range"
          min={-12}
          max={12}
          step={0.5}
          value={props.value}
          disabled={props.disabled}
          onChange={(e) => props.onChange(Number(e.target.value))}
          onDoubleClick={() => props.onChange(0)}
          aria-label={`${props.label} Hz`}
          className="h-28 w-5 accent-[var(--accent)] [writing-mode:vertical-lr] [direction:rtl]"
        />
        <span className="text-[11px] text-ink/55">{props.label}</span>
      </label>
    )
  return (
    <label className="flex items-center gap-3">
      <span className="w-20 text-[12.5px] text-ink/70">{props.label}</span>
      <input
        type="range"
        min={-12}
        max={12}
        step={0.5}
        value={props.value}
        onChange={(e) => props.onChange(Number(e.target.value))}
        onDoubleClick={() => props.onChange(0)}
        aria-label={props.label}
        className="h-1 flex-1 accent-[var(--accent)]"
      />
      <span className="w-12 text-right text-[12px] tabular-nums text-ink/60">{shown} dB</span>
    </label>
  )
}

function Card(props: { title: string; right?: React.ReactNode; children?: React.ReactNode }) {
  return (
    <section className="glass min-w-0 rounded-2xl px-4 py-3.5">
      <div className="flex items-center gap-3">
        <h3 className="flex-1 text-[14px] font-medium">{props.title}</h3>
        {props.right}
      </div>
      {props.children && <div className="mt-3">{props.children}</div>}
    </section>
  )
}

/**
 * The sound tools, Poweramp-style, in the player itself. All off by default (bit-perfect);
 * any of them on changes the sound, and the badge and signal path say so.
 */
export function SoundPanel() {
  const s = usePlayer()
  // Sliders move freely; the engine hears about it a moment later.
  const [draft, setDraft] = useState<Sound>(s.sound)
  const timer = useRef(0)
  const synced = useRef(s.sound)
  useEffect(() => {
    if (s.sound !== synced.current && JSON.stringify(s.sound) !== JSON.stringify(draft)) {
      synced.current = s.sound
      setDraft(s.sound)
    }
  }, [s.sound, draft])
  const change = (next: Sound) => {
    setDraft(next)
    window.clearTimeout(timer.current)
    timer.current = window.setTimeout(() => {
      synced.current = next
      void setSound(next)
    }, 120)
  }
  const band = (i: number, v: number) => change({ ...draft, eq: true, preset: 'Custom', bands: draft.bands.map((b, k) => (k === i ? v : b)) })
  // From the settings themselves (there's no signal while nothing plays).
  const changed =
    (draft.eq && draft.bands.some((b) => Math.abs(b) > 0.05)) ||
    Math.abs(draft.preamp_db) > 0.05 ||
    Math.abs(draft.bass_db) > 0.05 ||
    Math.abs(draft.treble_db) > 0.05 ||
    draft.leveling !== 'off'
  const sleepMin = s.sleep_in_s !== null ? Math.ceil(s.sleep_in_s / 60) : null

  return (
    <div className="grid min-w-0 grid-cols-1 gap-3 pb-6 text-[13px]">
      <p className={`text-[12.5px] ${changed ? 'text-fix' : 'text-ink/55'}`}>
        {changed
          ? 'These change the sound on the way (Signal shows each step). Turn them off for bit-perfect playback.'
          : 'Everything here is off: you hear the file exactly as it is. Any of these changes the sound.'}
      </p>

      <Card title="Equalizer" right={<Switch label="Equalizer" on={draft.eq} onChange={(eq) => change({ ...draft, eq })} />}>
        <div className="no-scrollbar -mx-1 flex gap-1 overflow-x-auto px-1 pb-1">
          {PRESETS.map(([name, bands]) => (
            <button
              key={name}
              onClick={() => change({ ...draft, eq: true, preset: name, bands })}
              className={`shrink-0 rounded-full px-3 py-1 text-[12px] ${draft.eq && draft.preset === name ? 'bg-accent text-on-accent' : 'bg-ink/[0.07] text-ink/70 hover:bg-ink/[0.12]'}`}
            >
              {name}
            </button>
          ))}
        </div>
        <div className="mt-3 flex justify-between gap-1">
          {draft.bands.map((v, i) => (
            <Db key={i} vertical label={BAND_LABELS[i]} value={v} disabled={!draft.eq} onChange={(x) => band(i, x)} />
          ))}
        </div>
        <div className="mt-3 grid gap-2">
          <Db label="Preamp" value={draft.preamp_db} onChange={(preamp_db) => change({ ...draft, preamp_db })} />
        </div>
      </Card>

      <Card title="Tone">
        <div className="grid gap-2">
          <Db label="Bass" value={draft.bass_db} onChange={(bass_db) => change({ ...draft, bass_db })} />
          <Db label="Treble" value={draft.treble_db} onChange={(treble_db) => change({ ...draft, treble_db })} />
        </div>
      </Card>

      <Card
        title="Volume levelling"
        right={
          <Segmented
            name="leveling"
            value={draft.leveling}
            options={[
              ['off', 'Off'],
              ['track', 'Per song'],
              ['album', 'Per album'],
            ]}
            onChange={(leveling) => change({ ...draft, leveling })}
          />
        }
      >
        <p className="text-[12px] text-ink/55">
          Evens out loudness using ReplayGain values in the files. Per album keeps the differences within an album. Songs without values
          play as they are; the quality check in Health measures them.
        </p>
        {draft.leveling !== 'off' && (
          <div className="mt-2">
            <Db label="Preamp" value={draft.leveling_preamp_db} onChange={(leveling_preamp_db) => change({ ...draft, leveling_preamp_db })} />
          </div>
        )}
      </Card>

      <Card title="Limiter" right={<Switch label="Limiter" on={draft.limiter} onChange={(limiter) => change({ ...draft, limiter })} />}>
        <p className="text-[12px] text-ink/55">Keeps boosts from clipping. Only acts when something above makes the sound louder.</p>
      </Card>

      <Card title="Sleep timer" right={sleepMin !== null ? <span className="text-[12px] text-accent-ink">{sleepMin} min left</span> : undefined}>
        <div className="flex flex-wrap gap-1.5">
          {[15, 30, 45, 60, 90].map((m) => (
            <button key={m} onClick={() => void setSleep(m)} className="rounded-full bg-ink/[0.07] px-3 py-1 text-[12px] text-ink/75 hover:bg-ink/[0.12]">
              {m} min
            </button>
          ))}
          {sleepMin !== null && (
            <button onClick={() => void setSleep(null)} className="rounded-full px-3 py-1 text-[12px] text-ink/60 hover:bg-ink/10">
              Cancel
            </button>
          )}
        </div>
        <label className="mt-3 flex cursor-pointer items-center gap-3">
          <Switch label="Stop after this song" on={s.stop_after_current} onChange={(on) => void setStopAfter(on)} />
          <span className="text-ink/75">Stop after this song</span>
        </label>
      </Card>

      <button onClick={() => change(SOUND_OFF)} className="justify-self-start rounded-full px-3 py-1.5 text-[12.5px] text-ink/60 hover:bg-ink/10 hover:text-ink">
        Turn everything off (bit-perfect)
      </button>
    </div>
  )
}
