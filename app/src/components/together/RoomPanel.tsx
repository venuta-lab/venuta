import { useEffect, useState } from 'react'
import { lineAt, type Lyrics } from '../../lib/lyrics'
import { songLength } from '../../lib/player'
import { TOGETHER, type Role } from '../../lib/together'
import type { MemberView, TogetherViewV2 } from '../../lib/togetherView'
import { IconNext, IconPause, IconPlay, IconPrevious, IconVolume } from '../Icons'
import { RemoteArt } from '../RemoteArt'
import { Switch } from '../ui'
import { Initial } from './Initial'
import {
  END_ASK,
  handOverChoices,
  inviteWords,
  joinChoice,
  joiningWords,
  may,
  meOf,
  modeSwitches,
  modeWord,
  newRoleValue,
  outputPanel,
  progress,
  requestsShown,
  rows,
  screenOf,
  startWords,
  statusLines,
  stepButton,
  transitionKey,
  transitionOptions,
  versionNote,
  doorWords,
  delayWords,
  type InvitePreview,
  type RoomActions,
  type Row,
} from './model'

/*
  The room panel v2 (design, WP12 notes): what the chip in the title bar opens. It draws what
  ./model.ts decides for the view, and asks for everything through `act`, so it is drawn the
  same in the app and in the preview page. Plain sentence case, no em dashes.
*/

/** What the panel needs besides the view: time since it came, and the lyrics of the room's song. */
export interface PanelEnv {
  since: () => number
  lyrics: Lyrics | null
  /** What starts open (the preview, to photograph the asks and a row's numbers). */
  start?: { ask?: 'end' | 'hand'; open?: number }
}

type P = { v: TogetherViewV2; act: RoomActions; env: PanelEnv }

export function RoomPanel(props: P & { offered?: string | null }) {
  const screen = screenOf(props.v)
  if (screen === 'start') return <Start {...props} />
  if (screen === 'door') return <Door {...props} />
  if (screen === 'joining') return <Joining {...props} />
  return <Room {...props} />
}

const ROLES: [Role, string][] = [
  ['control', 'Control'],
  ['add', 'Add songs'],
  ['listen', 'Just listen'],
]

function Title({ children, side }: { children: React.ReactNode; side?: React.ReactNode }) {
  return (
    <div className="mb-2 flex items-center gap-2">
      <span className="min-w-0 flex-1 truncate text-[14px] font-semibold">{children}</span>
      {side}
    </div>
  )
}

/** A tinted note: the accent for something to answer, the warning colour for a problem. */
function Note({ tone = 'accent', children }: { tone?: 'accent' | 'warn' | 'quiet'; children: React.ReactNode }) {
  const bg =
    tone === 'warn'
      ? 'bg-[color-mix(in_oklab,var(--fix)_16%,transparent)]'
      : tone === 'quiet'
        ? 'bg-ink/[0.05]'
        : 'bg-[color-mix(in_oklab,var(--accent)_16%,transparent)]'
  return <div className={`flex items-center gap-2 rounded-xl px-2.5 py-1.5 text-[12.5px] ${bg}`}>{children}</div>
}

const Section = ({ children, first }: { children: React.ReactNode; first?: boolean }) => (
  <div className={first ? '' : 'mt-2.5 border-t border-edge pt-2.5'}>{children}</div>
)

// --- Out of a room ---------------------------------------------------------------------------

function Start({ v, act, offered }: P & { offered?: string | null }) {
  const w = startWords(v)
  const [code, setCode] = useState(() => offered ?? '')
  const [read, setRead] = useState<{ code: string; p: InvitePreview | null } | null>(null)
  const [draft, setDraft] = useState<string | null>(null)
  const name = draft ?? v.name
  // Rooms nearby are listened for only while this screen shows (A15).
  useEffect(() => {
    act.watchNearby(true)
    return () => act.watchNearby(false)
  }, [act])
  const preview = code.trim() && read?.code === code ? read : null
  useEffect(() => {
    if (!code.trim()) return
    let live = true
    act
      .readInvite(code)
      .then((p) => live && setRead({ code, p }))
      .catch(() => live && setRead({ code, p: null }))
    return () => {
      live = false
    }
  }, [code, act])
  return (
    <div className="grid gap-3 text-[13px]">
      <div>
        <div className="text-[14px] font-semibold">{TOGETHER}</div>
        <p className="mt-0.5 text-[12.5px] leading-relaxed text-ink/55">
          Friends hear what you play at the same moment, each from their best copy: their own file, the online song, or yours streamed to them.
        </p>
      </div>
      {w.resume && (
        <Note>
          <span className="min-w-0 flex-1">{w.resume.text}</span>
          <button onClick={() => act.resume(true)} className="btn btn-sm">
            {w.resume.button}
          </button>
          <button onClick={() => act.resume(false)} className="btn-ghost btn-sm">
            Not now
          </button>
        </Note>
      )}
      {w.message && (
        <div className="grid gap-1.5">
          <p className="text-[12.5px] text-ink/70">{w.message}</p>
          {w.saveQueue && (
            <button onClick={act.saveQueue} className="btn btn-sm justify-self-start">
              {w.saveQueue}
            </button>
          )}
        </div>
      )}
      {w.opening ? (
        <p className="text-ink/60" role="status">
          Opening your room...
        </p>
      ) : (
        <button onClick={act.start} className="btn-primary btn-sm justify-self-start">
          Start a room
        </button>
      )}
      <Section>
        <div className="mb-1.5 text-[12.5px] font-medium">Join a room</div>
        <div className="flex gap-2">
          <input value={code} onChange={(e) => setCode(e.target.value)} placeholder="Paste an invite" aria-label="Invite" spellCheck={false} className="field h-8 min-w-0 flex-1 rounded-full px-3 text-[12.5px]" />
          <button onClick={() => act.join(code)} disabled={!preview?.p?.open} className="btn btn-sm disabled:opacity-40">
            Join
          </button>
        </div>
        {code.trim() && preview && <p className="mt-1.5 text-[12px] text-ink/55">{inviteWords(preview.p)}</p>}
      </Section>
      {w.nearby.length > 0 && (
        <Section>
          <div className="mb-1 text-[12.5px] font-medium">Rooms nearby</div>
          <div className="divide-y divide-ink/[0.07]">
            {w.nearby.map((n) => (
              <div key={n.id} className="flex items-center gap-2 py-1.5">
                <Initial m={{ id: n.id, name: n.text }} />
                <span className="min-w-0 flex-1">
                  <span className="block truncate">{n.text}</span>
                  <span className="block text-[11.5px] text-ink/50">{n.sub}</span>
                </span>
                <button onClick={() => act.joinNearby(n.id)} className="btn btn-sm">
                  Ask to join
                </button>
              </div>
            ))}
          </div>
        </Section>
      )}
      <label className="flex items-center gap-2 border-t border-edge pt-3 text-[12.5px] text-ink/60">
        Your name
        <input
          value={name}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={() => {
            if (name.trim() && name !== v.name) act.setName(name)
            setDraft(null)
          }}
          className="field h-7 min-w-0 flex-1 rounded-full px-3 text-[12.5px] text-ink"
          aria-label="Your name"
        />
      </label>
    </div>
  )
}

function Door({ v, act }: P) {
  return (
    <div className="grid gap-2 text-[13px]">
      <div className="font-semibold">{TOGETHER}</div>
      <p className="text-ink/65" role="status">
        {doorWords(v)}
      </p>
      <button onClick={act.cancel} className="btn-ghost btn-sm justify-self-end">
        Cancel
      </button>
    </div>
  )
}

function Joining({ v, act }: P) {
  const w = joiningWords(v)
  const c = joinChoice(v)
  return (
    <div className="grid gap-2.5 text-[13px]">
      <div className="font-semibold">{w.title}</div>
      <div role="status">
        <p className="text-[12.5px] text-ink/70">{w.line}</p>
        {w.pct != null && (
          <div className="mt-1.5 h-1 overflow-hidden rounded-full bg-ink/10" aria-hidden>
            <div className="h-full rounded-full bg-accent" style={{ width: `${w.pct}%` }} />
          </div>
        )}
      </div>
      {c && (
        <div className="grid gap-1.5" role="radiogroup" aria-label="How you listen">
          <Choice on={c.selected === 'speaker'} label={c.speaker} says={c.speakerSays} pick={() => act.setMode('one_speaker')} />
          <Choice on={c.selected === 'here'} label={c.here} says={c.hereWhyNot ?? c.hereSays} off={!!c.hereWhyNot} pick={() => act.setMode(c.hereMode)} />
        </div>
      )}
      {(!c || c.selected === 'here') && (
        <button onClick={act.startWithNext} className="btn-ghost btn-sm justify-self-start" title="Silence until the room's next song, then a clean start">
          Start with the next song
        </button>
      )}
      <button onClick={act.cancel} className="btn-ghost btn-sm justify-self-end">
        Cancel
      </button>
    </div>
  )
}

function Choice(props: { on: boolean; label: string; says: string; off?: boolean; pick: () => void }) {
  return (
    <button
      role="radio"
      aria-checked={props.on}
      disabled={props.off}
      onClick={props.pick}
      className={`flex items-start gap-2.5 rounded-xl px-3 py-2 text-left transition-colors disabled:opacity-45 ${
        props.on ? 'bg-[color-mix(in_oklab,var(--accent)_16%,transparent)] shadow-[inset_0_0_0_1.5px_var(--accent)]' : 'bg-ink/[0.05] hover:bg-ink/[0.08]'
      }`}
    >
      <span className={`mt-0.5 grid h-4 w-4 shrink-0 place-items-center rounded-full ${props.on ? 'bg-accent' : 'shadow-[inset_0_0_0_1.5px_color-mix(in_oklab,var(--ink)_40%,transparent)]'}`} aria-hidden>
        {props.on && <span className="h-1.5 w-1.5 rounded-full bg-[var(--on-accent)]" />}
      </span>
      <span className="min-w-0">
        <span className="block text-[13px] font-medium">{props.label}</span>
        <span className="block text-[12px] text-ink/55">{props.says}</span>
      </span>
    </button>
  )
}

// --- In a room --------------------------------------------------------------------------------

function Room({ v, act, env }: P) {
  const room = v.room!
  const m = may(v)
  const me = meOf(v)
  const [qr, setQr] = useState(false)
  const roleWords = v.host ? null : me?.role === 'control' ? 'You can control' : me?.role === 'add' ? 'You can add songs' : 'Just listening'
  return (
    <div className="text-[13px]">
      {v.host ? (
        <Title
          side={
            <>
              <button onClick={() => setQr((q) => !q)} aria-expanded={qr} className="btn-ghost btn-sm">
                QR
              </button>
              <button onClick={act.copyInvite} className="btn btn-sm">
                Copy invite
              </button>
            </>
          }
        >
          {TOGETHER} · {room.members.length}
        </Title>
      ) : (
        <Title side={<span className="text-[11.5px] text-ink/50">{roleWords}</span>}>
          With {room.host_name} · {room.members.length}
        </Title>
      )}
      {qr && v.invite && (
        <div className="mb-2 grid justify-items-center gap-1 rounded-xl bg-white p-2">
          <div className="h-[180px] w-[180px] [&>svg]:h-full [&>svg]:w-full" dangerouslySetInnerHTML={{ __html: v.invite.qr_svg }} />
          <span className="text-[11px] text-[#444]">Scan with a phone, then open the link on a PC with Venuta</span>
        </div>
      )}
      <StatusStrip v={v} />
      <You v={v} act={act} env={env} />
      <Asks v={v} act={act} />
      <People v={v} act={act} open={env.start?.open} />
      <Output v={v} act={act} />
      <ActivityLine v={v} act={act} undo={v.host} />
      {m.settings && <Settings v={v} act={act} />}
      <Footer v={v} act={act} ask={env.start?.ask} />
    </div>
  )
}

function StatusStrip({ v }: { v: TogetherViewV2 }) {
  const lines = statusLines(v)
  if (!lines.length) return null
  return (
    <div className="mb-2 grid gap-1" role="status">
      {lines.map((s) => (
        <p key={s.text} className={`flex items-center gap-2 text-[12px] ${s.kind === 'phase' ? 'text-ink/60' : 'text-ink/75'}`}>
          <span
            className={`keep-round h-1.5 w-1.5 shrink-0 rounded-full ${s.kind === 'press' ? 'animate-pulse bg-accent' : s.kind === 'away' ? 'animate-pulse bg-ink/40' : 'bg-ink/30'}`}
            aria-hidden
          />
          {s.text}
        </p>
      ))}
    </div>
  )
}

/** Your own place: the remote view, or how you listen and your copy. */
function You({ v, act, env }: P) {
  const y = v.you
  if (!y) return null
  if (y.mode === 'one_speaker') return <Remote v={v} act={act} env={env} />
  const switches = modeSwitches(v)
  const version = versionNote(v)
  if (!switches.length && !version) return null
  return (
    <div className="mb-1 grid gap-1.5">
      {version && (
        <Note tone="quiet">
          <span className="min-w-0 flex-1">{version}</span>
          <button onClick={act.takeHostCopy} disabled={!may(v).modes} className="btn btn-sm disabled:opacity-40">
            Use it
          </button>
        </Note>
      )}
      {switches.length > 0 && (
        <div className="flex flex-wrap items-center gap-1.5 text-[12.5px]">
          <span className="text-ink/55">How you listen</span>
          <span className="rounded-full bg-ink/[0.07] px-1.5 text-[10.5px] leading-[16px] text-ink/60">{modeWord(y.mode, v.room?.host_name ?? '')}</span>
          <span className="flex-1" />
          {switches.map((s) => (
            <ModeButton key={s.mode} label={s.label} why={s.why_not} pick={() => act.setMode(s.mode)} />
          ))}
        </div>
      )}
      {switches
        .filter((s) => s.why_not)
        .map((s) => (
          <p key={s.mode} className="text-[11.5px] text-ink/50">
            {s.why_not}
          </p>
        ))}
    </div>
  )
}

const ModeButton = ({ label, why, pick }: { label: string; why: string | null; pick: () => void }) => (
  <button onClick={pick} disabled={!!why} title={why ?? undefined} className="btn btn-sm disabled:opacity-40">
    {label}
  </button>
)

/** One speaker: the room's song, where it is, its words, and the room's buttons (J7). */
function Remote({ v, act, env }: P) {
  const [, tick] = useState(0)
  const p = progress(v, env.since())
  const playing = !!p?.playing
  useEffect(() => {
    if (!playing) return
    const t = window.setInterval(() => tick((n) => n + 1), 250)
    return () => window.clearInterval(t)
  }, [playing])
  const m = may(v)
  const switches = modeSwitches(v)
  const lyric = env.lyrics && p ? lineAt(env.lyrics, p.ms) : -1
  const now = lyric >= 0 ? env.lyrics!.lines[lyric][1] : null
  const after = env.lyrics && lyric + 1 < env.lyrics.lines.length ? env.lyrics.lines[lyric + 1][1] : null
  const greyed = !!v.room?.host_away
  const song = p?.entry.song
  return (
    <div className="mb-2 rounded-xl bg-ink/[0.05] p-2.5">
      {song ? (
        <>
          <div className="flex items-center gap-2.5">
            <div className="grid h-12 w-12 shrink-0 place-items-center overflow-hidden rounded-lg bg-ink/[0.08] text-ink/30">
              <RemoteArt url={song.artwork_url ?? undefined} seed={song.album || song.title} size={48} imgClassName="h-full w-full object-cover" />
            </div>
            <div className="min-w-0 flex-1">
              <div className="truncate font-medium">{song.title}</div>
              <div className="truncate text-[12px] text-ink/55">{song.artists.join(', ')}</div>
            </div>
          </div>
          <div className="mt-2">
            <div className="h-1 overflow-hidden rounded-full bg-ink/10" aria-hidden>
              <div className="h-full rounded-full bg-accent" style={{ width: p && p.length_ms ? `${(p.ms / p.length_ms) * 100}%` : '0%' }} />
            </div>
            <div className="mt-1 flex justify-between text-[11px] tabular-nums text-ink/45">
              <span>{songLength(p?.ms ?? 0)}</span>
              <span>{songLength(p?.length_ms ?? 0)}</span>
            </div>
          </div>
          {(now || after) && (
            <div className="mt-1 text-[12.5px] leading-snug">
              {now && <p className="text-ink/85">{now}</p>}
              {after && <p className="text-ink/40">{after}</p>}
            </div>
          )}
        </>
      ) : (
        <p className="text-[12.5px] text-ink/60">Nothing is playing on the speaker.</p>
      )}
      <div className="mt-2 flex items-center gap-1">
        <RoundButton label="Previous" onClick={act.previous} disabled={greyed}>
          <IconPrevious width={16} height={16} />
        </RoundButton>
        <RoundButton label={playing ? 'Pause' : 'Play'} onClick={act.toggle} disabled={greyed} main>
          {playing ? <IconPause width={16} height={16} /> : <IconPlay width={16} height={16} />}
        </RoundButton>
        <RoundButton label="Next" onClick={act.next} disabled={greyed}>
          <IconNext width={16} height={16} />
        </RoundButton>
        {m.volume && v.room?.volume != null && <Volume value={v.room.volume} onChange={act.setVolume} />}
        <span className="flex-1" />
        {switches.map((s) => (
          <ModeButton key={s.mode} label={s.label} why={s.why_not} pick={() => act.setMode(s.mode)} />
        ))}
      </div>
      {!m.control && !greyed && <p className="mt-1 text-[11.5px] text-ink/45">Your presses ask whoever controls the room.</p>}
    </div>
  )
}

function RoundButton(props: { label: string; onClick: () => void; disabled?: boolean; main?: boolean; children: React.ReactNode }) {
  return (
    <button
      onClick={props.onClick}
      disabled={props.disabled}
      aria-label={props.label}
      title={props.label}
      className={`grid h-8 w-8 place-items-center rounded-full disabled:opacity-40 ${props.main ? 'accent-fill text-[var(--on-accent)]' : 'text-ink/70 hover:bg-ink/10 hover:text-ink'}`}
    >
      {props.children}
    </button>
  )
}

function Volume({ value, onChange }: { value: number; onChange: (n: number) => void }) {
  const [sliding, setSliding] = useState<number | null>(null)
  const shown = sliding ?? value
  const done = () => {
    if (sliding !== null) onChange(sliding)
    setSliding(null)
  }
  return (
    <label className="ml-1 flex items-center gap-1 text-ink/55" title={`Volume of the speaker: ${shown}%`}>
      <IconVolume width={15} height={15} />
      <input type="range" min={0} max={100} step={1} value={shown} onChange={(e) => setSliding(Number(e.target.value))} onPointerUp={done} onKeyUp={done} aria-label="The speaker's volume" className="h-1 w-20 accent-[var(--accent)]" />
    </label>
  )
}

/** Your output: its delay, its notes, and Line up by ear (4.14). */
function Output({ v, act }: { v: TogetherViewV2; act: RoomActions }) {
  const o = outputPanel(v)
  const [sliding, setSliding] = useState<number | null>(null)
  const [lining, setLining] = useState(false)
  if (!o.show) return null
  const ms = sliding ?? v.output.delay_ms
  const save = () => {
    if (sliding !== null) act.setDelay(sliding)
    setSliding(null)
  }
  return (
    <Section>
      <div className="flex items-center gap-1.5 text-[12.5px]">
        <span className="min-w-0 flex-1 truncate text-ink/70">
          {o.name}
          {o.bluetooth && <span className="ml-1.5 rounded-full bg-ink/[0.08] px-1.5 text-[10.5px] text-ink/55">Bluetooth</span>}
        </span>
        <span className="text-[11.5px] tabular-nums text-ink/50">{sliding === null ? o.delay : delayWords(ms)}</span>
      </div>
      {o.note && (
        <p className={`mt-1 text-[12px] ${o.note.warn ? 'rounded-lg bg-[color-mix(in_oklab,var(--fix)_16%,transparent)] px-2 py-1 text-ink/85' : 'text-ink/55'}`}>{o.note.text}</p>
      )}
      {o.suggest && (
        <div className="mt-1.5 flex items-center gap-2 text-[12px] text-ink/60">
          <span className="min-w-0 flex-1">{o.suggest.says}</span>
          <button onClick={() => act.setDelay(o.suggest!.ms)} className="btn btn-sm">
            Use {o.suggest.ms} ms
          </button>
        </div>
      )}
      <label className="mt-1.5 block text-[11.5px] text-ink/50">
        {lining ? 'Slide until the clicks sound evenly spaced' : 'Sounds early or late? Slide to line it up'}
        <input type="range" min={-500} max={500} step={5} value={ms} onChange={(e) => setSliding(Number(e.target.value))} onPointerUp={save} onKeyUp={save} onDoubleClick={() => act.setDelay(0)} aria-label="This output's delay" className="mt-1.5 h-1 w-full accent-[var(--accent)]" />
      </label>
      {o.lineUp && (
        <div className="mt-1 flex items-center gap-2">
          <button
            onClick={() => {
              act.lineUp(!lining)
              setLining(!lining)
            }}
            className={lining ? 'btn-primary btn-sm' : 'btn btn-sm'}
          >
            {lining ? 'Done' : 'Line up by ear'}
          </button>
          {lining && <span className="text-[11.5px] text-ink/50">{`${v.room?.host_name ?? 'The host'}'s device and this one take turns clicking`}</span>}
        </div>
      )}
    </Section>
  )
}

/** Requests to answer, and (the host's) knocks at the door. */
function Asks({ v, act }: { v: TogetherViewV2; act: RoomActions }) {
  const requests = requestsShown(v)
  const knocks = v.host ? (v.room?.waiting ?? []) : []
  const m = may(v)
  if (!requests.length && !knocks.length) return null
  return (
    <Section>
      <div className="grid gap-1.5">
        {requests.map((r) => (
          <Note key={`r${r.id}`}>
            <span className="min-w-0 flex-1 truncate">{r.words}</span>
            <button onClick={() => act.answer(r.id, true)} disabled={!m.answer} className="btn btn-sm">
              Accept
            </button>
            <button onClick={() => act.answer(r.id, false)} disabled={!m.answer} className="btn-ghost btn-sm">
              No
            </button>
          </Note>
        ))}
        {knocks.map(([id, name]) => (
          <Note key={`k${id}`}>
            <span className="min-w-0 flex-1 truncate">{name} wants to join</span>
            <button onClick={() => act.admit(id)} className="btn btn-sm">
              Let in
            </button>
            <button onClick={() => act.deny(id)} className="btn-ghost btn-sm">
              No
            </button>
          </Note>
        ))}
      </div>
    </Section>
  )
}

function People({ v, act, open }: { v: TogetherViewV2; act: RoomActions; open?: number }) {
  const list = rows(v)
  const [shown, setShown] = useState<number | null>(open ?? null)
  return (
    <Section>
      <div className="divide-y divide-ink/[0.07]">
        {list.map((r) => (
          <Person key={r.m.id} r={r} open={shown === r.m.id} toggle={() => setShown((s) => (s === r.m.id ? null : r.m.id))} act={act} />
        ))}
      </div>
      {list.length === 1 && v.host && !v.room?.waiting.length && <p className="py-1 text-[12px] text-ink/50">Waiting for friends. Copy the invite and send it to them.</p>}
    </Section>
  )
}

function lineTone(m: MemberView): string {
  const s = m.sub_state?.state
  if (m.away || s === 'stepped_out' || s === 'cannot_play') return 'text-ink/40'
  if (s === 'correcting' || s === 'realigning' || s === 'suspended') return 'text-[color-mix(in_oklab,var(--fix)_80%,var(--ink))]'
  return 'text-ink/50'
}

function Person({ r, open, toggle, act }: { r: Row; open: boolean; toggle: () => void; act: RoomActions }) {
  const [menu, setMenu] = useState(false)
  const m = r.m
  const details = [r.copy, r.numbers, r.path].filter(Boolean) as string[]
  return (
    <div className="py-1.5">
      <div className="flex items-center gap-2">
        <Initial m={m} />
        <button onClick={toggle} disabled={!details.length} aria-expanded={open} className="min-w-0 flex-1 text-left" title={details.length ? 'Show the numbers' : undefined}>
          <span className="flex items-center gap-1.5 text-[13px]">
            <span className="truncate">{m.name}</span>
            {m.host && <span className="text-[11.5px] text-ink/45">host</span>}
            {r.you && !m.host && <span className="text-[11.5px] text-ink/45">you</span>}
            <span className="shrink-0 rounded-full bg-ink/[0.07] px-1.5 text-[10.5px] leading-[16px] text-ink/55">{r.mode}</span>
          </span>
          <span className={`mt-0.5 block truncate text-[11.5px] ${lineTone(m)}`}>{r.line}</span>
        </button>
        {r.manage && (
          <>
            <select value={m.role} onChange={(e) => act.setRole(m.id, e.target.value as Role)} aria-label={`What ${m.name} can do`} className="field h-7 shrink-0 rounded-full py-0 pl-2.5 pr-2 text-[12px] leading-none">
              {ROLES.map(([role, l]) => (
                <option key={role} value={role}>
                  {l}
                </option>
              ))}
            </select>
            <button onClick={() => setMenu((x) => !x)} aria-expanded={menu} aria-label={`More for ${m.name}`} className="grid h-7 w-7 place-items-center rounded-full text-ink/50 hover:bg-ink/10 hover:text-ink">
              ···
            </button>
          </>
        )}
      </div>
      {open && details.length > 0 && (
        <div className="ml-[30px] mt-1 grid gap-0.5 text-[11.5px] text-ink/55">
          {details.map((d) => (
            <span key={d}>{d[0].toUpperCase() + d.slice(1)}</span>
          ))}
        </div>
      )}
      {r.slow && (
        <div className="ml-[30px] mt-1 flex flex-wrap items-center gap-1 text-[11.5px]">
          <span className="mr-1 text-[color-mix(in_oklab,var(--fix)_80%,var(--ink))]">{r.slow}</span>
          <button onClick={() => act.lighter(m.id)} className="btn-ghost btn-sm">
            Give a lighter copy
          </button>
          <button onClick={() => act.nextSongFor(m.id)} className="btn-ghost btn-sm">
            Start with the next song
          </button>
        </div>
      )}
      {menu && (
        <div className="ml-[30px] mt-1 flex gap-1">
          <button onClick={() => act.remove(m.id, false)} className="btn btn-sm">
            Remove
          </button>
          <button onClick={() => act.remove(m.id, true)} className="btn-ghost btn-sm">
            Remove and block
          </button>
        </div>
      )}
    </div>
  )
}

function ActivityLine({ v, act, undo }: { v: TogetherViewV2; act: RoomActions; undo: boolean }) {
  const a = v.room?.activity ?? []
  if (!a.length) return null
  const last = a.slice(-2).reverse()
  const name = (id: number) => v.room?.members.find((m) => m.id === id)?.name ?? ''
  return (
    <div className="mt-2 flex items-center gap-2 border-t border-edge pt-2 text-[11.5px] text-ink/55">
      <span className="min-w-0 flex-1 truncate">
        {last.map((x, i) => (
          <span key={i}>
            {i > 0 && ' · '}
            {x.name || name(x.who)} {x.what}
          </span>
        ))}
      </span>
      {undo && a[a.length - 1]?.undoable && (
        <button onClick={act.undo} className="shrink-0 text-accent-ink hover:underline">
          Undo
        </button>
      )}
    </div>
  )
}

function Settings({ v, act }: { v: TogetherViewV2; act: RoomActions }) {
  const room = v.room!
  const options = transitionOptions(room.transition)
  const row = 'flex items-center gap-2'
  const label = 'flex-1 text-ink/65'
  return (
    <div className="mt-2 grid gap-2 border-t border-edge pt-2.5 text-[12.5px]">
      <div className={row}>
        <span className={label}>New people can</span>
        <select value={newRoleValue(room)} onChange={(e) => act.setNewRole(e.target.value as Role | 'auto')} aria-label="What new people can do" className="field h-7 shrink-0 rounded-full py-0 pl-2.5 pr-2 text-[12px] leading-none">
          <option value="auto">Automatic</option>
          {ROLES.map(([r, l]) => (
            <option key={r} value={r}>
              {l}
            </option>
          ))}
        </select>
      </div>
      <div className={row}>
        <span className={label}>Between songs</span>
        <select
          value={transitionKey(room.transition)}
          onChange={(e) => {
            const o = options.find((x) => x.key === e.target.value)
            if (o) act.setTransition(o.t)
          }}
          aria-label="Between songs"
          className="field h-7 shrink-0 rounded-full py-0 pl-2.5 pr-2 text-[12px] leading-none"
        >
          {options.map((o) => (
            <option key={o.key} value={o.key}>
              {o.label}
            </option>
          ))}
        </select>
      </div>
      <div className={row}>
        <span className={label}>Let guests change the volume</span>
        <Switch on={room.guests_volume} onChange={act.setGuestsVolume} label="Let guests change the volume" />
      </div>
      <div className={row}>
        <span className={label}>Let people with the link in without asking</span>
        <Switch on={v.let_in} onChange={act.setLetIn} label="Let people in without asking" />
      </div>
      <div className={row}>
        <span className={label}>Visible on this network</span>
        <Switch on={v.visible} onChange={act.setVisible} label="Visible on this network" />
      </div>
    </div>
  )
}

function StepButton({ v, act }: { v: TogetherViewV2; act: RoomActions }) {
  const step = stepButton(v)
  if (!step) return null
  return (
    <button onClick={() => act.stepOut(!step.out)} className={step.out ? 'btn-primary btn-sm' : 'btn-ghost btn-sm'} title={step.out ? 'Back to where the room is' : 'Pause just for you; the room goes on'}>
      {step.label}
    </button>
  )
}

function Footer({ v, act, ask: first }: { v: TogetherViewV2; act: RoomActions; ask?: 'end' | 'hand' }) {
  const [ask, setAsk] = useState<'none' | 'end' | 'hand'>(first ?? 'none')
  if (!v.host)
    return (
      <div className="mt-2.5 flex items-center gap-1.5">
        <StepButton v={v} act={act} />
        <span className="flex-1" />
        <button onClick={act.leave} className="btn btn-sm">
          Leave
        </button>
      </div>
    )
  if (ask === 'end')
    return (
      <div className="mt-2.5 grid gap-2 rounded-xl bg-ink/[0.05] p-2.5">
        <p className="text-[12.5px]">{END_ASK}</p>
        <div className="flex justify-end gap-1.5">
          <button onClick={() => setAsk('none')} className="btn-ghost btn-sm">
            Keep the room
          </button>
          <button onClick={act.end} className="btn-danger btn-sm">
            End for everyone
          </button>
        </div>
      </div>
    )
  if (ask === 'hand') {
    const choices = handOverChoices(v)
    return (
      <div className="mt-2.5 grid gap-1.5 rounded-xl bg-ink/[0.05] p-2.5">
        <p className="text-[12.5px] font-medium">Hand the room to</p>
        {choices.length ? (
          choices.map((c) => (
            <button key={c.id} onClick={() => act.handOver(c.id)} className="flex items-center gap-2 rounded-lg px-1.5 py-1 text-left hover:bg-ink/[0.06]">
              <Initial m={c} />
              <span className="min-w-0 flex-1">
                <span className="block">{c.name}</span>
                {c.warn && <span className="block text-[11.5px] text-ink/50">{c.warn}</span>}
              </span>
            </button>
          ))
        ) : (
          <p className="text-[12px] text-ink/55">Nobody here can take the room over yet</p>
        )}
        <button onClick={() => setAsk('none')} className="btn-ghost btn-sm justify-self-end">
          Cancel
        </button>
      </div>
    )
  }
  return (
    <div className="mt-2.5 flex items-center gap-1.5">
      <StepButton v={v} act={act} />
      <span className="flex-1" />
      <button onClick={() => setAsk('hand')} className="btn-ghost btn-sm">
        Hand over...
      </button>
      <button onClick={() => setAsk('end')} className="btn btn-sm">
        End for everyone
      </button>
    </div>
  )
}

