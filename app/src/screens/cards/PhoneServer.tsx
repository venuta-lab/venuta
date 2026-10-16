import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useState } from 'react'
import {
  inApp,
  phoneServerAdd,
  phoneServerRemove,
  phoneServerStart,
  phoneServerStatus,
  phoneServerStop,
  type Phone,
  type PhoneServerStatus,
} from '../../lib/engine'
import { notify } from '../../lib/notify'
import { Switch } from '../../components/ui'

/** Devices › Phones over Wi-Fi: phones on the same Wi-Fi play the library, and keep albums and
 *  playlists for offline, in a Subsonic app. */
export function PhoneServer({ root }: { root: string }) {
  const [s, setS] = useState<PhoneServerStatus | null>(null)
  const [name, setName] = useState('')
  const [busy, setBusy] = useState(false)
  // The phone whose sign-in is showing: closed until asked for (a shared screen shows no
  // password), and open for a phone just added.
  const [open, setOpen] = useState<string | null>(null)

  useEffect(() => {
    if (inApp()) void phoneServerStatus().then(setS)
  }, [])

  const run = async (f: () => Promise<PhoneServerStatus>) => {
    setBusy(true)
    try {
      const next = await f()
      setS(next)
      return next
    } catch (e) {
      notify(e instanceof Error ? e.message : String(e))
      return null
    } finally {
      setBusy(false)
    }
  }

  if (!inApp() || !s) return null
  const url = s.address ? `http://${s.address}:${s.port}` : null

  return (
    <section className="mt-12" aria-labelledby="wifi-heading">
      <div className="flex items-start gap-4">
        <div className="min-w-0 flex-1">
          <h2 id="wifi-heading" className="text-[17px] font-semibold tracking-[-0.01em]">
            Phones over Wi-Fi
          </h2>
          <p className="mt-1 max-w-[66ch] text-[12.5px] text-ink/55">
            Your phone plays your library at home, and keeps albums and playlists to play anywhere, in a free music app. Only devices on your own Wi-Fi can reach it.
          </p>
        </div>
        <label className="mt-0.5 flex shrink-0 items-center gap-3 text-[13px] text-ink/65">
          {s.on ? 'On' : 'Off'}
          <Switch
            on={s.on}
            label="Share with phones on this Wi-Fi"
            disabled={busy}
            onChange={(on) => void run(() => (on ? phoneServerStart(root) : phoneServerStop()))}
          />
        </label>
      </div>

      <AnimatePresence initial={false}>
        {s.on && (
          <motion.div initial={{ height: 0, opacity: 0 }} animate={{ height: 'auto', opacity: 1 }} exit={{ height: 0, opacity: 0 }} className="overflow-hidden">
            <div className="glass mt-4 rounded-[22px] p-5">
              {!url && (
                <p className="mb-4 rounded-xl px-3 py-2.5 text-[13px]" style={{ background: 'color-mix(in oklab, var(--fix) 16%, transparent)' }}>
                  This PC isn’t on a network right now. Connect it to your Wi-Fi, and the address shows here.
                </p>
              )}

              {s.phones.length > 0 && (
                <ul className="grid gap-2">
                  {s.phones.map((p) => (
                    <PhoneRow
                      key={p.user}
                      phone={p}
                      url={url}
                      open={open === p.user}
                      onToggle={() => setOpen(open === p.user ? null : p.user)}
                      onRemove={() => void run(() => phoneServerRemove(p.user)).then(() => notify(`${p.name} can’t reach your library any more.`))}
                    />
                  ))}
                </ul>
              )}

              <form
                className={`flex flex-wrap items-center gap-2 ${s.phones.length ? 'mt-4' : ''}`}
                onSubmit={(e) => {
                  e.preventDefault()
                  if (!name.trim()) return
                  void run(() => phoneServerAdd(name)).then((next) => {
                    if (!next) return
                    setName('')
                    // The newest phone is last: its sign-in opens, ready to type into the app.
                    setOpen(next.phones.at(-1)?.user ?? null)
                  })
                }}
              >
                <label htmlFor="phone-name" className="w-full text-[13.5px] font-medium">
                  {s.phones.length ? 'Add another phone' : 'Add your phone'}
                </label>
                <input
                  id="phone-name"
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder="A name for it, like My phone"
                  className="min-w-[200px] flex-1 rounded-full bg-ink/[0.07] px-4 py-2 text-[13.5px] outline-none shadow-[inset_0_0_0_1px_var(--edge)] focus:ring-1 focus:ring-[color-mix(in_oklab,var(--accent)_60%,transparent)]"
                />
                <button type="submit" disabled={busy || !name.trim()} className={`btn btn-sm ${s.phones.length ? '' : 'accent-fill'}`}>
                  Add phone
                </button>
              </form>

              <p className="mt-4 text-[12px] leading-relaxed text-ink/50">
                Can’t connect? The phone must be on the same Wi-Fi as this PC, and Windows must allow Venuta on private networks (it asks the first time).
              </p>
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </section>
  )
}

function PhoneRow(props: { phone: Phone; url: string | null; open: boolean; onToggle: () => void; onRemove: () => void }) {
  const { phone } = props
  return (
    <li className="rounded-2xl bg-ink/[0.04]">
      <div className="flex items-center gap-3 px-4 py-2.5">
        <span className="min-w-0 flex-1 truncate text-[14px] font-medium">{phone.name}</span>
        <button onClick={props.onToggle} aria-expanded={props.open} className="btn-ghost btn-sm">
          {props.open ? 'Hide sign-in' : 'Sign-in details'}
        </button>
        <button onClick={props.onRemove} className="btn-ghost btn-sm" title="This phone can’t reach your library any more">
          Remove
        </button>
      </div>
      <AnimatePresence initial={false}>
        {props.open && (
          <motion.div initial={{ height: 0, opacity: 0 }} animate={{ height: 'auto', opacity: 1 }} exit={{ height: 0, opacity: 0 }} className="overflow-hidden">
            <div className="px-4 pb-4">
              <dl className="grid gap-2 sm:grid-cols-3">
                <Field label="Server" value={props.url ?? 'Not on a network'} copy={!!props.url} />
                <Field label="Username" value={phone.user} copy />
                <Field label="Password" value={phone.password} copy />
              </dl>
              <ol className="mt-4 grid list-decimal gap-1.5 pl-5 text-[12.5px] leading-relaxed text-ink/70 marker:text-ink/40">
                <li>
                  On the phone, install a music app: <b className="font-medium text-ink/85">Tempus</b> on Android or <b className="font-medium text-ink/85">Amperfy</b> on iPhone (both free).{' '}
                  <b className="font-medium text-ink/85">Symfonium</b> (Android, paid) is the most complete.
                </li>
                <li>In the app, add a Subsonic server and type the three above.</li>
                <li>Play. To listen away from home, download albums or playlists in the app.</li>
              </ol>
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </li>
  )
}

function Field(props: { label: string; value: string; copy: boolean }) {
  return (
    <div className="min-w-0 rounded-xl bg-ink/[0.05] px-3.5 py-2.5">
      <dt className="text-[11.5px] text-ink/50">{props.label}</dt>
      <dd className="mt-0.5 flex items-center gap-2">
        <span className="min-w-0 flex-1 select-text truncate text-[15.5px] font-semibold tracking-[0.01em]" title={props.value}>
          {props.value}
        </span>
        {props.copy && (
          <button
            onClick={() => void navigator.clipboard.writeText(props.value).then(() => notify(`${props.label} copied.`))}
            className="btn-ghost btn-sm shrink-0"
            aria-label={`Copy the ${props.label.toLowerCase()}`}
          >
            Copy
          </button>
        )}
      </dd>
    </div>
  )
}
