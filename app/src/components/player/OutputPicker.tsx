import { useEffect, useState } from 'react'
import { Switch } from '../ui'
import { outputChoices, setOutput, setOutputDelay, type Signal } from '../../lib/player'
import {
  current,
  delayWords,
  deviceLine,
  followChoice,
  groups,
  isChosen,
  isPlaying,
  noteOf,
  stateLine,
  suggestion,
  type OutputChoices,
  type OutputDevice,
} from '../../lib/outputPicker'

/*
  Where the sound goes (docs/design/devices.md): Follow Windows on top, then every device by kind
  (Bluetooth, USB, wired, HDMI, virtual) with a clear name, a Bluetooth device's stereo and
  hands-free endpoints as one; the device the music is on with its state (bit-perfect or shared,
  its format) and its delay (what Windows measured, and an extra you set for Bluetooth or a
  receiver, which lyrics and rooms follow); and exclusive mode. In the quality badge's popover
  and in Settings > Playback. `choices` given (the dev preview) shows them without the engine.
*/

export function OutputPicker(props: { signal: Signal | null; choices?: OutputChoices; roomy?: boolean }) {
  const [c, setC] = useState<OutputChoices | null>(props.choices ?? null)
  const endpoint = props.signal?.endpoint ?? null
  // Listed again when the music moves to another output (a device came or went).
  useEffect(() => {
    if (props.choices) return
    let live = true
    void outputChoices().then((x) => live && setC(x))
    return () => {
      live = false
    }
  }, [props.choices, endpoint])
  if (!c) return null
  const following = c.device === null
  const choose = (device: string | null, name?: string) => {
    setC({ ...c, device })
    void setOutput(device, c.exclusive, name)
  }
  const now = current(c)
  return (
    <div className={props.roomy ? 'text-[13px]' : 'text-[12.5px]'}>
      <label className="flex cursor-pointer items-center gap-3 rounded-lg px-2 py-1.5 hover:bg-ink/[0.06]">
        <span className="min-w-0 flex-1">
          <span className="block text-ink/85">Follow Windows</span>
          <span className="block text-[11.5px] text-ink/50">Play on Windows' default output, and move with it.</span>
        </span>
        <Switch
          label="Follow Windows"
          on={following}
          onChange={(on) => {
            const id = followChoice(c, on)
            choose(id, c.devices.find((d) => d.id === id)?.name)
          }}
        />
      </label>
      <Note signal={props.signal} />
      <div className="mt-1">
        {groups(c.devices).map((g) => (
          <div key={g.kind} className="mt-2 first:mt-1">
            <div className="px-2 pb-0.5 text-[10.5px] font-semibold uppercase tracking-[0.06em] text-ink/45">{g.title}</div>
            {g.devices.map((d) => (
              <DeviceRow key={d.id} c={c} d={d} following={following} onChoose={() => choose(d.id, d.name)} />
            ))}
          </div>
        ))}
      </div>
      {now && <NowOn device={now} signal={isPlaying(c, now) ? props.signal : null} onDelay={(ms) => {
        setC({ ...c, devices: c.devices.map((d) => (d.id === now.id ? { ...d, delay_ms: ms } : d)) })
        void setOutputDelay(now.id, ms)
      }} />}
      <label className="mt-2 flex cursor-pointer items-center gap-3 rounded-lg px-2 py-1.5 hover:bg-ink/[0.06]">
        <span className="min-w-0 flex-1">
          <span className="block text-ink/85">Exclusive (bit-perfect)</span>
          <span className="block text-[11.5px] text-ink/50">
            Each song at its own rate and depth, untouched, where the device takes it. Other apps can't play sound meanwhile. Bluetooth and virtual devices always share.
          </span>
        </span>
        <Switch
          label="Exclusive (bit-perfect)"
          on={c.exclusive}
          onChange={(on) => {
            setC({ ...c, exclusive: on })
            void setOutput(c.device, on)
          }}
        />
      </label>
    </div>
  )
}

/** What Venuta did about the choice: a twin, a fallback, a device that sounds poor. */
function Note(props: { signal: Signal | null }) {
  const n = noteOf(props.signal)
  if (!n) return null
  return (
    <p role={n.warn ? 'alert' : undefined} className={`mx-2 mt-1.5 rounded-lg px-2.5 py-1.5 text-[11.5px] leading-snug ${n.warn ? 'bg-fix/10 text-fix' : 'bg-ink/[0.05] text-ink/70'}`}>
      {n.text}
    </p>
  )
}

function DeviceRow(props: { c: OutputChoices; d: OutputDevice; following: boolean; onChoose: () => void }) {
  const { c, d } = props
  const chosen = isChosen(c, d) || (props.following && d.default)
  const playing = isPlaying(c, d)
  const line = deviceLine(c, d)
  return (
    <button
      onClick={props.onChoose}
      disabled={!d.connected}
      aria-pressed={isChosen(c, d)}
      className={`flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-left hover:bg-ink/[0.06] disabled:cursor-default disabled:opacity-60 ${chosen ? 'text-accent-ink' : 'text-ink/85'}`}
    >
      <span className={`keep-round h-1.5 w-1.5 shrink-0 rounded-full ${playing ? 'bg-good' : 'bg-transparent'}`} aria-hidden title={playing ? 'Playing here' : undefined} />
      <span className="min-w-0 flex-1">
        <span className="block truncate">{d.name}</span>
        {line && <span className={`block truncate text-[11px] ${d.hands_free_only ? 'text-fix' : 'text-ink/50'}`}>{line}</span>}
      </span>
      {isChosen(c, d) && <span aria-hidden>✓</span>}
    </button>
  )
}

/** The device the music is on: its state and its delay. */
function NowOn(props: { device: OutputDevice; signal: Signal | null; onDelay: (ms: number | null) => void }) {
  const d = props.device
  const s = props.signal
  const [sliding, setSliding] = useState<number | null>(null)
  const set = d.delay_ms
  const ms = sliding ?? set ?? 0
  const offer = suggestion(d)
  const done = () => {
    if (sliding !== null) props.onDelay(sliding)
    setSliding(null)
  }
  return (
    <div className="mx-2 mt-3 rounded-xl bg-ink/[0.05] px-3 py-2.5">
      <div className="truncate font-medium text-ink/85">{d.name}</div>
      {s && <div className={`mt-0.5 text-[11.5px] ${s.bit_perfect ? 'text-good' : 'text-ink/60'}`}>{stateLine(s)}</div>}
      <div className="mt-2 flex items-baseline justify-between gap-2 text-[11.5px]">
        <span className="text-ink/55">Delay</span>
        <span className="text-ink/75">{delayWords(s ? s.latency_ms : null, sliding ?? set)}</span>
      </div>
      <input
        type="range"
        min={-500}
        max={500}
        step={5}
        value={ms}
        onChange={(e) => setSliding(Number(e.target.value))}
        onPointerUp={done}
        onKeyUp={done}
        aria-label={`${d.name}'s extra delay`}
        className="mt-1 h-1 w-full accent-[var(--accent)]"
      />
      <div className="mt-1.5 flex flex-wrap items-center gap-2 text-[11px] text-ink/50">
        <span className="flex-1">How much later than Windows says you hear it. Lyrics and Listen together follow it.</span>
        {offer && (
          <button onClick={() => props.onDelay(offer.ms)} className="btn btn-sm" title={offer.says}>
            Use {offer.ms} ms
          </button>
        )}
        {set !== null && (
          <button onClick={() => props.onDelay(null)} className="btn btn-sm">
            Clear
          </button>
        )}
      </div>
    </div>
  )
}
