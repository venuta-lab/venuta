import { useEffect, useState } from 'react'
import { open } from '@tauri-apps/plugin-dialog'
import { engineVersion, scanLibrary, type ScanReport } from './lib/engine'

const SECTIONS = [
  { id: 'library', label: 'Library', ready: true },
  { id: 'browse', label: 'Browse', ready: false },
  { id: 'review', label: 'Review', ready: false },
  { id: 'incoming', label: 'Incoming', ready: false },
  { id: 'playlists', label: 'Playlists', ready: false },
  { id: 'cards', label: 'Cards', ready: false },
] as const

const ROOT_KEY = 'library-root'

function readStoredRoot(): string {
  try {
    return localStorage.getItem(ROOT_KEY) ?? ''
  } catch {
    return ''
  }
}

export default function App() {
  const [version, setVersion] = useState('')
  const [root, setRoot] = useState(readStoredRoot)
  const [report, setReport] = useState<ScanReport | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')

  useEffect(() => {
    engineVersion()
      .then(setVersion)
      .catch(() => setVersion('?'))
  }, [])

  async function chooseRoot() {
    const picked = await open({ directory: true, title: 'Choose your music library folder' })
    if (typeof picked !== 'string') return
    setRoot(picked)
    setReport(null)
    try {
      localStorage.setItem(ROOT_KEY, picked)
    } catch {
      // Storage unavailable: the choice is just not remembered.
    }
  }

  async function scan() {
    if (!root) return
    setBusy(true)
    setError('')
    try {
      setReport(await scanLibrary(root))
    } catch (e) {
      setError(String(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="flex h-full">
      <nav className="flex w-52 shrink-0 flex-col border-r border-line bg-panel px-3 py-4">
        <div className="px-2 pb-6 text-sm font-semibold tracking-tight">Music Librarian</div>
        {SECTIONS.map((s) => (
          <button
            key={s.id}
            disabled={!s.ready}
            className={`rounded-md px-2 py-1.5 text-left text-sm ${
              s.id === 'library'
                ? 'bg-canvas font-medium'
                : 'text-muted disabled:cursor-default disabled:opacity-50'
            }`}
          >
            {s.label}
          </button>
        ))}
        <div className="mt-auto px-2 text-xs text-muted">engine {version}</div>
      </nav>

      <main className="flex-1 overflow-auto px-10 py-8">
        <h1 className="text-2xl font-semibold tracking-tight">Library</h1>
        <p className="mt-1 text-sm text-muted">
          Scanning only reads your files. Nothing in the library is changed.
        </p>

        <div className="mt-6 flex items-center gap-3 rounded-lg border border-line bg-panel p-4">
          <div className="min-w-0 flex-1">
            <div className="text-xs uppercase tracking-wide text-muted">Library folder</div>
            <div className="truncate text-sm">{root || 'Not chosen yet'}</div>
          </div>
          <button
            onClick={chooseRoot}
            className="rounded-md border border-line px-3 py-1.5 text-sm hover:bg-canvas"
          >
            Choose…
          </button>
          <button
            onClick={scan}
            disabled={!root || busy}
            className="rounded-md bg-accent px-4 py-1.5 text-sm font-medium text-white disabled:opacity-40"
          >
            {busy ? 'Scanning…' : 'Scan'}
          </button>
        </div>

        {error && (
          <div className="mt-4 rounded-md border border-bad/40 px-4 py-3 text-sm text-bad">{error}</div>
        )}

        {report && (
          <div className="mt-6 grid grid-cols-2 gap-3 lg:grid-cols-4">
            <Stat label="Audio files" value={report.audio_files} />
            <Stat label="All files seen" value={report.files_seen} />
            <Stat
              label="Changes since last scan"
              value={report.added + report.changed + report.removed}
              note={`${report.added} added · ${report.changed} changed · ${report.removed} removed`}
            />
            <Stat
              label="Scan time"
              value={`${report.elapsed_ms} ms`}
              note={report.read_errors ? `${report.read_errors} unreadable files` : 'No read errors'}
              tone={report.read_errors ? 'bad' : 'ok'}
            />
          </div>
        )}
      </main>
    </div>
  )
}

function Stat(props: { label: string; value: number | string; note?: string; tone?: 'ok' | 'bad' }) {
  const toneClass =
    props.tone === 'bad' ? 'text-bad' : props.tone === 'ok' ? 'text-ok' : 'text-muted'
  return (
    <div className="rounded-lg border border-line bg-panel p-4">
      <div className="text-xs uppercase tracking-wide text-muted">{props.label}</div>
      <div className="mt-1 text-2xl font-semibold tabular-nums">
        {typeof props.value === 'number' ? props.value.toLocaleString() : props.value}
      </div>
      {props.note && <div className={`mt-1 text-xs ${toneClass}`}>{props.note}</div>}
    </div>
  )
}
