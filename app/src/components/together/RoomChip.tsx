import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useRef, useState } from 'react'
import { libraryHome } from '../../lib/engine'
import { useEscape } from '../../lib/layers'
import { notify } from '../../lib/notify'
import {
  admit,
  can,
  deny,
  dropInvite,
  joinRoom,
  leaveRoom,
  meIn,
  memberName,
  pendingInvite,
  readInvite,
  removeMember,
  setLetIn,
  setMyDelay,
  setMyName,
  setNewRole,
  setRole,
  setSameRoom,
  startRoom,
  stepOut,
  TOGETHER,
  undoRoom,
  useTogether,
  type Hearing,
  type Member,
  type Role,
  type TogetherView,
} from '../../lib/together'
import { Switch } from '../ui'

/*
  Listening together lives here, in the title bar, on every screen: one chip and the panel it
  opens. Out of a room: start one, or join with an invite. In a room: who's here and how each of
  them hears the song, who did what, and (for the host) who may do what.
*/

const ROLES: [Role, string][] = [
  ['control', 'Control'],
  ['add', 'Add songs'],
  ['listen', 'Just listen'],
]

const INITIAL_COLORS = ['#CECBF6', '#9FE1CB', '#F5C4B3', '#F4C0D1', '#B5D4F4', '#C0DD97', '#FAC775', '#D3D1C7']
const INK = ['#3C3489', '#085041', '#712B13', '#72243E', '#0C447C', '#27500A', '#633806', '#444441']

function Initial({ m, size = 22 }: { m: { id: number; name: string }; size?: number }) {
  const i = m.id % INITIAL_COLORS.length
  return (
    <span
      className="grid shrink-0 place-items-center rounded-full font-semibold"
      style={{ width: size, height: size, fontSize: size * 0.48, background: INITIAL_COLORS[i], color: INK[i] }}
      aria-hidden
    >
      {(m.name.trim()[0] ?? '?').toUpperCase()}
    </span>
  )
}

/** How someone hears the song now, in a word or two. */
function HearingLabel({ how, host }: { how: Hearing; host: string }) {
  const [text, tone] = (
    {
      opening: ['GETTING READY', 'bg-ink/[0.07] text-ink/50'],
      local: ['LOCAL', 'bg-ink/[0.07] text-ink/60'],
      online: ['ONLINE', 'bg-accent/15 text-accent-ink'],
      from_host: [`FROM ${host.toUpperCase()}`, 'bg-ink/[0.1] text-ink/70'],
      cannot: ["CAN'T PLAY THIS", 'bg-ink/[0.05] text-ink/40'],
      stepped_out: ['STEPPED OUT', 'bg-ink/[0.05] text-ink/40'],
    } as Record<Hearing, [string, string]>
  )[how]
  return <span className={`shrink-0 truncate rounded-full px-1.5 text-[10px] font-semibold leading-[16px] tracking-[0.04em] ${tone}`}>{text}</span>
}

function syncWords(m: Member): string {
  if (m.away) return 'reconnecting…'
  if (m.hearing === 'cannot' || m.hearing === 'stepped_out' || m.hearing === 'opening') return ''
  if (m.error_ms === null) return m.host ? 'in charge' : ''
  const e = Math.abs(m.error_ms)
  return e <= 60 ? `in sync ±${e} ms` : 'catching up…'
}

/** The title bar's chip and its panel. */
export function RoomChip() {
  const v = useTogether()
  const [shut, setOpen] = useState(false)
  // An invite link Venuta was opened with opens the panel, ready to join.
  const offered = pendingInvite()
  const open = shut || offered !== null
  const box = useRef<HTMLDivElement>(null)
  const closeIt = () => {
    setOpen(false)
    dropInvite()
  }
  useEscape(open, closeIt)
  useEffect(() => {
    if (!open) return
    const close = (e: PointerEvent) => {
      if (box.current?.contains(e.target as Node)) return
      setOpen(false)
      dropInvite()
    }
    window.addEventListener('pointerdown', close)
    return () => window.removeEventListener('pointerdown', close)
  }, [open])
  // Someone knocking opens the host's panel (so the request is seen at once).
  const waiting = v.host ? (v.room?.waiting.length ?? 0) : 0
  const lastWaiting = useRef(0)
  useEffect(() => {
    if (waiting > lastWaiting.current) setOpen(true)
    lastWaiting.current = waiting
  }, [waiting])

  const inRoom = v.state === 'hosting' || v.state === 'in' || v.state === 'reconnecting' || v.state === 'waiting'
  const people = v.room?.members ?? []
  const off = people.reduce((n, m) => n + (m.error_ms !== null && Math.abs(m.error_ms) > 60 ? 1 : 0), 0)
  const ring = v.state === 'reconnecting' ? 'shadow-[0_0_0_1.5px_rgb(140_140_140/0.7)]' : off ? 'shadow-[0_0_0_1.5px_rgb(220_160_40/0.8)]' : 'shadow-[0_0_0_1.5px_rgb(60_180_120/0.75)]'

  return (
    <div ref={box} className="relative mr-1 flex items-center">
      {inRoom ? (
        <button
          onClick={() => setOpen((o) => !o)}
          aria-expanded={open}
          aria-label={`${TOGETHER}: ${people.length} ${people.length === 1 ? 'person' : 'people'}`}
          title={TOGETHER}
          className={`flex items-center gap-1 rounded-full bg-ink/[0.06] py-[3px] pl-1 pr-2.5 text-[12px] text-ink/80 hover:bg-ink/[0.1] ${ring}`}
        >
          <span className="flex">
            {people.slice(0, 3).map((m, i) => (
              <span key={m.id} style={{ marginLeft: i ? -7 : 0 }} className="rounded-full ring-2 ring-[var(--app-bg,transparent)]">
                <Initial m={m} size={20} />
              </span>
            ))}
          </span>
          {people.length > 3 && <span>+{people.length - 3}</span>}
          {waiting > 0 && <span className="ml-0.5 h-1.5 w-1.5 rounded-full bg-[#e0a030]" aria-label={`${waiting} waiting`} />}
          {v.state === 'waiting' && <span className="ml-1">Waiting…</span>}
        </button>
      ) : (
        <button
          onClick={() => (open ? closeIt() : setOpen(true))}
          aria-expanded={open}
          aria-label={TOGETHER}
          title={TOGETHER}
          className="grid h-8 w-8 place-items-center rounded-full text-ink/60 transition-colors hover:bg-ink/10 hover:text-ink"
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
            <circle cx="9" cy="8" r="3.2" />
            <path d="M3.5 19c.6-3.2 2.8-5 5.5-5s4.9 1.8 5.5 5" />
            <circle cx="16.5" cy="9" r="2.6" />
            <path d="M15.5 14.2c2.4-.3 4.4 1.2 5 4.3" />
          </svg>
        </button>
      )}
      <AnimatePresence>
        {open && (
          <motion.div
            initial={{ opacity: 0, y: -6 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -6 }}
            transition={{ duration: 0.16 }}
            className="popover absolute right-0 top-10 z-50 w-[380px] rounded-2xl p-3.5"
            role="dialog"
            aria-label={TOGETHER}
          >
            {v.host && v.room ? <HostPanel v={v} /> : inRoom || v.state === 'joining' ? <GuestPanel v={v} /> : <StartPanel v={v} />}
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  )
}

/** Out of a room: start one, or join with an invite. */
function StartPanel({ v }: { v: TogetherView }) {
  const [code, setCode] = useState(() => pendingInvite() ?? '')
  const [read, setRead] = useState<{ code: string; host: string; open: boolean } | null>(null)
  const [busy, setBusy] = useState(false)
  // Your edit while typing; the saved name otherwise.
  const [draft, setDraft] = useState<string | null>(null)
  const name = draft ?? v.name
  // What the pasted invite says (only for the invite in the box now).
  const host = code.trim() && read?.code === code ? read : null
  useEffect(() => {
    if (!code.trim()) return
    let live = true
    readInvite(code)
      .then((h) => live && setRead({ code, ...h }))
      .catch(() => live && setRead(null))
    return () => {
      live = false
    }
  }, [code])
  const root = async () => (await libraryHome()).library ?? ''
  const start = async () => {
    setBusy(true)
    await startRoom(await root())
    setBusy(false)
  }
  const join = async () => {
    setBusy(true)
    await joinRoom(await root(), code)
    setBusy(false)
  }
  return (
    <div className="grid gap-3.5 text-[13px]">
      <div>
        <div className="text-[14px] font-semibold">{TOGETHER}</div>
        <p className="mt-0.5 text-[12.5px] leading-relaxed text-ink/55">
          Friends hear what you play at the same moment, each from their best copy: their own file, the online song, or yours streamed to them.
        </p>
      </div>
      {v.state === 'starting' ? (
        <p className="text-ink/60">Opening your room…</p>
      ) : (
        <button onClick={() => void start()} disabled={busy} className="btn-primary btn-sm justify-self-start">
          Start a room
        </button>
      )}
      <div className="border-t border-edge pt-3">
        <div className="mb-1.5 text-[12.5px] font-medium">Join a room</div>
        <div className="flex gap-2">
          <input value={code} onChange={(e) => setCode(e.target.value)} placeholder="Paste an invite" aria-label="Invite" spellCheck={false} className="field h-8 min-w-0 flex-1 rounded-full px-3 text-[12.5px]" />
          <button onClick={() => void join()} disabled={!host?.open || busy} className="btn btn-sm">
            {v.state === 'joining' ? 'Joining…' : 'Join'}
          </button>
        </div>
        {code.trim() && (
          <p className="mt-1.5 text-[12px] text-ink/55">{host ? (host.open ? `${host.host}'s room` : `${host.host}'s invite has ended. Ask for a new one.`) : "That isn't a Venuta invite."}</p>
        )}
      </div>
      <label className="flex items-center gap-2 border-t border-edge pt-3 text-[12.5px] text-ink/60">
        Your name
        <input
          value={name}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={() => {
            if (name.trim() && name !== v.name) void setMyName(name)
            setDraft(null)
          }}
          className="field h-7 min-w-0 flex-1 rounded-full px-3 text-[12.5px] text-ink"
          aria-label="Your name"
        />
      </label>
      {v.message && <p className="text-[12px] text-ink/55">{v.message}</p>}
    </div>
  )
}

function RoleMenu({ value, onChange, label }: { value: Role; onChange: (r: Role) => void; label: string }) {
  return (
    <select value={value} onChange={(e) => onChange(e.target.value as Role)} aria-label={label} className="field h-7 shrink-0 rounded-full py-0 pl-2.5 pr-2 text-[12px] leading-none">
      {ROLES.map(([r, l]) => (
        <option key={r} value={r}>
          {l}
        </option>
      ))}
    </select>
  )
}

function People({ v, host }: { v: TogetherView; host: boolean }) {
  const room = v.room
  if (!room) return null
  const hostName = room.host_name
  return (
    <div className="divide-y divide-ink/[0.07]">
      {room.members.map((m) => (
        <div key={m.id} className="group flex items-center gap-2 py-1.5">
          <Initial m={m} />
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-1.5 text-[13px]">
              <span className="truncate">{m.name}</span>
              {m.host && <span className="text-[11.5px] text-ink/45">host</span>}
              {m.id === v.me && !m.host && <span className="text-[11.5px] text-ink/45">you</span>}
            </div>
            <div className="mt-0.5 flex items-center gap-1.5">
              <HearingLabel how={m.hearing} host={hostName} />
              <span className="truncate text-[11.5px] text-ink/45">{syncWords(m)}</span>
            </div>
          </div>
          {host && !m.host && (
            <>
              <RoleMenu value={m.role} onChange={(r) => void setRole(m.id, r)} label={`What ${m.name} can do`} />
              <button onClick={() => void removeMember(m.id, false)} title={`Take ${m.name} out of the room`} aria-label={`Remove ${m.name}`} className="grid h-6 w-6 place-items-center rounded-full text-ink/40 opacity-0 hover:bg-ink/10 hover:text-ink group-hover:opacity-100">
                ✕
              </button>
            </>
          )}
        </div>
      ))}
    </div>
  )
}

/** A song on its way: said, with who it waits for, so a press is seen to have worked. */
function Starting({ v }: { v: TogetherView }) {
  const who = v.room?.starting
  if (who == null) return null
  const names = who.length > 2 ? `${who.slice(0, 2).join(', ')} and ${who.length - 2} more` : who.join(' and ')
  return (
    <p className="flex items-center gap-2 py-1 text-[12px] text-ink/65" role="status">
      <span className="keep-round h-1.5 w-1.5 animate-pulse rounded-full bg-accent" aria-hidden />
      {names ? `Starting, waiting for ${names}…` : 'Starting…'}
    </p>
  )
}

function ActivityLine({ v, host }: { v: TogetherView; host: boolean }) {
  const a = v.room?.activity ?? []
  if (!a.length) return null
  const last = a.slice(-2).reverse()
  const undoable = host && a[a.length - 1]?.undoable
  return (
    <div className="mt-2 flex items-center gap-2 border-t border-edge pt-2 text-[11.5px] text-ink/55">
      <span className="min-w-0 flex-1 truncate">
        {last.map((x, i) => (
          <span key={i}>
            {i > 0 && ' · '}
            {x.name || memberName(v, x.who)} {x.what}
          </span>
        ))}
      </span>
      {undoable && (
        <button onClick={() => void undoRoom()} className="shrink-0 text-accent-ink hover:underline">
          Undo
        </button>
      )}
    </div>
  )
}

/** Your output's delay (Bluetooth headphones hear late): slide until it lines up. */
function Delay({ v }: { v: TogetherView }) {
  // While sliding, your value; the saved one otherwise.
  const [sliding, setSliding] = useState<number | null>(null)
  const ms = sliding ?? v.delay_ms
  const save = () => {
    if (sliding !== null) void setMyDelay(sliding).then(() => setSliding(null))
  }
  return (
    <div className="mt-2 border-t border-edge pt-2">
      <div className="flex items-center justify-between text-[11.5px] text-ink/55">
        <span>Sounds early or late? (Bluetooth)</span>
        <span>{ms === 0 ? 'none' : `${ms > 0 ? '+' : ''}${ms} ms`}</span>
      </div>
      <input type="range" min={-200} max={400} step={10} value={ms} onChange={(e) => setSliding(Number(e.target.value))} onPointerUp={save} onKeyUp={save} aria-label="Your delay" className="mt-1 w-full" />
    </div>
  )
}

function HostPanel({ v }: { v: TogetherView }) {
  const room = v.room!
  const [qr, setQr] = useState(false)
  const copy = async () => {
    if (!v.invite) return
    await navigator.clipboard?.writeText(v.invite.link).catch(() => {})
    notify('Invite link copied. Send it to friends; you let each of them in.')
  }
  const end = async () => {
    await leaveRoom()
    notify('Your room has ended.')
  }
  return (
    <div className="text-[13px]">
      <div className="mb-2 flex items-center gap-2">
        <span className="flex-1 font-semibold">
          {TOGETHER} · {room.members.length}
        </span>
        <button onClick={() => setQr((q) => !q)} aria-expanded={qr} className="btn-ghost btn-sm">
          QR
        </button>
        <button onClick={() => void copy()} className="btn btn-sm">
          Copy invite
        </button>
      </div>
      {qr && v.invite && (
        <div className="mb-2 grid justify-items-center gap-1 rounded-xl bg-white p-2">
          <div className="h-[180px] w-[180px] [&>svg]:h-full [&>svg]:w-full" dangerouslySetInnerHTML={{ __html: v.invite.qr_svg }} />
          <span className="text-[11px] text-[#444]">Scan with a phone, then open the link on a PC with Venuta</span>
        </div>
      )}
      {room.waiting.map(([id, name]) => (
        <div key={id} className="mb-1.5 flex items-center gap-2 rounded-xl bg-[color-mix(in_oklab,#e0a030_18%,transparent)] px-2.5 py-1.5">
          <span className="min-w-0 flex-1 truncate">{name} wants to join</span>
          <button onClick={() => void admit(id)} className="btn btn-sm">
            Let in
          </button>
          <button onClick={() => void deny(id)} className="btn-ghost btn-sm">
            No
          </button>
        </div>
      ))}
      <People v={v} host />
      {room.members.length === 1 && !room.waiting.length && <p className="py-1 text-[12px] text-ink/50">Waiting for friends. Copy the invite and send it to them.</p>}
      <Starting v={v} />
      <ActivityLine v={v} host />
      <div className="mt-2 grid gap-2 border-t border-edge pt-2.5 text-[12.5px]">
        <div className="flex items-center gap-2">
          <span className="flex-1 text-ink/65">New people can</span>
          <RoleMenu value={room.new_role} onChange={(r) => void setNewRole(r)} label="What new people can do" />
        </div>
        <div className="flex items-center gap-2">
          <span className="flex-1 text-ink/65" title="For speakers that hear each other, with everyone on this Wi-Fi: within a few milliseconds (not bit-perfect while it corrects). Over the internet each of you hears it smoothly on your own.">
            Same room (tight sync)
          </span>
          <Switch on={room.same_room} onChange={(on) => void setSameRoom(on)} label="Same room" />
        </div>
        <div className="flex items-center gap-2">
          <span className="flex-1 text-ink/65">Let people with the link in without asking</span>
          <Switch on={v.let_in} onChange={(on) => void setLetIn(on)} label="Let people in without asking" />
        </div>
      </div>
      <Delay v={v} />
      <div className="mt-2.5 flex justify-end">
        <button onClick={() => void end()} className="btn btn-sm">
          End the room
        </button>
      </div>
    </div>
  )
}

function GuestPanel({ v }: { v: TogetherView }) {
  const me = meIn(v)
  const host = v.room?.host_name ?? v.invite?.host ?? 'the host'
  const out = me?.hearing === 'stepped_out'
  if (v.state === 'joining' || v.state === 'waiting')
    return (
      <div className="grid gap-2 text-[13px]">
        <div className="font-semibold">{TOGETHER}</div>
        <p className="text-ink/60">{v.state === 'joining' ? 'Connecting to the room…' : `Waiting for ${host} to let you in…`}</p>
        <button onClick={() => void leaveRoom()} className="btn-ghost btn-sm justify-self-end">
          Cancel
        </button>
      </div>
    )
  const roleWords = can(v, 'control') ? 'You can control' : can(v, 'add') ? 'You can add songs' : 'Listening'
  return (
    <div className="text-[13px]">
      <div className="mb-2 flex items-center gap-2">
        <span className="flex-1 font-semibold">
          With {host} · {v.room?.members.length ?? 0}
        </span>
        <span className="text-[11.5px] text-ink/50">{roleWords}</span>
      </div>
      {v.state === 'reconnecting' && <p className="mb-1.5 text-[12px] text-ink/60">Reconnecting… the music goes on meanwhile.</p>}
      <People v={v} host={false} />
      <Starting v={v} />
      <ActivityLine v={v} host={false} />
      <Delay v={v} />
      <div className="mt-2.5 flex items-center justify-between">
        <button onClick={() => void stepOut(!out)} className="btn-ghost btn-sm" title={out ? 'Back to where the room is' : 'Pause just for you; the room goes on'}>
          {out ? 'Catch up' : 'Step out'}
        </button>
        <span className="text-[11px] text-ink/45">{v.path ? (v.path.direct ? 'Direct connection' : v.path.relay ? 'Through a relay' : '') + (v.path.rtt_ms != null ? ` · ${v.path.rtt_ms} ms` : '') : ''}</span>
        <button onClick={() => void leaveRoom()} className="btn btn-sm">
          Leave the room
        </button>
      </div>
    </div>
  )
}
