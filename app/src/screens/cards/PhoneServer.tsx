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

/** Devices › Over Wi-Fi: phones on the same Wi-Fi play the library, and save it for offline,
 *  in a Subsonic app. */
export function PhoneServer({ root }: { root: string }) {
  const [s, setS] = useState<PhoneServerStatus | null>(null)
  const [name, setName] = useState('')
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    if (inApp()) void phoneServerStatus().then(setS)
  }, [])

  const run = async (f: () => Promise<PhoneServerStatus>) => {
    setBusy(true)
    try {
      setS(await f())
    } catch (e) {
      notify(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  if (!inApp() || !s) return null
  const url = s.address ? `http://${s.address}:${s.port}` : null

  return (
    <section className="mt-12" aria-labelledby="wifi-heading">
      <div className="flex items-center gap-4">
        <div className="min-w-0 flex-1">
          <h2 id="wifi-heading" className="text-[15px] font-medium text-ink/80">
            Over Wi-Fi
          </h2>
          <p className="mt-1 max-w-[62ch] text-[12.5px] text-ink/55">
            Phones on your Wi-Fi play your library, and save albums and playlists to listen anywhere, in a free Subsonic app. Only devices on your own network can reach it.
          </p>
        </div>
        <Switch
          on={s.on}
          label="Share with phones on this Wi-Fi"
          disabled={busy}
          onChange={(on) => void run(() => (on ? phoneServerStart(root) : phoneServerStop()))}
        />
      </div>

      {s.on && (
        <div className="glass mt-4 rounded-3xl p-5">
          <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-6 gap-y-2 text-[13.5px]">
            <dt className="text-ink/55">Server address</dt>
            <dd className="min-w-0 select-text font-medium">
              {url ?? 'This PC isn’t on a network right now.'}
              {url && s.host && <span className="ml-2 text-ink/50">(or http://{s.host}:{s.port})</span>}
            </dd>
          </dl>

          <h3 className="mt-6 text-[13.5px] font-medium">Phones</h3>
          {s.phones.length === 0 && <p className="mt-1 text-[12.5px] text-ink/55">Add a phone to get the name and password it signs in with.</p>}
          <ul className="mt-2 grid gap-2">
            {s.phones.map((p) => (
              <PhoneRow key={p.user} phone={p} onRemove={() => void run(() => phoneServerRemove(p.user))} />
            ))}
          </ul>
          <form
            className="mt-3 flex flex-wrap items-center gap-2"
            onSubmit={(e) => {
              e.preventDefault()
              if (!name.trim()) return
              void run(() => phoneServerAdd(name)).then(() => setName(''))
            }}
          >
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="My phone"
              aria-label="Name for the phone"
              className="min-w-[200px] flex-1 rounded-full bg-ink/[0.07] px-4 py-2 text-[13.5px] outline-none shadow-[inset_0_0_0_1px_var(--edge)] focus:ring-1 focus:ring-[color-mix(in_oklab,var(--accent)_60%,transparent)]"
            />
            <button type="submit" disabled={busy || !name.trim()} className="btn btn-sm">
              Add phone
            </button>
          </form>

          <p className="mt-6 text-[12.5px] leading-relaxed text-ink/55">
            In the app, add a Subsonic server with the address above and the phone’s name and password. Android: <b className="font-medium text-ink/75">Tempus</b> (free) or{' '}
            <b className="font-medium text-ink/75">Symfonium</b>. iPhone: <b className="font-medium text-ink/75">Amperfy</b> (free). Keep this PC awake while you listen at home; songs you save in the app play anywhere.
          </p>
        </div>
      )}
    </section>
  )
}

function PhoneRow({ phone, onRemove }: { phone: Phone; onRemove: () => void }) {
  const [shown, setShown] = useState(false)
  return (
    <li className="flex flex-wrap items-center gap-x-5 gap-y-1 rounded-2xl bg-ink/[0.04] px-4 py-2.5 text-[13px]">
      <span className="min-w-[120px] font-medium">{phone.name}</span>
      <span className="text-ink/55">
        Name <span className="select-text font-medium text-ink/85">{phone.user}</span>
      </span>
      <span className="text-ink/55">
        Password{' '}
        <span className="select-text font-mono font-medium text-ink/85">{shown ? phone.password : '••••••••••'}</span>
      </span>
      <span className="ml-auto flex gap-1">
        <button onClick={() => setShown((v) => !v)} className="btn-ghost btn-sm">
          {shown ? 'Hide' : 'Show'}
        </button>
        <button onClick={onRemove} className="btn-ghost btn-sm" title="This phone can't reach your library any more">
          Remove
        </button>
      </span>
    </li>
  )
}
