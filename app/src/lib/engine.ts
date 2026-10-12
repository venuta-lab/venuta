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

// --- Review & build -------------------------------------------------------------------

export type Confidence = 'low' | 'medium' | 'high'
export type Decision = 'approved' | 'skipped'

export interface FieldChange {
  field: string
  before: string[]
  after: string[]
  reason: string
  confidence: Confidence
}

export interface AlbumView {
  id: string
  title: string
  album_artist: string
  target_dir: string
  action: { kind: 'migrate' } | { kind: 'quarantine'; duplicate_of: string }
  confidence: Confidence
  notes: string[]
  changes: FieldChange[]
  art: { kind: 'keep' | 'missing' } | { kind: 'reencode'; max_px: number }
  tracks: number
  files: string[]
}

export interface Question {
  id: string
  text: string
  a: string
  b: string
}

export interface Settings {
  rules_dir: string | null
  new_library: string | null
}

export interface Review {
  plan: { albums: AlbumView[]; questions: Question[] }
  decisions: Record<string, Decision>
  settings: Settings
}

export interface ApplyReport {
  albums: number
  tracks_written: number
  tracks_skipped: number
  quarantined_albums: number
  errors: { album: string; file: string; message: string }[]
  journal: string
  cancelled: boolean
  elapsed_ms: number
}

export interface Run {
  journal: string
  started_ms: number
  target: string
  files: number
}

// Outside the app, decisions live in memory so the screen can still be tried.
const devDecisions: Record<string, Decision> = {}

export const review = (root: string): Promise<Review> =>
  inApp()
    ? invoke<Review>('review', { root })
    : dev<Review['plan']>('review.json').then((plan) => ({
        plan,
        decisions: { ...devDecisions },
        settings: { rules_dir: 'D:/Music-meta/rules', new_library: 'D:/Music' },
      }))

export const setDecisions = (root: string, changes: Record<string, Decision | null>) => {
  if (inApp()) return invoke<Record<string, Decision>>('set_decisions', { root, changes })
  for (const [k, v] of Object.entries(changes)) {
    if (v) devDecisions[k] = v
    else delete devDecisions[k]
  }
  return Promise.resolve({ ...devDecisions })
}

export const answerQuestion = (root: string, a: string, b: string, keep: string | null) =>
  inApp()
    ? invoke<Review>('answer_question', { root, a, b, keep })
    : Promise.reject(new Error('Answers are saved by the desktop app.'))

export const setSettings = (settings: Settings) =>
  inApp() ? invoke<void>('set_settings', { settings }) : Promise.resolve()

export function applyApproved(root: string, onProgress: (done: number, total: number) => void) {
  if (!inApp()) return Promise.reject(new Error('The library is built by the desktop app.'))
  const channel = new Channel<{ done: number; total: number }>()
  channel.onmessage = (m) => onProgress(m.done, m.total)
  return invoke<ApplyReport>('apply_approved', { root, onProgress: channel })
}

export const cancelApply = () => (inApp() ? invoke<void>('cancel_apply') : Promise.resolve())
export const listRuns = () => (inApp() ? invoke<Run[]>('list_runs') : Promise.resolve([] as Run[]))
export const undoRun = (journal: string) => invoke<number>('undo_run', { journal })

/** Friendly names for tag fields in the Review screen. */
export const FIELD_NAMES: Record<string, string> = {
  ALBUM: 'Album',
  ALBUMARTIST: 'Album artist',
  ARTIST: 'Artists (display)',
  ARTISTS: 'Artists (each)',
  COMPOSER: 'Composer',
  LYRICIST: 'Lyricist',
  CREDITS: 'Other credits',
  TITLE: 'Title',
  GENRE: 'Genre',
  LANGUAGE: 'Language',
  CATEGORY: 'Category',
  RELEASETYPE: 'Release type',
  ORIGINALYEAR: 'Original year',
  ORIGINALDATE: 'Original date',
  BARCODE: 'Barcode',
  UPC: 'UPC',
  TRACKTOTAL: 'Track total',
  TOTALTRACKS: 'Track total (alt.)',
  DISCTOTAL: 'Disc total',
  TOTALDISCS: 'Disc total (alt.)',
  REPLAYGAIN_TRACK_GAIN: 'Track volume level',
  REPLAYGAIN_TRACK_PEAK: 'Track peak',
  REPLAYGAIN_ALBUM_GAIN: 'Album volume level',
  REPLAYGAIN_ALBUM_PEAK: 'Album peak',
  LYRICS: 'Lyrics',
  UNSYNCEDLYRICS: 'Unsynced lyrics',
}

// --- Album detail & in-place edits ----------------------------------------------------

export interface TrackRow {
  path: string
  disc: number
  track: number
  title: string
  artist: string
  composer: string
  lyricist: string
  stars: number
  duration_ms: number
  sample_rate: number
  bit_depth: number
}

export interface AlbumDetail {
  album: string
  album_artist: string
  original_year: string
  genre: string
  composer: string
  lyricist: string
  tracks: TrackRow[]
}

export interface TagEdit {
  file: string
  set: [string, string[]][]
}

export interface EditReport {
  changed: number
  unchanged: number
  errors: [string, string][]
  journal: string | null
}

export async function albumDetail(root: string, dir: string): Promise<AlbumDetail> {
  if (inApp()) return invoke<AlbumDetail>('album_detail', { root, dir })
  // Outside the app: build a read-only view from the review snapshot.
  const plan = await dev<Review['plan']>('review.json')
  const a = plan.albums.find((x) => x.id === dir)
  return {
    album: a?.title ?? '',
    album_artist: a?.album_artist ?? '',
    original_year: a?.target_dir.match(/(\d{4}) - /)?.[1] ?? '',
    genre: '',
    composer: '',
    lyricist: '',
    tracks: (a?.files ?? []).map((f, i) => ({
      path: `${dir}/${f}`,
      disc: 1,
      track: i + 1,
      title: f.replace(/^\d+-?\d* - /, '').replace(/\.\w+$/, ''),
      artist: '',
      composer: '',
      lyricist: '',
      stars: 0,
      duration_ms: 0,
      sample_rate: 0,
      bit_depth: 0,
    })),
  }
}

/** Rejects with message BACKUP_NOT_CONFIRMED until the owner confirms a backup once. */
export const editTags = (root: string, edits: TagEdit[]) =>
  inApp()
    ? invoke<EditReport>('edit_tags', { root, edits })
    : Promise.reject(new Error('Edits are saved by the desktop app.'))

export const confirmBackup = () => invoke<void>('confirm_backup')

// --- Online lookups & Incoming --------------------------------------------------------

export interface EnrichReport {
  albums_looked_up: number
  albums_without_barcode: number
  musicbrainz_matches: number
  apple_matches: number
  deezer_matches: number
  lyrics_checked: number
  lyrics_found: number
  errors: string[]
  cancelled: boolean
  elapsed_ms: number
}

function withProgress<T>(cmd: string, args: Record<string, unknown>, onProgress: (d: number, t: number) => void) {
  if (!inApp()) return Promise.reject(new Error('This runs in the desktop app.'))
  const channel = new Channel<{ done: number; total: number }>()
  channel.onmessage = (m) => onProgress(m.done, m.total)
  return invoke<T>(cmd, { ...args, onProgress: channel })
}

export const lookupOnline = (root: string, onProgress: (d: number, t: number) => void) =>
  withProgress<EnrichReport>('lookup_online', { root }, onProgress)
export const cancelLookup = () => (inApp() ? invoke<void>('cancel_lookup') : Promise.resolve())

export interface IncomingView {
  folder: string
  plan: { albums: AlbumView[]; questions: Question[] }
}

export const incoming = (root: string): Promise<IncomingView> =>
  inApp()
    ? invoke<IncomingView>('incoming', { root })
    : Promise.resolve({ folder: `${root}/_Incoming`, plan: { albums: [], questions: [] } })
export const incomingLookup = (root: string, onProgress: (d: number, t: number) => void) =>
  withProgress<EnrichReport>('incoming_lookup', { root }, onProgress)
export const incomingImport = (root: string, albums: string[], onProgress: (d: number, t: number) => void) =>
  withProgress<ApplyReport>('incoming_import', { root, albums }, onProgress)
export const addToIncoming = (root: string, paths: string[]) => invoke<number>('add_to_incoming', { root, paths })
export const incomingCount = (root: string) => (inApp() ? invoke<number>('incoming_count', { root }) : Promise.resolve(0))
