import { invoke } from '@tauri-apps/api/core'
import { useEffect, useState } from 'react'
import { getSettings, inApp, setSettings } from '../../lib/engine'
import { notify } from '../../lib/notify'

/*
  A proxy for every online request (the catalog, streams, lyrics, covers, sign-in), for networks
  that only let traffic out that way. Used from the next start; Restart now starts again with it.
*/

/** The same rule as the engine's `proxy_address`: a known scheme, a host and a port. */
const looksRight = (s: string) => /^(https?|socks5h?):\/\/([^@\s/]+@)?[A-Za-z0-9.\-[\]:]+:\d{1,5}\/?$/i.test(s.trim())

export function ProxySetting() {
  const [saved, setSaved] = useState<string | null>(null)
  const [value, setValue] = useState('')
  const [changed, setChanged] = useState(false)
  useEffect(() => {
    if (!inApp()) return
    void getSettings().then((s) => {
      setSaved(s.proxy ?? '')
      setValue(s.proxy ?? '')
    })
  }, [])
  const save = async (to: string) => {
    const v = to.trim()
    if (v && !looksRight(v)) {
      notify('A proxy is written like http://host:port or socks5://host:port.')
      return
    }
    await setSettings({ proxy: v || null })
    setSaved(v)
    setChanged(true)
    notify(v ? 'Saved. Venuta uses the proxy from its next start.' : 'Saved. Venuta goes online directly from its next start.')
  }
  if (saved === null) return null
  return (
    <div className="flex flex-wrap items-center gap-4 border-b border-edge py-4">
      <div className="min-w-[220px] flex-1">
        <div className="text-[14px] font-medium">Proxy</div>
        <div className="mt-0.5 text-[12.5px] leading-relaxed text-ink/55">
          Only for networks that let traffic out through a proxy (some offices, colleges, hostels). Every online request goes through it: songs, search, lyrics, covers and sign-in. Leave empty to go online directly.
        </div>
      </div>
      <form
        className="flex items-center gap-2"
        onSubmit={(e) => {
          e.preventDefault()
          void save(value)
        }}
      >
        <input value={value} onChange={(e) => setValue(e.target.value)} placeholder="http://host:port" aria-label="Proxy address" spellCheck={false} className="field h-9 w-[230px] rounded-full px-4 text-[13px]" />
        <button type="submit" disabled={value.trim() === saved} className="btn btn-sm">
          Save
        </button>
        {changed && (
          <button type="button" onClick={() => void invoke('restart_app')} className="btn-primary btn-sm">
            Restart now
          </button>
        )}
      </form>
    </div>
  )
}
