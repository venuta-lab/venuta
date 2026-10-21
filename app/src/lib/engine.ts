// Typed wrappers over the Rust commands in app/src-tauri/src/lib.rs.
import { Channel, convertFileSrc, invoke } from '@tauri-apps/api/core'
import type { YtmChoices as YtmAccountChoices } from './ytmChoiceWords'
import type { Vote } from './ytmVotes'
import type { MenuFeedback, Press, TrackMenu } from './ytmSongMenu'

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
  /** How listening goes (see lib/listening.ts). */
  listening?: Listening
  /** A proxy for every online request, used from the next start. */
  proxy?: string | null
  /** Which new versions are offered: releases, or pre-releases too. */
  update_channel?: 'stable' | 'beta' | null
}

/** How listening goes: what plays next, what is never offered, what is remembered. */
export interface Listening {
  /** When the queue runs out, keep playing songs like the last one. */
  autoplay: boolean
  /** Hybrid: a song YouTube Music queues that is in your library as the same song plays from
   *  your file, in the same place. Off: everything streams. */
  play_my_copy: boolean
  /** Open Now Playing when you press play on something. */
  open_player_on_play: boolean
  /** Artists radio, autoplay and suggestions never offer. */
  blocked_artists: { name: string; artist_id: string | null }[]
  /** Online songs you disliked (`provider:id`). */
  disliked: string[]
  hide_videos: boolean
  /** Keep a history of what you play. */
  history: boolean
  /** 'auto': the best a song offers; 'low': data saver. */
  stream_quality: 'auto' | 'low'
  /** The country the online catalog answers for; '' for the computer's own region. */
  catalog_region: string
  /** The lyrics sources in your order, "-id" for one switched off. */
  lyrics_sources: string[]
  /** Say each new song in a Windows notification while Venuta is in the background. */
  song_notifications: boolean
  /** Old, for the whole library: each account has its own now (lib/ytmChoices.ts). Never used. */
  sync_likes: boolean
  /** Old, for the whole library: each account has its own now (lib/ytmChoices.ts). Never used. */
  report_plays: boolean
  /** Old, for the whole library: each account is asked now (lib/ytmChoices.ts). Never used. */
  ytm_offered: boolean
  /** Only your own music, and nothing online (lib/localOnly.ts). */
  local_only: boolean
  /** Listening together: the name others see ('' for this computer's user name). */
  together_name: string
  /** Listening together through a relay only, so others never see your IP address. */
  together_hide_ip: boolean
  /** Listening together through your own relay (https://...), not the public ones. */
  together_relay: string | null
  /** Each output's delay (Bluetooth), by output id, milliseconds. */
  together_delay: Record<string, number>
  /** What you pinned to the top of Home (lib/shortcuts.ts). */
  shortcuts: unknown[]
  /** Artists you blocked stay out of what the catalog picks; off, they can come again (the list is kept). */
  hide_blocked: boolean
}

/** Where a queue of the catalog's starts: the endpoint it put on a button, a tuning chip or the
 *  end of a list, kept as it came and sent back as it is (its `params` are opaque). */
export interface RadioEndpoint {
  video_id?: string | null
  playlist_id?: string | null
  params?: string | null
  index?: number | null
  playlist_set_video_id?: string | null
}

/** A tuning chip over a radio's queue ("All", "Familiar", "Deep cuts", a mood…). */
export interface RadioChip {
  text: string
  /** The one in effect, as the catalog marks it. */
  selected: boolean
  /** What choosing it asks for: its songs replace what is coming up. */
  endpoint: RadioEndpoint
}

/** A radio, or a page of one: the catalog's queue as it sent it (its order, repeats and all),
 *  and what keeps the same session going (see lib/radioSession.ts). */
export interface OnlineRadio {
  /** The radio's own list id. */
  playlist_id: string | null
  /** Its songs; on a radio of a song, the song it opens with is left out (it is playing). */
  tracks: OnlineTrack[]
  /** What the catalog's player calls it ("Kitni Baatein Radio"). */
  title?: string | null
  /** What was asked for: sent again with `continuation` for the next page. */
  endpoint?: RadioEndpoint | null
  /** The next page of the same radio; null on its last. */
  continuation?: string | null
  /** The tuning chips (a radio's first page only). */
  chips?: RadioChip[]
  /** What Autoplay goes on with when this runs out. */
  continue_with?: RadioEndpoint | null
  /** The catalog's Related page for the song it is about. */
  related_id?: string | null
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

// --- Phone server (Wi-Fi) ------------------------------------------------------------------

export interface Phone {
  name: string
  user: string
  password: string
}
export interface PhoneServerStatus {
  on: boolean
  port: number
  /** This PC's address on the network (none when it isn't on one). */
  address: string | null
  host: string | null
  phones: Phone[]
}
export const phoneServerStatus = () => invoke<PhoneServerStatus>('phone_server_status')
export const phoneServerStart = (root: string) => invoke<PhoneServerStatus>('phone_server_start', { root })
export const phoneServerResume = (root: string) => invoke<PhoneServerStatus>('phone_server_resume', { root })
export const phoneServerStop = () => invoke<PhoneServerStatus>('phone_server_stop')
export const phoneServerAdd = (name: string) => invoke<PhoneServerStatus>('phone_server_add', { name })
export const phoneServerRemove = (user: string) => invoke<PhoneServerStatus>('phone_server_remove', { user })

export const gb = (bytes: number) =>
  bytes >= 1e9 ? `${(bytes / 1e9).toFixed(1)} GB` : `${Math.max(1, Math.round(bytes / 1e6))} MB`

// --- Playlists, Apple import, want-list -------------------------------------------------

export interface PlaylistSummary {
  name: string
  file: string
  smart: boolean
  /** It is your Liked songs (whatever it is called). */
  liked?: boolean
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
/** 'room': a friend's file, streamed from the host of the room you're in (shown only). */
export type Availability = 'local' | 'online' | 'local_preferred' | 'room'

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
  // Ranking hints from the catalog (search results only; may be missing on older rows).
  /** "ATV" (the artist's own audio), "OMV" (official music video), "UGC" (someone's upload),
   *  "PODCAST_EPISODE", or another catalog word. */
  video_type?: string | null
  /** Plays (songs) or views (videos). */
  plays?: number | null
  /** The catalog's own top result for this search. */
  is_top?: boolean
  /** Its place among the unfiltered search's songs and videos (0 = first). */
  shelf_rank?: number | null
  /** What its own YouTube Music menu offers the account, where it was listed signed in
   *  (lib/ytmSongMenu.ts): save to that library, pin, not interested. */
  menu?: TrackMenu | null
  /** The lead artist's channel (`UC…`), when the catalog linked one: what Follow follows. */
  artist_id?: string | null
}

export interface OnlineAlbum {
  provider: string
  album_id: string
  title: string
  artists: string[]
  track_count: number | null
  release_date: string | null
  artwork_url: string | null
  /** "Album", "Single", "EP"… as the catalog labels it. */
  kind?: string | null
  /** Its card's "Not interested" (Home, signed in), in YouTube Music's words. */
  feedback?: MenuFeedback[]
}

export interface OnlineArtist {
  provider: string
  artist_id: string
  name: string
  artwork_url: string | null
  /** Monthly audience, when the catalog shows it. */
  audience?: number | null
}

export interface OnlineResults {
  tracks: OnlineTrack[]
  albums: OnlineAlbum[]
  artists: OnlineArtist[]
  /** Where more songs start (`onlineSearchTracks`); null when that's all. A paging token only. */
  tracks_cursor?: string | null
}

/** An online artist's page (ids and what to show; never a stream address). */
export interface OnlineArtistPage {
  provider: string
  artist_id: string
  name: string
  artwork_url: string | null
  audience: number | null
  top_songs: OnlineTrack[]
  /** More of their songs, the top ones left out. */
  songs: OnlineTrack[]
  albums: OnlineAlbum[]
  singles: OnlineAlbum[]
  similar: OnlineArtist[]
  /** Who they are, in a paragraph or two. */
  description?: string | null
  /** "6.52M subscribers". */
  subscribers?: string | null
  /** The wide picture across the top of their page. */
  banner_url?: string | null
  /** Their radio (a mix of them and artists like them). */
  radio_id?: string | null
  /** Their page's radio button, as the catalog put it there. */
  radio?: RadioEndpoint | null
  /** Their page's Shuffle button, as the catalog put it there. */
  shuffle?: RadioEndpoint | null
  /** Every row of their page in the catalog's order (Albums, Singles & EPs, Videos…). */
  shelves?: OnlineShelf[]
}

/** An online album's whole page: what it is, who made it, its songs and the rows under them. */
export interface OnlineAlbumPage {
  album: OnlineAlbum
  /** Who made it, in credit order, each with their online page when the catalog links one. */
  artists: { name: string; artist_id: string | null }[]
  /** "Album • 2026", "EP • 2025". */
  subtitle: string | null
  /** "18 songs • 1 hour, 8 minutes". */
  length: string | null
  description: string | null
  explicit: boolean
  tracks: OnlineTrack[]
  shelves: OnlineShelf[]
  /** The album's own list in the catalog (`OLAK5uy_…`): what Autoplay asks for when it ends. */
  playlist_id?: string | null
  /** Its page's Start radio, as the catalog put it there. */
  radio?: RadioEndpoint | null
}

/** A titled row of cards on an online page, and where its "See all" leads. */
export interface OnlineShelf {
  title: string
  items: OnlineSuggestion[]
  more: OnlineMore | null
}

/** One shelf of a song's Related page, in the catalog's order: songs to play, albums, artists
 *  and playlists to open, or words about the artist. */
export interface OnlineRelatedShelf {
  title: string
  tracks: OnlineTrack[]
  items: OnlineSuggestion[]
  /** "About the artist". */
  about: string | null
}

/** The catalog page behind a row's "See all". */
export interface OnlineMore {
  browse_id: string
  params: string | null
}

/** A further page of online songs, and where the next one starts. */
export interface OnlineTrackPage {
  tracks: OnlineTrack[]
  next_cursor: string | null
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
/** A new, empty playlist; refused when one with that name is there. */
export const playlistCreate = (root: string, name: string) => invoke<void>('playlist_create', { root, name })
export const playlistUndelete = (root: string, name: string, gone: DeletedPlaylist) =>
  invoke<void>('playlist_undelete', { root, name, aside: gone.aside, was: gone.was })
export const playlistRename = (root: string, from: string, to: string) =>
  invoke<void>('playlist_rename', { root, from, to })
export const smartSave = (root: string, rule: SmartRule) => invoke<void>('smart_save', { root, rule })
export const playlistSaveItems = (root: string, name: string, items: PlaylistItem[]) =>
  invoke<void>('playlist_save_items', { root, name, items })
/** Adds songs (skipping ones already in it); makes the playlist if it's new. Returns how many were added. */
/** Add songs to a playlist (made if new); ones already in it are skipped unless `again`. Returns how many were added. */
export const playlistAdd = (root: string, name: string, items: PlaylistItem[], again = false) =>
  invoke<number>('playlist_add', { root, name, items, again })
/** A playlist as the player gets it: each song from your file when it's here, else online; songs to get left out. */
export const playlistQueue = (root: string, name: string) => invoke<Playable[]>('playlist_queue', { root, name })

/** Songs, albums and artists in the online catalog for these words (empty outside the app). */
/** Which search box asks. A newer search from the same box replaces the one before it (which
 *  fails with `SearchCancelled`); other boxes' searches carry on. */
export type SearchBox = 'music' | 'palette' | 'add-songs' | 'pick' | 'more-by' | 'artist-page' | 'radio'

/** A search replaced by a newer one from the same box: ignore it, it is not "nothing found". */
export class SearchCancelled extends Error {
  constructor() {
    super('Search replaced by a newer one.')
    this.name = 'SearchCancelled'
  }
}
export const isSearchCancelled = (e: unknown) => e instanceof SearchCancelled

const cancelled = (e: unknown) => {
  throw String(e) === 'online-search-cancelled' ? new SearchCancelled() : e
}

/** `typed`: the person typed these words in the Search box. Only such a search may go as the
 *  YouTube Music account (with Signed-in search on); Venuta's own look-ups leave it false. */
export const onlineSearch = (query: string, searchBox: SearchBox = 'music', typed = false) =>
  inApp()
    ? invoke<OnlineResults>('online_search', { query, searchBox, typed }).catch(cancelled)
    : Promise.resolve<OnlineResults>({ tracks: [], albums: [], artists: [] })

/** More online songs for the same words, from a cursor the last answer gave. */
export const onlineSearchTracks = (query: string, cursor: string, searchBox: SearchBox = 'music', typed = false) =>
  inApp()
    ? invoke<OnlineTrackPage>('online_search_tracks', { query, cursor, searchBox, typed }).catch(cancelled)
    : Promise.resolve<OnlineTrackPage>({ tracks: [], next_cursor: null })
/** A playlist or podcast in the online catalog: its name, owner line and cover. */
export interface OnlineCollection {
  provider: string
  /** The catalog page id (a playlist's `VL…`, a podcast's `MPSP…`). */
  collection_id: string
  title: string
  /** "YouTube Music • 50 songs", "T-Series • 550K views". */
  subtitle: string | null
  artwork_url: string | null
  kind: 'playlist' | 'podcast'
  /** What the card's own play button plays (a mix's endpoint, `params` and all), when it has one. */
  play?: RadioEndpoint | null
  /** Its card's "Not interested" (Home, signed in), in YouTube Music's words. */
  feedback?: MenuFeedback[]
}

/** One thing the catalog suggests while typing: a song to play, or something to open. */
export type OnlineSuggestion =
  | { kind: 'song'; track: OnlineTrack }
  | { kind: 'album'; album: OnlineAlbum; subtitle: string | null }
  | { kind: 'artist'; artist: OnlineArtist; subtitle: string | null }
  | { kind: 'playlist'; collection: OnlineCollection }

/** Completions of the typed words, and a few matching things, in the catalog's order. */
export interface OnlineSuggestions {
  queries: string[]
  items: OnlineSuggestion[]
  /** The ones of `queries` that are your own past searches (Signed-in search), with the token
   *  that takes each out of your YouTube Music search history. */
  past?: { text: string; remove_token?: string | null }[]
}

/** A mood or genre to browse, with the catalog's color for it. */
export interface MoodTile {
  title: string
  params: string
  color: string
}
export interface MoodSection {
  title: string
  tiles: MoodTile[]
}

/** A catalog playlist or podcast, opened: its name and its songs or episodes. */
export interface OnlineCollectionPage {
  title: string | null
  tracks: OnlineTrack[]
  /** Its page's Start radio, as the catalog put it there. */
  radio?: RadioEndpoint | null
}

const none = <T,>(empty: T) => Promise.resolve<T>(empty)

/** What the catalog suggests for half-typed words (empty offline, never an error). */
export const onlineSuggest = (input: string, searchBox: SearchBox = 'music') =>
  inApp() ? invoke<OnlineSuggestions>('online_suggest', { input, searchBox }).catch(cancelled) : none<OnlineSuggestions>({ queries: [], items: [] })
/** Playlists or podcasts for these words. */
export const onlineCollections = (query: string, kind: 'playlists' | 'podcasts', searchBox: SearchBox = 'music') =>
  inApp() ? invoke<OnlineCollection[]>('online_collections', { query, kind, searchBox }).catch(cancelled) : none<OnlineCollection[]>([])
/** Videos for these words (live sets, covers, uploads). */
export const onlineVideos = (query: string, searchBox: SearchBox = 'music') =>
  inApp() ? invoke<OnlineTrack[]>('online_videos', { query, searchBox }).catch(cancelled) : none<OnlineTrack[]>([])
/** The catalog's moods and genres (empty offline). */
export const onlineMoods = () => (inApp() ? invoke<MoodSection[]>('online_moods') : none<MoodSection[]>([]))
/** One mood's or genre's playlists. */
export const onlineMood = (params: string) => invoke<OnlineCollection[]>('online_mood', { params })
/** A radio of `seed` (a song id), or of the radio `playlistId`, for one with no button to replay. */
export const onlineRadio = (seed: string | null, playlistId: string | null = null) => invoke<OnlineRadio>('online_radio', { seed, playlistId })
/** The queue the catalog put on a button, a chip or a list's end, asked for as it came.
 *  `playing`: the song playing, left out when the answer opens with it. */
export const onlineRadioAt = (endpoint: RadioEndpoint, playing: string | null = null) => invoke<OnlineRadio>('online_radio_at', { endpoint, playing })
/** The next page of the same radio (`endpoint` and `continuation` from the page before). */
export const onlineRadioMore = (endpoint: RadioEndpoint, continuation: string) => invoke<OnlineRadio>('online_radio_more', { endpoint, continuation })
/** The catalog's Related songs for a song (`relatedId`: its Related page, when known). */
export const onlineRelated = (trackId: string, relatedId: string | null = null) => invoke<OnlineTrack[]>('online_related', { trackId, relatedId })
/** The whole of a song's Related page, shelf by shelf (what the Related tab shows). */
export const onlineRelatedPage = (trackId: string, relatedId: string | null = null) => invoke<OnlineRelatedShelf[]>('online_related_page', { trackId, relatedId })
/** The account's mixes, from Mixed for you's own page, in its rows. */
export const onlineMixedForYou = () => invoke<OnlineShelf[]>('online_mixed_for_you')
/** An online album's whole page. */
export const onlineAlbumPage = (albumId: string) => invoke<OnlineAlbumPage>('online_album_page', { albumId })
/** Everything in a row of an online page (its "See all"). */
export const onlineShelfMore = (more: OnlineMore) => invoke<OnlineSuggestion[]>('online_shelf_more', { more })
/** A catalog playlist's or podcast's songs. */
export const onlineCollection = (collectionId: string) => invoke<OnlineCollectionPage>('online_collection', { collectionId })

/** An online artist's page. */
export const onlineArtistPage = (artistId: string) => invoke<OnlineArtistPage>('online_artist', { artistId })
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

/** One line from a lyrics source, word by word when it times words. */
export interface FoundLine {
  ms: number
  text: string
  words?: { ms: number; end_ms: number; text: string }[]
  translation?: string | null
}
/** Lyrics a source gave. */
export interface FoundLyrics {
  source: string
  lines: FoundLine[]
  text: string
  instrumental: boolean
  credit: string | null
}
/** What you chose for one song's lyrics. */
export interface LyricsChoice {
  source?: string | null
  offset_ms: number
  title?: string | null
  artists?: string[] | null
}
/** The song lyrics are asked for: its key and what it is. */
export interface LyricsAsk {
  key: string
  video_id: string | null
  title: string
  artists: string[]
  album: string
  duration_ms: number
}
export interface LyricsAnswer {
  found: FoundLyrics | null
  label: string | null
  choice: LyricsChoice
}
export interface LyricsSourceStatus {
  source: string
  label: string
  status: 'synced' | 'plain' | 'none' | 'failed'
  on: boolean
}
/** A song's lyrics from the lyrics services: Automatic, or `source`. */
export const lyricsFind = (ask: LyricsAsk, source: string | null = null, fresh = false) => invoke<LyricsAnswer>('lyrics_find', { ask, source, fresh })
/** What every lyrics source has for a song, in your order. */
export const lyricsCheck = (ask: LyricsAsk) => invoke<LyricsSourceStatus[]>('lyrics_check', { ask })
/** Choose a song's lyrics source (null: Automatic); `rename` sets the name to ask by too. */
export const lyricsChoose = (key: string, source: string | null, rename?: { title: string; artists: string[] }) =>
  invoke<LyricsChoice>('lyrics_choose', { key, source, title: rename?.title ?? null, artists: rename?.artists ?? null, rename: !!rename })
/** How much later (positive) or earlier a song's lyrics run, in ms. */
export const lyricsOffset = (key: string, offsetMs: number) => invoke<LyricsChoice>('lyrics_offset', { key, offsetMs })

/** Who is signed in to YouTube Music. */
export interface YtmAccountInfo {
  name: string | null
  email: string | null
  handle: string | null
  picture: string | null
}
/** Open Google's sign-in for YouTube Music (it answers with the ytm-account-changed event). */
export const ytmSignIn = () => invoke<void>('ytm_sign_in')
export const ytmSignOut = () => invoke<void>('ytm_sign_out')
export const ytmAccount = () => invoke<YtmAccountInfo | null>('ytm_account')
/** Your YouTube Music playlists (none when signed out). */
export const ytmPlaylists = () => invoke<OnlineCollection[]>('ytm_playlists')
/** Rate an online song on the account; false when no one is signed in. */
export const ytmRate = (trackId: string, rating: 'like' | 'dislike' | 'none') => invoke<boolean>('ytm_rate', { trackId, rating })
/** Your YouTube Music likes into Venuta's Liked songs (only when you ask). */
export const ytmBringInLikes = (root: string) => invoke<{ name: string | null; found: number; added: number }>('ytm_bring_in_likes', { root })

/** The choices of the account and channel in use (null: signed out, or Local only). */
export interface YtmChoicesView {
  /** Which account and channel ("email#channel"). */
  account: string
  /** Asked already; false the first time (the choices are the safe ones). */
  asked: boolean
  choices: YtmAccountChoices
}
/** The account in use's choices, read again (and told to the catalog). */
export const ytmChoices = () => invoke<YtmChoicesView | null>('ytm_choices')
/** Keep an account's choices, all of them. */
export const ytmSetChoices = (account: string, choices: YtmAccountChoices) => invoke<YtmChoicesView>('ytm_set_choices', { account, choices })
/** With "New YouTube likes come in" on: the songs liked there since Venuta last looked join Liked songs. */
export const ytmNewLikesIn = (root: string) => invoke<{ name: string | null; added: number }>('ytm_new_likes_in', { root })

// --- The signed-in account: library, changes, history, uploads, identities, collaboration ----

/** Signed in, and whether YouTube still takes the sign-in (expired: "Sign in again"). */
export interface YtmStatus {
  signed_in: boolean
  expired: boolean
}
export type YtmShelf = 'playlists' | 'albums' | 'artists' | 'subscriptions' | 'podcasts' | 'upload_albums' | 'podcast_channels' | 'upload_artists'
/** A song as the account's lists hold it: the song, and its place in the list. */
export interface YtmTrack extends OnlineTrack {
  set_video_id?: string | null
  /** The account uploaded it: only it can play the song. */
  upload?: boolean
  /** Who put it in the playlist, on a collaborative one. */
  added_by?: YtmContributor | null
  /** Its votes, on a collaborative playlist that has them. */
  votes?: YtmVotes | null
  /** One of your uploads: the id that deletes it on YouTube Music. */
  delete_id?: string | null
}
/** The person who added a song to a collaborative playlist, as its row shows them. */
export interface YtmContributor {
  name: string | null
  picture: string | null
  channel_id: string | null
}
/** A song's votes in a collaborative playlist, and the token each button sends (lib/ytmVotes.ts). */
export interface YtmVotes {
  /** How you voted when the list came. */
  mine: Vote
  /** The net count when the list came. */
  votes: number
  /** YouTube Music's words for the count with no vote of yours, with yours up, with yours down. */
  count: string | null
  count_up: string | null
  count_down: string | null
  up: YtmVoteButton
  down: YtmVoteButton
}
/** One vote button: its vote, and what it sends pressed again to take the vote back. */
export interface YtmVoteButton {
  token: string
  undo_token: string | null
}
export interface YtmPlaylist {
  playlist_id: string
  title: string
  description: string | null
  /** PRIVATE, UNLISTED or PUBLIC. */
  privacy: string | null
  /** The account made it, so it can change it. */
  owned: boolean
  collaborative: boolean
  /** A collaborative playlist's people (their pictures), and the words beside them. */
  people: string[]
  people_text: string | null
  artwork_url: string | null
  tracks: YtmTrack[]
  /** Every page came (a sync never reads a missing song as taken out when false). */
  complete: boolean
  /** Its page's Start radio, as YouTube Music put it there. */
  radio?: RadioEndpoint | null
}
export type YtmEdit =
  | { kind: 'rename'; title: string }
  | { kind: 'describe'; description: string }
  | { kind: 'privacy'; privacy: string }
  | { kind: 'add'; video_ids: string[] }
  | { kind: 'remove'; items: [string, string][] }
  | { kind: 'move'; set_video_id: string; before: string | null }
  /** A whole playlist (or an album's list) copied in, on YouTube Music's side. */
  | { kind: 'add_playlist'; playlist_id: string }
  /** New songs go to the top (`on`) or to the end. */
  | { kind: 'add_to_top'; on: boolean }
export interface YtmSaved {
  playlist_id: string
  saved: boolean
}
export interface YtmFollow {
  channel_id: string
  subscribed: boolean
}
export interface YtmHistoryDay {
  title: string
  tracks: YtmHistoryPlay[]
}
/** One play in the history, and the token that takes it out (absent: YouTube Music sent none). */
export interface YtmHistoryPlay extends OnlineTrack {
  remove_token?: string | null
}
export interface YtmIdentity {
  id: string
  name: string
  handle: string | null
  email: string | null
  picture: string | null
  current: boolean
  /** The sign-in's own channel (not a brand account): used by choosing no identity. */
  own: boolean
}
export const ytmStatus = () => invoke<YtmStatus>('ytm_status')
export const ytmLibrary = (shelf: YtmShelf) => invoke<OnlineSuggestion[]>('ytm_library', { shelf })
export const ytmLibrarySongs = (uploads: boolean) => invoke<YtmTrack[]>('ytm_library_songs', { uploads })
export const ytmPlaylist = (playlistId: string) => invoke<YtmPlaylist>('ytm_playlist', { playlistId })
export const ytmPlaylistCreate = (title: string, privacy: string, videoIds: string[], description?: string) =>
  invoke<string>('ytm_playlist_create', { title, description: description ?? null, privacy, videoIds })
/** Change a playlist the account owns; where added songs landed, [videoId, setVideoId]. */
export const ytmPlaylistEdit = (playlistId: string, edits: YtmEdit[]) => invoke<[string, string][]>('ytm_playlist_edit', { playlistId, edits })
export const ytmPlaylistDelete = (playlistId: string) => invoke<void>('ytm_playlist_delete', { playlistId })
export const ytmAlbumSaved = (albumId: string) => invoke<YtmSaved | null>('ytm_album_saved', { albumId })
export const ytmSave = (playlistId: string, saved: boolean) => invoke<void>('ytm_save', { playlistId, saved })
export const ytmArtistFollowed = (artistId: string) => invoke<YtmFollow | null>('ytm_artist_followed', { artistId })
export const ytmSubscribe = (channelId: string, subscribed: boolean) => invoke<void>('ytm_subscribe', { channelId, subscribed })
export const ytmHistory = () => invoke<YtmHistoryDay[]>('ytm_history')
/** Take one play out of the account's history, by its token. */
export const ytmHistoryRemove = (token: string) => invoke<void>('ytm_history_remove', { token })
/** Vote on a song of a collaborative playlist, or take the vote back: the token its button sends. */
export const ytmVote = (token: string) => invoke<void>('ytm_vote', { token })
/** One artist YouTube Music's taste builder offers, and the two values its answer is made of. */
export interface TasteArtist {
  name: string
  selection_value: string
  impression_value: string
  picture: string | null
  selected: boolean
}
/** One online song's own menu as the account sees it, for one listed without (by its id alone). */
export const ytmSongMenu = (trackId: string) => invoke<TrackMenu | null>('ytm_song_menu', { trackId })
export const ytmTasteProfile = () => invoke<TasteArtist[]>('ytm_taste_profile')
/** Tell YouTube Music the artists chosen: every one shown, and the chosen ones. */
export const ytmSetTasteProfile = (impressions: string[], selected: string[]) => invoke<void>('ytm_set_taste_profile', { impressions, selected })
/** One press on an online song's own menu: the token its menu holds for that way. */
export const ytmMenuPress = (token: string, press: Press) => invoke<void>('ytm_menu_press', { token, press })
/** Take one past search out of your YouTube Music search history, by its token. */
export const ytmRemovePastSearch = (token: string) => invoke<void>('ytm_remove_past_search', { token })
/** Delete one of your uploads on YouTube Music: a song by its `delete_id`, an album by its page id. */
export const ytmDeleteUpload = (entityId: string) => invoke<void>('ytm_delete_upload', { entityId })
/** Give a playlist of yours this picture (a JPEG or PNG file) as its cover on YouTube Music. */
export const ytmPlaylistCover = (playlistId: string, path: string) => invoke<void>('ytm_playlist_cover', { playlistId, path })
/** Take a playlist's own cover away; the one YouTube Music makes from its songs, when it said. */
export const ytmPlaylistCoverClear = (playlistId: string) => invoke<string | null>('ytm_playlist_cover_clear', { playlistId })
export const ytmUpload = (path: string) => invoke<void>('ytm_upload', { path })
export const ytmIdentities = () => invoke<YtmIdentity[]>('ytm_identities')
export const ytmUseIdentity = (id: string | null) => invoke<void>('ytm_use_identity', { id })
export const ytmInvite = (playlistId: string) => invoke<string>('ytm_invite', { playlistId })
export const ytmCloseToCollaborators = (playlistId: string, closed: boolean) => invoke<void>('ytm_close_to_collaborators', { playlistId, closed })
export const ytmJoin = (link: string) => invoke<string>('ytm_join', { link })

/** A Google sign-in kept on this computer, to switch to (never its cookies). */
export interface YtmSavedAccount {
  id: string
  name: string | null
  email: string | null
  picture: string | null
  current: boolean
}
export const ytmSavedAccounts = () => invoke<YtmSavedAccount[]>('ytm_saved_accounts')
export const ytmAddAccount = () => invoke<void>('ytm_add_account')
export const ytmSwitchAccount = (id: string) => invoke<void>('ytm_switch_account', { id })
export const ytmForgetAccount = (id: string) => invoke<void>('ytm_forget_account', { id })

/** What one sync of a linked playlist did. */
export interface YtmSyncNote {
  at: number
  pulled: number
  dropped: number
  pushed: number
  removed: number
  moved: number
  /** Yours as it was before, when this sync changed it (for Undo). */
  undo?: string
  problem?: string
}
/** A playlist of yours linked to one on YouTube Music ("Also on YouTube Music"). */
export interface YtmLink {
  playlist: string
  account: string
  playlist_id: string
  title: string
  base: string[]
  matches: Record<string, string>
  local_only: string[]
  last_sync: number
  held?: boolean
  uploading?: Record<string, { title: string; artist: string; at: number }>
  history?: YtmSyncNote[]
}
export interface YtmLinkView extends YtmLink {
  /** Files of yours not asked about yet. */
  unmatched: number
  /** Songs that are (or go) there, and lines that stay here only. */
  there: number
  only_here: number
}
export type YtmPause =
  | { kind: 'unread' }
  | { kind: 'held' }
  | { kind: 'other_account'; account: string }
  | { kind: 'many_gone'; side: 'venuta' | 'youtube_music'; gone: number; of: number }
export interface YtmSyncReport {
  paused: YtmPause | null
  busy: boolean
  note: YtmSyncNote
  uploaded: number
  upload_problems: string[]
}
/** A file of yours the matching review asks about. */
export interface YtmReviewRow {
  key: string
  path: string
  title: string
  artist: string
  album: string
  duration_ms: number
  found: OnlineEntry | null
  uploadable: boolean
  /** What you said before (Check matches). */
  current: OnlineEntry | null
  current_id: string | null
  kept_here: boolean
  uploading: boolean
}
export interface YtmChoices {
  matches?: [string, string][]
  local_only?: string[]
  upload?: string[]
}
export const ytmLink = (root: string, name: string) => invoke<YtmLinkView | null>('ytm_link', { root, name })
export const ytmLinks = (root: string) => invoke<YtmLink[]>('ytm_links', { root })
export const ytmLinkReview = (root: string, name: string, all = false) => invoke<YtmReviewRow[]>('ytm_link_review', { root, name, all })
export const ytmLinkStart = (root: string, name: string, target: string | null, privacy: string, choices: YtmChoices) =>
  invoke<YtmSyncReport>('ytm_link_start', { root, name, target, privacy, choices })
export const ytmLinkChoose = (root: string, name: string, choices: YtmChoices) => invoke<YtmSyncReport>('ytm_link_choose', { root, name, choices })
/** Sync now. `force` answers the pause shown: "Sync again" after an Undo, or "Sync anyway" for the side most songs went from (`gone`). */
export const ytmLinkSync = (root: string, name: string, force = false, gone: 'venuta' | 'youtube_music' | null = null) =>
  invoke<YtmSyncReport>('ytm_link_sync', { root, name, force, gone })
export const ytmUnlink = (name: string) => invoke<void>('ytm_unlink', { name })
export const ytmLinkUndo = (root: string, name: string, copy: string) => invoke<void>('ytm_link_undo', { root, name, copy })

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
/** Read playlists to bring in: files (any format, folders, zips), a share link, pasted text (links,
 *  songs copied from Spotify's app, a list of songs, or the Copy for Venuta bookmark's list), or the
 *  Apple Music library. `onProgress` hears the slow part (naming songs that came as ids). */
export const importPreview = (
  root: string,
  files: string[],
  link: string | null,
  apple: boolean,
  text: string | null = null,
  onProgress: (d: number, t: number, note?: string) => void = () => {},
) => withProgress<ImportRead>('import_preview', { root, files, link, apple, text }, onProgress)
/** The online catalog's song for each of these, or null where it isn't sure: so a playlist's
 *  songs you don't have play online. */
export const playlistFindOnline = (songs: ForeignSong[], onProgress: (d: number, t: number) => void = () => {}) =>
  withProgress<(OnlineEntry | null)[]>('playlist_find_online', { songs }, onProgress)
/** Every playlist that came from somewhere readable, read again: the ones that changed there. */
export const playlistsCheck = (root: string, onProgress: (d: number, t: number, note?: string) => void) =>
  withProgress<ImportRead>('playlists_check', { root }, onProgress)

// --- Playlists out ---------------------------------------------------------------------------

export type ExportFormat = 'm3u8' | 'm3u' | 'pls' | 'xspf' | 'jspf' | 'wpl' | 'csv' | 'txt' | 'itunes_xml'
export type PathStyle = { kind: 'this_pc' } | { kind: 'next_to_playlist' } | { kind: 'in_music_folder' } | { kind: 'starts_with'; prefix: string }
export interface ExportOptions {
  format: ExportFormat
  style: PathStyle
  include_missing: boolean
}
export type SendService = 'spotify' | 'apple_music' | 'deezer'
export interface OnServiceOut {
  total: number
  found: number
  missing: number[]
  /** Spotify: one link a line, to paste into a playlist in Spotify's app. */
  paste: string
  /** A page listing every song with its link there (saved; open it in the browser). */
  page: string
}
export interface YouTubeLinks {
  links: string[]
  found: number
  total: number
}
/** Export playlists: one to a file, several into a folder. Returns the files written. */
export const playlistsExport = (root: string, names: string[], to: string, options: ExportOptions) =>
  invoke<string[]>('playlists_export', { root, names, to, options })
/** A playlist as text in a format (a plain list of songs to copy, or any other). */
export const playlistText = (root: string, name: string, format: ExportFormat) => invoke<string>('playlist_text', { root, name, format })
/** Every playlist, To get and the smart playlists' rules in one zip. */
export const playlistsBackup = (root: string, to: string) => invoke<number>('playlists_backup', { root, to })
/** A playlist's songs found on a service, free (Apple Music looks in this computer's region's store). */
export const playlistOnService = (
  root: string,
  name: string,
  service: SendService,
  onProgress: (d: number, t: number, note?: string) => void,
) => withProgress<OnServiceOut>('playlist_on_service', { root, name, service, country: regionCode() }, onProgress)
/** A playlist as YouTube links of up to 50 songs each. */
export const playlistYoutubeLinks = (root: string, name: string, onProgress: (d: number, t: number, note?: string) => void) =>
  withProgress<YouTubeLinks>('playlist_youtube_links', { root, name }, onProgress)
/** This computer's region, as two letters ("IN"), from its language settings; null when unknown. */
export function regionCode(): string | null {
  try {
    const loc = new Intl.Locale(navigator.language || Intl.DateTimeFormat().resolvedOptions().locale)
    return loc.maximize().region ?? null
  } catch {
    return null
  }
}
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
/** Open a link Import & export made, in your browser (music services and transfer tools only). */
export const openSendLink = (url: string) => invoke<void>('open_send_link', { url })
/** Open a page of links Import & export saved, in your browser. */
export const openSavedPage = (path: string) => invoke<void>('open_saved_page', { path })
