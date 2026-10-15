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
  untagged?: boolean
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
// local snapshot from app/dev-data (served at /_dev by vite.config.ts) instead. That folder is
// git-ignored: it holds the developer's own library data and cover art.
const dev = <T,>(file: string) => fetch(`/_dev/${file}`).then((r) => r.json() as Promise<T>)

export const engineVersion = () =>
  inApp() ? invoke<string>('engine_version') : Promise.resolve('dev')
export interface ScanProgress {
  stage: 'finding' | 'reading' | 'saving'
  done: number
  total: number
}

export function scanLibrary(root: string, onProgress: (p: ScanProgress) => void = () => {}) {
  if (!inApp()) return Promise.resolve(null)
  const channel = new Channel<ScanProgress>()
  channel.onmessage = onProgress
  return invoke<ScanReport>('scan_library', { root, onProgress: channel })
}
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
  /** The folder the songs come from. */
  source_dir: string
  /** Cover key: a folder, or "track:<path>" for one song's own art. */
  cover: string
  /** Questions this album waits for before it can be built. */
  waiting_on: string[]
  title: string
  album_artist: string
  target_dir: string
  action: { kind: 'migrate' } | { kind: 'quarantine'; duplicate_of: string }
  confidence: Confidence
  notes: string[]
  changes: FieldChange[]
  art: { kind: 'keep' | 'missing' } | { kind: 'reencode'; max_px: number }
  cover_upgrade?: boolean
  tracks: number
  files: string[]
}

export type DecisionKind = 'duplicate_songs' | 'same_artist' | 'same_album' | 'year' | 'untagged'

export interface DecisionSong {
  title: string
  path: string
  duration_ms: number
}

export interface DecisionSide {
  title: string
  subtitle: string
  covers: string[]
  facts: { label: string; value: string }[]
  songs: DecisionSong[]
  choice: string | null
}

export interface PlanDecision {
  id: string
  kind: DecisionKind
  /** Handled automatically because the evidence proves it; already applied. */
  auto: boolean
  proof: string
  answer: string | null
  question: string
  subject: string
  sides: DecisionSide[]
  choices: { id: string; label: string; hint: string }[]
  suggested: string | null
  why: string
  albums: string[]
}

export interface AnswerItem {
  id: string
  choice: string
  a?: string
  b?: string
}

export interface Settings {
  rules_dir: string | null
  new_library: string | null
  backup_confirmed?: boolean
  apple_library?: string | null
  apple_key?: string | null
  acoustid_key?: string | null
  discogs_token?: string | null
  sources?: { musicbrainz: boolean; apple: boolean; deezer: boolean; lrclib: boolean }
  /** The folder showing in the app: your library, or music being brought in. */
  open_folder?: string | null
  appearance?: Record<string, unknown> | null
  /** What closing the window does: keep playing in the tray, or quit. Unset until asked. */
  close_action?: 'tray' | 'quit' | null
  /** Your Spotify app's Client ID (not a secret), for signing in. */
  spotify_client_id?: string | null
}

export interface HomeInfo {
  /** Your library, once chosen. Venuta keeps everything it knows in its .venuta folder. */
  library: string | null
  data_dir: string
  /** A good place for a new library on this computer. */
  suggestion: string
}

export type HomeKind = 'new' | 'existing'

export const libraryHome = () =>
  inApp()
    ? invoke<HomeInfo>('library_home')
    : Promise.resolve<HomeInfo>({ library: 'C:/Music Library', data_dir: 'C:/Music Library/.venuta', suggestion: 'C:/Music Library' })
export const checkHome = (path: string) => invoke<HomeKind>('check_home', { path })
export const chooseHome = (path: string) => invoke<HomeKind>('choose_home', { path })
/** Refuses a music folder inside your library, or around it (`library`: the one about to be chosen). */
export const checkMusicFolder = (path: string, library?: string) =>
  inApp() ? invoke<void>('check_music_folder', { path, library: library ?? null }) : Promise.resolve()
export const setOpenFolder = (path: string) => (inApp() ? invoke<void>('set_open_folder', { path }) : Promise.resolve())
export const saveAppearance = (appearance: object) =>
  inApp() ? invoke<void>('save_appearance', { appearance }) : Promise.resolve()

export interface EditChange {
  /** Tags or ratings changed in place, or a song's lyrics timed (its .lrc). */
  kind: 'edit' | 'lyrics'
  journal: string
  started_ms: number
  files: number
  examples: string[]
}

export const listAnswers = (root: string) =>
  inApp() ? invoke<Record<string, string>>('list_answers', { root }) : Promise.resolve<Record<string, string>>({})
export const clearAnswerQuick = (root: string, id: string) => invoke<unknown>('clear_answer', { root, id })

export const listEdits = () => (inApp() ? invoke<EditChange[]>('list_edits') : Promise.resolve([] as EditChange[]))
export const undoEdit = (journal: string) => invoke<number>('undo_edit', { journal })

export interface Aliases {
  artists: { name: string; variants: string[] }[]
  distinct: [string, string][]
}

export const getAliases = () =>
  inApp() ? invoke<Aliases>('get_aliases') : Promise.resolve<Aliases>({ artists: [{ name: 'Madan Mohan', variants: ['Late Madan Mohan'] }], distinct: [] })
export const saveAliases = (view: Aliases) => (inApp() ? invoke<void>('save_aliases', { view }) : Promise.resolve())

export const getSettings = () =>
  inApp()
    ? invoke<Settings>('get_settings')
    : Promise.resolve<Settings>({ rules_dir: 'C:/Music Library/.venuta/rules', new_library: 'C:/Music Library' })

export interface Review {
  plan: { albums: AlbumView[]; decisions: PlanDecision[] }
  /** Albums left out of the build, by album id. */
  decisions: Record<string, Decision>
  answers: Record<string, string>
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
  /** build: a library built from a music folder; tidy: your library tidied in place. */
  kind: 'build' | 'tidy'
  journal: string
  started_ms: number
  target: string
  files: number
  /** Files that could not be built, and why. */
  failed: { file: string; message: string }[]
  /** false: stopped before the end; null: from before this was recorded. */
  finished: boolean | null
}

// Outside the app, decisions live in memory so the screen can still be tried.
const devDecisions: Record<string, Decision> = {}

export const review = (root: string): Promise<Review> =>
  inApp()
    ? invoke<Review>('review', { root })
    : dev<Review['plan']>('review.json').then((plan) => ({
        plan,
        decisions: { ...devDecisions },
        answers: { ...devAnswers },
        settings: { rules_dir: 'C:/Music Library/.venuta/rules', new_library: 'C:/Music Library' },
      }))

const devAnswers: Record<string, string> = {}

/** Answer decisions (or approve handled ones); returns the plan made again. */
export const answerDecisions = (root: string, items: AnswerItem[]): Promise<Review> => {
  if (inApp()) return invoke<Review>('answer_decisions', { root, items })
  for (const i of items) devAnswers[i.id] = i.choice
  return review(root)
}

export const setDecisions = (root: string, changes: Record<string, Decision | null>) => {
  if (inApp()) return invoke<Record<string, Decision>>('set_decisions', { root, changes })
  for (const [k, v] of Object.entries(changes)) {
    if (v) devDecisions[k] = v
    else delete devDecisions[k]
  }
  return Promise.resolve({ ...devDecisions })
}


// --- Signed-in services (Spotify) --------------------------------------------------------------

export interface AccountStatus {
  /** A Client ID is set. */
  ready: boolean
  connected: boolean
  name: string | null
}

export interface SpotifyPlaylist {
  id: string
  name: string
  owner: string
  songs: number
  /** Spotify shares the songs only of playlists you made or collaborate on. */
  readable: boolean
  image: string | null
}

export const spotifyStatus = () =>
  inApp() ? invoke<AccountStatus>('spotify_status') : Promise.resolve<AccountStatus>({ ready: false, connected: false, name: null })
/** Opens Spotify in the browser and waits (up to five minutes) for you to agree. */
export const spotifyConnect = () => invoke<string>('spotify_connect')
export const spotifyCancel = () => invoke<void>('spotify_cancel')
export const spotifyDisconnect = () => invoke<void>('spotify_disconnect')
export const spotifyPlaylists = () => invoke<SpotifyPlaylist[]>('spotify_playlists')
export const spotifyRead = (root: string, chosen: SpotifyPlaylist[]) =>
  invoke<{ lists: PreviewPlaylist[]; problems: string[] }>('spotify_read', { root, chosen })

/** Remember what closing the window does; `now` also does it. */
export const setCloseAction = (action: 'tray' | 'quit', now: boolean) => invoke<void>('set_close_action', { action, now })

/** Save only these fields, onto what is saved now. */
export const setSettings = (change: Partial<Settings>) =>
  inApp() ? invoke<void>('set_settings', { change }) : Promise.resolve()

/** Build the new library from every album that isn't left out or waiting for an answer. */
export function buildLibrary(root: string, onProgress: (done: number, total: number) => void) {
  if (!inApp()) return Promise.reject(new Error('The library is built by the desktop app.'))
  const channel = new Channel<{ done: number; total: number }>()
  channel.onmessage = (m) => onProgress(m.done, m.total)
  return invoke<ApplyReport>('build_library', { root, onProgress: channel })
}

export const cancelApply = () => (inApp() ? invoke<void>('cancel_apply') : Promise.resolve())
export const listRuns = () => (inApp() ? invoke<Run[]>('list_runs') : Promise.resolve([] as Run[]))
export const undoRun = (journal: string) => invoke<number>('undo_run', { journal })

/** Friendly names for tag fields in the Review screen. */
export const FIELD_NAMES: Record<string, string> = {
  ALBUM: 'Album',
  ALBUMARTIST: 'Album artist',
  ARTIST: 'Singers',
  ARTISTS: 'Singers (list)',
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
  discogs_matches?: number
  identified?: number
  lyrics_checked: number
  lyrics_found: number
  errors: string[]
  cancelled: boolean
  elapsed_ms: number
}

function withProgress<T>(
  cmd: string,
  args: Record<string, unknown>,
  onProgress: (d: number, t: number, note?: string) => void,
) {
  if (!inApp()) return Promise.reject(new Error('This runs in the desktop app.'))
  const channel = new Channel<{ done: number; total: number; note?: string }>()
  channel.onmessage = (m) => onProgress(m.done, m.total, m.note)
  return invoke<T>(cmd, { ...args, onProgress: channel })
}

export const lookupOnline = (root: string, onProgress: (d: number, t: number, note?: string) => void) =>
  withProgress<EnrichReport>('lookup_online', { root }, onProgress)
export const cancelLookup = () => (inApp() ? invoke<void>('cancel_lookup') : Promise.resolve())

export interface IncomingView {
  folder: string
  plan: { albums: AlbumView[]; decisions: PlanDecision[] }
}

export const incoming = (root: string): Promise<IncomingView> =>
  inApp()
    ? invoke<IncomingView>('incoming', { root })
    : Promise.resolve({ folder: `${root}/_Incoming`, plan: { albums: [], decisions: [] } })
export const incomingLookup = (root: string, onProgress: (d: number, t: number) => void) =>
  withProgress<EnrichReport>('incoming_lookup', { root }, onProgress)
export const incomingImport = (root: string, albums: string[], onProgress: (d: number, t: number) => void) =>
  withProgress<ApplyReport>('incoming_import', { root, albums }, onProgress)
export const addToIncoming = (root: string, paths: string[]) => invoke<number>('add_to_incoming', { root, paths })
export const incomingCount = (root: string) => (inApp() ? invoke<number>('incoming_count', { root }) : Promise.resolve(0))

// --- SD cards --------------------------------------------------------------------------

export interface Volume {
  root: string
  label: string
  filesystem: string
  serial: number
  removable: boolean
  total_bytes: number
  free_bytes: number
}

/** What a card carries when it isn't everything: every song any of these holds, each once. */
export interface CardPick {
  playlists: string[]
  /** Album folders, library-relative. */
  albums: string[]
  artists: string[]
}

export type CardKind = 'player' | 'gift' | 'car'

export interface Convert {
  /** Hi-res FLAC as 16-bit FLAC (still lossless). */
  hires: boolean
  /** Lossless songs as AAC (.m4a), for phones and cars. */
  aac: boolean
}

export interface Extras {
  readme: boolean
  missing: boolean
  gaps: boolean
}

export interface CardConfig {
  id: string
  name: string
  folder: string
  include: string[]
  auto_sync: boolean
  created_at: number
  player?: PlayerId
  test_results?: Record<string, boolean>
  pick?: CardPick
  kind: CardKind
  convert: Convert
  extras: Extras
  serial?: number
}

/** What you choose for a card; `pick` null or empty means everything. */
export interface CardSettings {
  name: string
  include: string[]
  pick: CardPick | null
  auto_sync: boolean
  player?: PlayerId
  kind: CardKind
  convert: Convert
  extras: Extras
}

/** Something a card can carry, with how much of the library it is. */
export interface Choice {
  id: string
  name: string
  detail: string
  songs: number
  bytes: number
}

export interface Choices {
  playlists: Choice[]
  albums: Choice[]
  artists: Choice[]
}

export interface Estimate {
  songs: number
  bytes: number
  room: number
  reserve: number
  fits: boolean
}

export interface CardStatus {
  volume: Volume
  card: CardConfig | null
  last_sync: number | null
  files: number
}

export interface SyncPlan {
  card: CardConfig
  copy: number
  patch: number
  moves: number
  adopt: number
  delete: number
  unchanged: number
  bytes_to_copy: number
  bytes_freed: number
  free_bytes: number
  total_bytes: number
  reserve_bytes: number
  fits: boolean
  songs: number
  warnings: string[]
}

export interface SyncReport {
  copied: number
  patched: number
  moved: number
  adopted: number
  deleted: number
  bytes_written: number
  errors: [string, string][]
  cancelled: boolean
  elapsed_ms: number
}

export const listCards = () => (inApp() ? invoke<CardStatus[]>('list_cards') : Promise.resolve([] as CardStatus[]))
export const libraryFolders = (root: string) =>
  inApp() ? invoke<string[]>('library_folders', { root }) : Promise.resolve([] as string[])
export type PlayerId = 'standard' | 'poweramp' | 'snowsky_disc' | 'echo_mini' | 'astell_kern'

export interface PlayerInfo {
  id: PlayerId
  label: string
  notes: string[]
}

export const players = () =>
  inApp()
    ? invoke<PlayerInfo[]>('players')
    : Promise.resolve<PlayerInfo[]>([
        { id: 'poweramp', label: 'Poweramp (Android)', notes: ['Several singers written as one name list, separated by “;”.'] },
        { id: 'echo_mini', label: 'Snowsky Echo Mini', notes: ['Covers re-saved as standard JPEG, at most 600 px.'] },
      ])

export interface TestItem {
  id: string
  file: string
  checks: string
}

export const playerTestWrite = (root: string, card: string) => invoke<TestItem[]>('player_test_write', { root, card })
export const playerTestRemove = (card: string) => invoke<void>('player_test_remove', { card })
export const playerTestSave = (card: string, results: Record<string, boolean>) =>
  invoke<CardConfig>('player_test_save', { card, results })

/** Put an album on "To get" to find a better copy. Returns how many albums the list holds. */
export const wantAdd = (album: string, artist: string, why: string) =>
  invoke<number>('want_add', { album, artist, why })

export const setupCard = (card: string, settings: CardSettings) => invoke<CardConfig>('setup_card', { card, settings })
export const reidentifyCard = (card: string) => invoke<CardConfig>('reidentify_card', { card })
export const cardChoices = (root: string) => invoke<Choices>('card_choices', { root })
export const cardEstimate = (root: string, card: string, settings: CardSettings) =>
  invoke<Estimate>('card_estimate', { root, card, settings })
export const planCard = (root: string, card: string) => invoke<SyncPlan>('plan_card', { root, card })
export function syncCard(root: string, card: string, onProgress: (d: number, t: number, bytes: number) => void) {
  const channel = new Channel<{ done: number; total: number; bytes: number }>()
  channel.onmessage = (m) => onProgress(m.done, m.total, m.bytes)
  return invoke<SyncReport>('sync_card', { root, card, onProgress: channel })
}
export const cancelSync = (card: string) => invoke<void>('cancel_sync', { card })
export const ejectCard = (card: string) => invoke<void>('eject_card', { card })

export const gb = (bytes: number) =>
  bytes >= 1e9 ? `${(bytes / 1e9).toFixed(1)} GB` : `${Math.max(1, Math.round(bytes / 1e6))} MB`

// --- Playlists, Apple import, want-list -------------------------------------------------

export interface PlaylistSummary {
  name: string
  file: string
  smart: boolean
  tracks: number
  /** Songs still to get (not in the library, not online). */
  missing: number
  /** Songs not in the library that play online. */
  online: number
  /** "Spotify 2026-10-13": where it came from, if imported. */
  source: string | null
  /** Which playlist at the source ("spotify:…", "apple:Hindi"), when known: it can be updated from there. */
  source_id: string | null
  /** Up to four album folders from the top, for the mosaic cover. */
  dirs: string[]
  duration_ms: number
  /** When its file last changed, seconds since 1970. */
  modified: number
}

/** A song a playlist wants that isn't in the library. */
export interface WantEntry {
  title: string
  artist: string
  album: string
  duration_ms: number
  isrc?: string
  /** The audio of a file it had that went away: the same audio, back anywhere, is this song. */
  md5?: string
  /** Library files you said are not this song. */
  not?: string[]
}

/** Whether a song plays from your library or from the online catalog ('local_preferred': an
 *  online song you have, playing from your file). */
export type Availability = 'local' | 'online' | 'local_preferred'

/** A stable id for a song. Local ones use their library path; online ones a provider id.
 *  Stream URLs never appear here. */
export type PlayableId = { kind: 'local'; path: string } | { kind: 'online'; provider: string; track_id: string }

/** A song ready to play, local first (see `playlistQueue`). */
export interface Playable {
  id: PlayableId
  title: string
  artists: string[]
  album: string | null
  duration_ms: number | null
  artwork_url: string | null
  availability: Availability | null
}

/** A song in the online catalog, as a search finds it. `artwork_url` is a picture, not audio. */
export interface OnlineTrack {
  provider: string
  track_id: string
  title: string
  artists: string[]
  album: string | null
  album_id: string | null
  duration_ms: number | null
  artwork_url: string | null
  isrc: string | null
}

export interface OnlineAlbum {
  provider: string
  album_id: string
  title: string
  artists: string[]
  track_count: number | null
  release_date: string | null
  artwork_url: string | null
}

export interface OnlineArtist {
  provider: string
  artist_id: string
  name: string
  artwork_url: string | null
}

export interface OnlineResults {
  tracks: OnlineTrack[]
  albums: OnlineAlbum[]
  artists: OnlineArtist[]
}

/** An online-catalog song a playlist holds by its stable id (never a stream address).
 *  `artwork_url` is an optional cover picture only. */
export interface OnlineEntry {
  provider: string
  track_id: string
  title: string
  artists?: string[]
  album?: string
  duration_ms?: number
  isrc?: string
  artwork_url?: string
}

/** A library song that may be one a playlist waits for. */
export interface Maybe {
  path: string
  title: string
  artist: string
  album: string
  duration_ms: number
  why: string
}

export interface PlaylistEntry {
  path: string
  title: string
  artist: string
  album: string
  duration_ms: number
  audio_md5: string | null
  missing: boolean
  want?: WantEntry
  maybe?: Maybe
  /** An online song: plays online while `missing`, from `path` when the library has it. */
  online?: OnlineEntry
}

/** One line of a playlist to save: a library song, one still wanted, or one online. */
export type PlaylistItem = { path: string; source?: WantEntry } | { want: WantEntry } | { online: OnlineEntry }

/** A song from elsewhere, as an import read it. */
export interface ForeignSong {
  title: string
  artist: string
  album: string
  duration_ms: number | null
  isrc: string | null
  file?: string
  youtube?: string
}

export interface MatchCandidate {
  path: string
  title: string
  artist: string
  album: string
  duration_ms: number
}

export interface PreviewItem {
  foreign: ForeignSong
  found: MatchCandidate | null
  grade: 'exact' | 'close' | null
  why: string
  /** Stable online id when not in the library (Takeout / catalog hit). Never a stream URL. */
  online?: OnlineEntry | null
}

/** A playlist you have that an import updates, and what the update does to it. */
export interface UpdateTarget {
  name: string
  /** Found by where it came from, not only by its name. */
  linked: boolean
  songs: number
  here: number
  after: number
  after_here: number
  /** Songs the source has that it doesn't yet. */
  added: number
  /** Songs you added, which stay. */
  yours: number
  /** Songs the source dropped since last time (they go). */
  removed: number
  /** Songs you took out, which stay out. */
  skipped: number
}

export interface PreviewPlaylist {
  name: string
  source: string
  source_id: string | null
  exists: boolean
  target: UpdateTarget | null
  items: PreviewItem[]
  exact: number
  close: number
  /** Not in library and not online — true To get. */
  missing: number
  /** Not in library; will write as online (stable id). */
  online: number
}

export interface ImportChoice {
  name: string
  source: string
  source_id: string | null
  items: PlaylistItem[]
  /** Update this playlist you have instead of writing a new one (nothing of yours is lost). */
  update: string | null
  replace: boolean
}

export interface ImportWritten {
  name: string
  songs: number
  wanted: number
  /** Songs written as online (stable id). */
  online?: number
  /** It updated a playlist you had: what changed, and the copy kept of how it was. */
  updated: boolean
  added: number
  removed: number
  copy: string | null
}

export interface ImportDone {
  written: ImportWritten[]
  wanted: number
}

/** A source playlist you have fewer songs of (an older import kept fewer), or never brought over. */
export interface BehindSource {
  name: string
  yours: string | null
  have: number
  source_songs: number
}

export type ImportRead = { lists: PreviewPlaylist[]; problems: string[] }

export interface Condition {
  field: string
  op: string
  value: string
}

export interface SmartRule {
  name: string
  match: 'all' | 'any'
  conditions: Condition[]
  limit: number | null
}

export interface WantedAlbum {
  album: string
  artist: string
  tier: 1 | 2 | 3
  why: string
  saved_tracks: number
  playlists: string[]
  /** On the list by itself (you added it), not because a playlist wants its songs. */
  kept?: boolean
}

export interface PlaylistsView {
  playlists: PlaylistSummary[]
  smart: SmartRule[]
  smart_counts: Record<string, number>
  smart_dirs: Record<string, string[]>
  folder: string
  wanted: WantedAlbum[]
  apple_ready: boolean
}

export const playlists = (root: string) =>
  inApp()
    ? invoke<PlaylistsView>('playlists', { root })
    : Promise.resolve<PlaylistsView>({ playlists: [], smart: [], smart_counts: {}, smart_dirs: {}, folder: '', wanted: [], apple_ready: false })
export const playlistTracks = (root: string, name: string) => invoke<PlaylistEntry[]>('playlist_tracks', { root, name })
/** Where a deleted playlist went and where it was: what Undo needs. */
export interface DeletedPlaylist {
  aside: string
  was: string
}
export const playlistDelete = (root: string, name: string) => invoke<DeletedPlaylist>('playlist_delete', { root, name })
export const playlistUndelete = (root: string, name: string, gone: DeletedPlaylist) =>
  invoke<void>('playlist_undelete', { root, name, aside: gone.aside, was: gone.was })
export const playlistRename = (root: string, from: string, to: string) =>
  invoke<void>('playlist_rename', { root, from, to })
export const smartSave = (root: string, rule: SmartRule) => invoke<void>('smart_save', { root, rule })
export const playlistSaveItems = (root: string, name: string, items: PlaylistItem[]) =>
  invoke<void>('playlist_save_items', { root, name, items })
/** Adds songs (skipping ones already in it); makes the playlist if it's new. Returns how many were added. */
export const playlistAdd = (root: string, name: string, items: PlaylistItem[]) => invoke<number>('playlist_add', { root, name, items })
/** A playlist as the player gets it: each song from your file when it's here, else online; songs to get left out. */
export const playlistQueue = (root: string, name: string) => invoke<Playable[]>('playlist_queue', { root, name })

/** Songs, albums and artists in the online catalog for these words (empty outside the app). */
export const onlineSearch = (query: string) =>
  inApp() ? invoke<OnlineResults>('online_search', { query }) : Promise.resolve<OnlineResults>({ tracks: [], albums: [], artists: [] })
/** An online album's songs, in order. */
export const onlineAlbumTracks = (albumId: string) => invoke<OnlineTrack[]>('online_album_tracks', { albumId })

/** Synced lyrics for an online song (LRCLIB by title/artist). Display only; not saved. */
export interface OnlineLyrics {
  lines: [number, string][]
  text: string
}
export const onlineLyrics = (title: string, artist: string, album: string, durationMs: number) =>
  inApp()
    ? invoke<OnlineLyrics>('online_lyrics', {
        title,
        artist,
        album,
        durationMs,
      }).then((r) => ({
        lines: (r.lines ?? []).map((pair) => {
          const a = pair as unknown as number[] | [number, string]
          return [Number(a[0]), String(a[1] ?? '')] as [number, string]
        }),
        text: r.text ?? '',
      }))
    : Promise.resolve<OnlineLyrics>({ lines: [], text: '' })

/** Playback catalog health for Settings (never includes stream URLs). */
export interface OnlineCatalogStatus {
  live_env: boolean
  live_provider_built: boolean
  stream_tool_available: boolean
  stream_tool_kind: string
  stream_tool_detail: string
  summary: string
  /** off | live | live_no_tool | fixture_only */
  mode: string
}
/** Download/install the stream tool (yt-dlp) into the user data folder if missing. */
export const onlineEnsureStreamTool = () =>
  inApp() ? invoke<string>('online_ensure_stream_tool') : Promise.resolve('')

export const onlineCatalogStatus = () =>
  inApp()
    ? invoke<OnlineCatalogStatus>('online_catalog_status')
    : Promise.resolve<OnlineCatalogStatus>({
        live_env: false,
        live_provider_built: false,
        stream_tool_available: false,
        stream_tool_kind: '',
        stream_tool_detail: '',
        summary: 'Open the desktop app to see online catalog status.',
        mode: 'off',
      })
export const playlistDuplicate = (root: string, name: string) => invoke<string>('playlist_duplicate', { root, name })
export const playlistExport = (root: string, name: string, to: string) => invoke<number>('playlist_export', { root, name, to })
export const playlistRefind = (root: string, name: string) => invoke<number>('playlist_refind', { root, name })
export const importPreview = (root: string, files: string[], link: string | null, apple: boolean) =>
  invoke<ImportRead>('import_preview', { root, files, link, apple })
/** Read a playlist's source again, for an update reviewed like an import. */
export const playlistUpdateRead = (root: string, name: string) => invoke<ImportRead>('playlist_update_read', { root, name })
export const playlistSourcesCheck = (root: string) =>
  inApp() ? invoke<BehindSource[]>('playlist_sources_check', { root }) : Promise.resolve<BehindSource[]>([])
/** Search the web for these words in your browser. */
export const searchWeb = (words: string) => invoke<void>('search_web', { words })
export const playlistRestore = (root: string, name: string, copy: string) => invoke<void>('playlist_restore', { root, name, copy })
export const importApply = (root: string, choices: ImportChoice[]) => invoke<ImportDone>('import_apply', { root, choices })
export const playlistShelf = (root: string) =>
  inApp() ? invoke<PlaylistSummary[]>('playlist_shelf', { root }) : Promise.resolve<PlaylistSummary[]>([])
export const wantRemove = (album: string, artist: string) => invoke<void>('want_remove', { album, artist })
export const wantedExport = (root: string, to: string) => invoke<number>('wanted_export', { root, to })
export const ratePlaylist = (root: string, name: string, stars: number) =>
  invoke<EditReport>('rate_playlist', { root, name, stars })

// --- Your library, tidied in place ------------------------------------------------------------

export interface TidyAlbum {
  id: string
  title: string
  album_artist: string
  from_dir: string
  to_dir: string
  moves: number
  retags: number
  fields: string[]
  set_aside: boolean
  /** Why it can't change (another file is already where it would go). */
  blocked: string | null
}

export interface TidyPreview {
  albums: TidyAlbum[]
  moves: number
  retags: number
  blocked: number
}

export interface TidyReport {
  albums: number
  moved: number
  retagged: number
  errors: { album: string; file: string; message: string }[]
  journal: string
  cancelled: boolean
}

export interface LayoutTrial {
  albums: number
  examples: { from: string; to: string }[]
}

const emptyPreview: TidyPreview = { albums: [], moves: 0, retags: 0, blocked: 0 }

export const tidyPreview = (root: string) =>
  inApp() ? invoke<TidyPreview>('tidy_preview', { root }) : Promise.resolve(emptyPreview)
export function tidyApply(root: string, albums: string[], onProgress: (done: number, total: number) => void) {
  if (!inApp()) return Promise.reject(new Error('Your library is changed by the desktop app.'))
  const channel = new Channel<{ done: number; total: number }>()
  channel.onmessage = (m) => onProgress(m.done, m.total)
  return invoke<TidyReport>('tidy_apply', { root, albums, onProgress: channel })
}
export const tidyUndo = (root: string, journal: string) => invoke<number>('tidy_undo', { root, journal })
export const getLayout = () =>
  inApp()
    ? invoke<string>('get_layout')
    : Promise.resolve('{category}/{subfolder}/{language}/{albumartist}/{album} ({year})/{disc}{track} - {title}')
export const setLayout = (template: string) => (inApp() ? invoke<void>('set_layout', { template }) : Promise.resolve())
export const tryLayout = (root: string, template: string) =>
  inApp()
    ? invoke<LayoutTrial>('try_layout', { root, template })
    : Promise.resolve<LayoutTrial>({ albums: 0, examples: [] })

// --- Listening -------------------------------------------------------------------------------

export interface CatalogTrack {
  path: string
  dir: string
  title: string
  artists: string[]
  album: string
  album_artist: string
  disc: number
  track: number
  duration_ms: number
  sample_rate: number
  bit_depth: number
  format: string
  year: string
  genre: string
  category: string
  lyrics: boolean
  stars: number
  /** ReplayGain from the tags. */
  rg: Gain
}

export interface Gain {
  track_db: number | null
  album_db: number | null
  track_peak: number | null
  album_peak: number | null
}

export const musicCatalog = (root: string) =>
  inApp() ? invoke<CatalogTrack[]>('music_catalog', { root }) : Promise.resolve([] as CatalogTrack[])

/** One song's lyrics as lines to search (no times, each line once). */
export interface SongWords {
  path: string
  lines: string[]
}

export const allLyrics = (root: string) =>
  inApp() ? invoke<SongWords[]>('all_lyrics', { root }) : Promise.resolve([] as SongWords[])

/** "FLAC 24/96", "MP3", "FLAC 16/44.1". */
export function quality(t: Pick<CatalogTrack, 'format' | 'bit_depth' | 'sample_rate'>): string {
  const khz = t.sample_rate ? (t.sample_rate / 1000).toFixed(t.sample_rate % 1000 ? 1 : 0) : ''
  return t.format === 'FLAC' || t.format === 'WAV' || t.format === 'AIFF'
    ? `${t.format} ${t.bit_depth}/${khz}`
    : t.format
}

/** Above CD quality: more than 16 bits or more than 48 kHz. */
export const isHiRes = (t: Pick<CatalogTrack, 'bit_depth' | 'sample_rate' | 'format'>) =>
  t.format === 'FLAC' && (t.bit_depth > 16 || t.sample_rate > 48000)

export interface ListeningSummary {
  /** Album folders, most recently played first. */
  recent: string[]
  /** Album folders by plays, most first. */
  top: [string, number][]
  /** Plays per song (library-relative path). */
  counts: Record<string, number>
  last_played: Record<string, number>
  /** Playlists played from, most recent first, with when (ms). */
  recent_playlists: [string, number][]
}

export const listeningSummary = (root: string) =>
  inApp()
    ? invoke<ListeningSummary>('listening_summary', { root })
    : Promise.resolve<ListeningSummary>({ recent: [], top: [], counts: {}, last_played: {}, recent_playlists: [] })
