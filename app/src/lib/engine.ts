// Typed wrappers over the Rust commands in app/src-tauri/src/lib.rs.
import { Channel, convertFileSrc, invoke } from '@tauri-apps/api/core'

export interface ScanReport {
  files_seen: number
  unchanged: number
  added: number
  changed: number
  removed: number
  read_errors: number
  audio_files: number
  elapsed_ms: number
}

export type Severity = 'note' | 'fix' | 'problem'

export interface Issue {
  id: string
  title: string
  why: string
  severity: Severity
  tracks: number
  albums: string[]
  examples: string[]
}

export interface AlbumSummary {
  dir: string
  title: string
  album_artist: string
  year: string | null
  tracks: number
  duration_ms: number
  cover_track: string | null
}

export interface HealthReport {
  summary: {
    tracks: number
    albums: number
    hours: number
    bytes: number
    formats: { bit_depth: number; sample_rate: number; tracks: number }[]
    healthy_tracks: number
  }
  issues: Issue[]
  albums: AlbumSummary[]
}

export interface CoverThumb {
  dir: string
  path: string
  accent: string | null
}

/** True when running inside the desktop app (engine available). */
export const inApp = () => '__TAURI_INTERNALS__' in window

// Outside the app (plain browser during UI development) the engine isn't reachable; read a
// local snapshot from app/public/_dev instead. That folder is git-ignored: it holds the
// developer's own library data and cover art.
const dev = <T,>(file: string) => fetch(`/_dev/${file}`).then((r) => r.json() as Promise<T>)

export const engineVersion = () =>
  inApp() ? invoke<string>('engine_version') : Promise.resolve('dev')
export const scanLibrary = (root: string) =>
  inApp() ? invoke<ScanReport>('scan_library', { root }) : Promise.resolve(null)
export const libraryHealth = (root: string) =>
  inApp() ? invoke<HealthReport>('library_health', { root }) : dev<HealthReport>('health.json')
export const albumCovers = (root: string, dirs: string[], size: number) =>
  inApp()
    ? invoke<CoverThumb[]>('album_covers', { root, dirs, size })
    : dev<CoverThumb[]>('covers.json').then((all) => all.filter((c) => dirs.includes(c.dir)))

export const assetUrl = (path: string) => (inApp() ? convertFileSrc(path) : path)

export interface DeepCheckReport {
  albums: number
  tracks: number
  cancelled: boolean
  elapsed_ms: number
}

/** Decode new/changed tracks; `onProgress` fires as albums finish. */
export function deepCheck(root: string, onProgress: (done: number, total: number) => void) {
  if (!inApp()) return Promise.reject(new Error('The deep check runs in the desktop app.'))
  const channel = new Channel<{ done: number; total: number }>()
  channel.onmessage = (m) => onProgress(m.done, m.total)
  return invoke<DeepCheckReport>('deep_check', { root, onProgress: channel })
}

export const cancelDeepCheck = () => (inApp() ? invoke<void>('cancel_deep_check') : Promise.resolve())

export const albumSpectrogram = (root: string, dir: string) =>
  inApp()
    ? invoke<string>('album_spectrogram', { root, dir }).then(assetUrl)
    : Promise.reject(new Error('Spectrograms are drawn by the desktop app.'))

/** Issues whose evidence is best seen as a spectrogram. */
export const SPECTRAL_ISSUES = new Set(['limited_highs', 'upsampled_hires', 'padded_24bit', 'damaged_audio'])
