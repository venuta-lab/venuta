import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useSyncExternalStore } from 'react'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { inApp } from '../../lib/engine'
import { useEscape } from '../../lib/layers'
import { notify } from '../../lib/notify'
import { check, freeId, getPacks, inspectTheme, installTheme, loadPacks, saveTheme, type PackFile } from '../../lib/packs'
import { FONT_SLOTS, fontStack, packInfo, tokensIn, type Problem } from '../../lib/packFormat'
import { setPrefs } from '../../lib/prefs'
import { styleInfo } from '../../lib/themes'
import { ThemeCard } from './ThemeCard'

/*
  Adding a theme someone shared: a .venuta-theme file (or a bare theme.json) chosen with
  Import… or dropped on the window. Nothing is added before you've seen the theme, what it
  carries, and anything wrong with it.
*/
interface Pending {
  path: string
  json: string
  files: PackFile[]
  skipped: string[]
  error: string
}

let pending: Pending | null = null
const listeners = new Set<() => void>()
const emit = () => listeners.forEach((l) => l())

export async function openImport(path: string) {
  try {
    const r = await inspectTheme(path)
    pending = { path, ...r, error: '' }
  } catch (e) {
    pending = { path, json: '', files: [], skipped: [], error: String(e) }
  }
  emit()
}

function close() {
  pending = null
  emit()
}

const kb = (n: number) => (n < 1024 * 1024 ? `${Math.max(1, Math.round(n / 1024))} KB` : `${(n / 1024 / 1024).toFixed(1)} MB`)

export function ThemeImportHost() {
  const p = useSyncExternalStore(
    (l) => {
      listeners.add(l)
      return () => {
        listeners.delete(l)
      }
    },
    () => pending,
  )
  useEscape(!!p, close)

  // A theme file dropped anywhere on the main window.
  useEffect(() => {
    if (!inApp()) return
    let unlisten: (() => void) | undefined
    void getCurrentWebview()
      .onDragDropEvent((e) => {
        if (e.payload.type !== 'drop') return
        const file = e.payload.paths.find((x) => /\.venuta-theme$/i.test(x))
        if (file) void openImport(file)
      })
      .then((u) => (unlisten = u))
    return () => unlisten?.()
  }, [])

  return <AnimatePresence>{p && <Dialog p={p} />}</AnimatePresence>
}

function Dialog({ p }: { p: Pending }) {
  // What the file's theme.json says; its id is its name in your themes.
  let raw: { id?: unknown } = {}
  try {
    raw = JSON.parse(p.json) as { id?: unknown }
  } catch {
    // Reported by the check below.
  }
  const wanted = typeof raw.id === 'string' && /^[a-z0-9][a-z0-9-]{0,63}$/.test(raw.id) ? raw.id : 'theme'
  const t = p.error ? null : check(wanted, p.json, p.files)
  const pack = t?.pack ?? null
  const problems: Problem[] = t ? t.problems : []
  const mine = getPacks().find((x) => x.id === wanted)
  const taken = !!mine
  const size = p.files.reduce((n, f) => n + f.size, 0)

  async function add(mode: 'new' | 'copy' | 'replace') {
    if (!pack) return
    const id = mode === 'copy' ? freeId(wanted) : wanted
    try {
      await installTheme(p.path, id, mode === 'replace')
      // A copy says its own name, so it isn't mistaken for the one it copies.
      if (mode === 'copy') {
        const j = JSON.parse(p.json) as Record<string, unknown>
        await saveTheme(id, JSON.stringify({ ...j, id, name: `${pack.name} (copy)` }, null, 2))
      }
      await loadPacks()
      close()
      setPrefs({ style: pack.extends, pack: id })
      notify(mode === 'replace' ? `Replaced ${pack.name}. The one it replaced is in Set aside.` : `Added ${pack.name} to your themes.`)
    } catch (e) {
      notify(`Couldn't add the theme: ${String(e)}`)
    }
  }

  const tokens = pack ? { ...tokensIn(pack, pack.modes[0]) } : {}
  if (pack) for (const slot of FONT_SLOTS) {
    const s = fontStack(pack, slot)
    if (s && pack.fonts[slot]?.startsWith('builtin:')) tokens[`font-${slot}`] = s
  }

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      className="fixed inset-0 z-[95] flex items-center justify-center bg-scrim p-6"
      onPointerDown={(e) => e.target === e.currentTarget && close()}
    >
      <motion.div
        role="dialog"
        aria-modal="true"
        aria-label="Add a theme"
        initial={{ y: 8, scale: 0.98 }}
        animate={{ y: 0, scale: 1 }}
        className="popover w-full max-w-[520px] rounded-2xl p-6"
      >
        <h2 className="title text-[20px]">{pack ? `Add ${pack.name}?` : "This theme can't be added"}</h2>
        {pack ? (
          <>
            <div className="mt-4 grid grid-cols-[200px_1fr] gap-5">
              <div className="pointer-events-none grid">
                <ThemeCard name={pack.name} preview={pack.extends} tokens={tokens} albumLight={packInfo(pack).accent === 'yours'} on={false} onPick={() => {}} />
              </div>
              <div className="min-w-0 text-[12.5px] leading-relaxed text-ink/70">
                {pack.description && <p className="text-ink/85">{pack.description}</p>}
                {pack.author && <p className="mt-1">By {pack.author}</p>}
                <p className="mt-1">
                  Built on {styleInfo(pack.extends).name}, {pack.modes.length > 1 ? 'light and dark' : pack.modes[0]}.
                </p>
                <p className="mt-1">
                  {p.files.length} {p.files.length === 1 ? 'file' : 'files'}, {kb(size)}. No code: themes can only change how Venuta looks.
                </p>
              </div>
            </div>
            {(problems.length > 0 || p.skipped.length > 0) && (
              <div className="mt-4 max-h-[160px] overflow-auto rounded-xl bg-ink/[0.04] px-4 py-3 text-[12px] leading-snug text-ink/65">
                <p className="mb-1.5 text-ink/80">Left out, so the rest still works:</p>
                <ul className="space-y-1">
                  {p.skipped.map((s) => (
                    <li key={s}>{s}</li>
                  ))}
                  {problems.map((x, i) => (
                    <li key={i}>
                      <code className="text-ink/75">{x.at}</code> {x.message}
                    </li>
                  ))}
                </ul>
              </div>
            )}
            {taken && <p className="mt-4 text-[12.5px] text-ink/70">You already have {mine?.pack?.name ?? wanted}. Add this one beside it, or replace yours (it moves to Set aside).</p>}
          </>
        ) : (
          <p className="mt-3 text-[13px] leading-relaxed text-ink/70">{p.error || problems.find((x) => x.fatal)?.message || "It isn't a Venuta theme."}</p>
        )}
        <div className="mt-6 flex flex-wrap justify-end gap-2">
          <button className="btn-ghost" onClick={close}>
            {pack ? 'Cancel' : 'Close'}
          </button>
          {pack && taken && (
            <button className="btn" onClick={() => void add('replace')}>
              Replace yours
            </button>
          )}
          {pack && (
            <button className="btn-primary" autoFocus onClick={() => void add(taken ? 'copy' : 'new')}>
              {taken ? 'Add as a copy' : 'Add theme'}
            </button>
          )}
        </div>
      </motion.div>
    </motion.div>
  )
}
