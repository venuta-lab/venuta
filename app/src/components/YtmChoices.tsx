import { useState } from 'react'
import { notify } from '../lib/notify'
import { bringInYtmLikes } from '../lib/ytmAccount'
import { allAre, everything, GROUPS, rowOn, type PlayChoices, type Row, type YtmChoices } from '../lib/ytmChoiceWords'
import { Switch } from './ui'

/*
  Your YouTube Music choices as rows of switches, in the four groups: what Venuta reads from
  your account, what it tells YouTube Music, what it keeps in step, and how it plays. The same
  rows after sign-in (YtmWelcome) and in Settings, Online (YtmAccount); each shows the value
  that really holds. Above them, every account switch at once, on or off; each row can still be
  set on its own after.
*/

export function YtmChoiceRows(props: {
  choices: YtmChoices
  plays: PlayChoices
  onChoice: (change: Partial<YtmChoices>) => void
  onPlay: (change: Partial<PlayChoices>) => void
}) {
  const [bringing, setBringing] = useState(false)
  const bringIn = async () => {
    setBringing(true)
    try {
      notify(await bringInYtmLikes())
    } finally {
      setBringing(false)
    }
  }
  const row = (r: Row) => {
    const text = (
      <span className="min-w-0 flex-1">
        <span className="block text-[13.5px] font-medium">{r.name}</span>
        <span className="mt-0.5 block text-[12.5px] leading-relaxed text-ink/55">{r.line}</span>
      </span>
    )
    if (r.kind === 'action')
      return (
        <div key={r.key} className="flex items-center gap-4 rounded-xl px-3 py-2.5">
          {text}
          <button onClick={() => void bringIn()} disabled={bringing} className="btn btn-sm shrink-0">
            {bringing ? 'Bringing them in…' : 'Bring them in'}
          </button>
        </div>
      )
    const { on, disabled } = rowOn(r, props.choices, props.plays)
    const set = (v: boolean) => (r.kind === 'account' ? props.onChoice({ [r.key]: v }) : props.onPlay({ [r.key]: v }))
    return (
      <label key={r.key} className={`flex items-center gap-4 rounded-xl px-3 py-2.5 ${disabled ? 'opacity-60' : 'cursor-pointer hover:bg-ink/[0.04]'}`}>
        {text}
        <Switch on={on} onChange={set} label={r.name} disabled={disabled} />
      </label>
    )
  }
  const all = (on: boolean) => props.onChoice(everything(on))
  return (
    <div className="grid gap-4">
      <div className="flex flex-wrap items-center gap-2 px-3">
        <span className="min-w-[180px] flex-1 text-[12.5px] leading-relaxed text-ink/55">All of your account at once, or pick below. How it plays stays as it is.</span>
        <button onClick={() => all(true)} disabled={allAre(props.choices, true)} className="btn btn-sm">
          Turn everything on
        </button>
        <button onClick={() => all(false)} disabled={allAre(props.choices, false)} className="btn-ghost btn-sm">
          Everything off
        </button>
      </div>
      {GROUPS.map((g) => (
        <section key={g.title} aria-label={g.title}>
          <h3 className="px-3 text-[13px] font-semibold text-ink/60">{g.title}</h3>
          <div className="mt-1 grid gap-0.5">{g.rows.map(row)}</div>
        </section>
      ))}
    </div>
  )
}
