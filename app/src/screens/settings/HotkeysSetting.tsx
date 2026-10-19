import { invoke } from '@tauri-apps/api/core'
import { useEffect, useState } from 'react'
import { inApp } from '../../lib/engine'
import { HOTKEY_ACTIONS, keysOf, prettyKeys } from '../../lib/hotkeyKeys'
import { notify } from '../../lib/notify'

/*
  Shortcuts that work from any app (app hotkeys.rs): click one, press the keys. Esc leaves it as
  it was, Backspace switches it off. Keys another app holds already are said so.
*/

interface HotkeysNow {
  keys: Record<string, string>
  problems: { action: string; keys: string; reason: string }[]
}

export function HotkeysSetting() {
  const [now, setNow] = useState<HotkeysNow | null>(null)
  const [setting, setSetting] = useState<string | null>(null)
  useEffect(() => {
    if (inApp())
      void invoke<HotkeysNow>('get_hotkeys')
        .then(setNow)
        .catch(() => {})
  }, [])
  const save = async (action: string, keys: string) => {
    try {
      const n = await invoke<HotkeysNow>('set_hotkey', { action, keys })
      setNow(n)
      const p = n.problems.find((x) => x.action === action)
      if (p) notify(`${prettyKeys(keys)}: ${p.reason}`)
    } catch (e) {
      notify(String(e))
    }
  }
  // While one is being set, the next key press is its keys.
  useEffect(() => {
    if (!setting) return
    const key = (e: KeyboardEvent) => {
      e.preventDefault()
      e.stopPropagation()
      if (e.key === 'Escape' && !e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey) return setSetting(null)
      if ((e.key === 'Backspace' || e.key === 'Delete') && !e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey) {
        setSetting(null)
        return void save(setting, '')
      }
      const keys = keysOf(e)
      if (!keys) return
      setSetting(null)
      void save(setting, keys)
    }
    window.addEventListener('keydown', key, { capture: true })
    return () => window.removeEventListener('keydown', key, { capture: true })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [setting])
  if (!now) return null
  return (
    <div className="border-b border-edge py-4">
      <div className="text-[14px] font-medium">Shortcuts from anywhere</div>
      <div className="mt-0.5 max-w-[62ch] text-[12.5px] leading-relaxed text-ink/55">
        These work while you're in another app, or with Venuta in the tray. Click one and press the keys, with Ctrl, Alt, Shift or Win. Backspace switches one off. Your media keys work as well, without setting anything.
      </div>
      <div className="mt-3 grid grid-cols-[repeat(auto-fill,minmax(360px,1fr))] gap-x-6 gap-y-1">
        {HOTKEY_ACTIONS.map((a) => {
          const keys = now.keys[a.id] ?? ''
          const problem = now.problems.find((p) => p.action === a.id)
          const waiting = setting === a.id
          return (
            <div key={a.id} className="flex flex-wrap items-center gap-3 rounded-xl px-2 py-1.5 hover:bg-ink/[0.04]">
              <span className="min-w-[180px] flex-1 text-[13.5px]">{a.label}</span>
              {problem && <span className="text-[12px] text-ink/55">{problem.reason}</span>}
              <button
                onClick={() => setSetting(waiting ? null : a.id)}
                aria-label={`${a.label}: ${waiting ? 'press the keys' : prettyKeys(keys)}`}
                className={`min-w-[150px] rounded-full px-3.5 py-1 text-center text-[12.5px] shadow-[inset_0_0_0_1px_var(--edge)] transition-colors ${
                  waiting ? 'bg-accent text-[var(--accent-ink)]' : keys ? (problem ? 'text-ink/45 line-through' : 'bg-ink/[0.07] text-ink/85 hover:bg-ink/[0.12]') : 'text-ink/45 hover:bg-ink/[0.07]'
                }`}
              >
                {waiting ? 'Press the keys…' : prettyKeys(keys)}
              </button>
            </div>
          )
        })}
      </div>
    </div>
  )
}
