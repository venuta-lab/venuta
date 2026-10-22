// Development only (lyrics-preview.html, served by `vite`, never part of a build): every lyrics
// view with a fixture song (tests/fixtures/lyrics-second), in a plain browser, for screenshots
// (scripts/lyrics-shots.mjs). Nothing here reaches the engine: the player state, the lyrics, the
// translation models and the Japanese dictionary's progress are stand-ins.
//
//   /lyrics-preview.html?view=stage&song=hindi&theme=dark&rows=all&size=xl&line=2
//
//   view   nowplaying | stage | overlay | mini | settings
//   song   hindi | japanese | spanish | mixed | plain | unbroken (lines with nowhere to wrap)
//   theme  dark | light
//   style  a theme (venuta, thermoptic, skin99...): its fonts, for the Theme look
//   rows   all | none | roman | translation   (the two switches, for every language)
//   size   s | m | l | xl                     (the big screen's and desktop lyrics' size)
//   look   karaoke | clean | glow | outline | serif | theme
//   line   the line being sung (0 based); `at` 0 to 1 how far through it
//   play   1: playing (the lyrics clock runs)
//   dict   downloading: the Japanese dictionary on its way

const q = new URLSearchParams(location.search)
const view = q.get('view') ?? 'stage'
const song = q.get('song') ?? 'hindi'
const theme = q.get('theme') === 'light' ? 'light' : 'dark'
const rows = q.get('rows') ?? 'all'
const size = q.get('size') ?? 'm'
const look = q.get('look') ?? 'karaoke'
const lineAt = Number(q.get('line') ?? 2)
const through = Number(q.get('at') ?? 0.45)

// What every module reads as it loads, so it is set first.
const put = (k: string, v: unknown) => localStorage.setItem(k, JSON.stringify(v))
localStorage.clear()
put('appearance', q.get('style') ? { theme, style: q.get('style') } : { theme })
put('lyric-second-lines', { defaults: { roman: rows === 'all' || rows === 'roman', translation: rows === 'all' || rows === 'translation' }, byLang: {}, into: 'en' })
for (const w of ['stage', 'overlay']) put(`lyric-look-${w}`, { look, size, colour: 'accent', lines: 2 })
put('mini-player', { lyrics: true, onTop: true })
if (['mini', 'stage', 'overlay'].includes(view)) document.documentElement.dataset.window = view

await import('@fontsource-variable/sora')
await import('@fontsource-variable/figtree')
await import('@fontsource-variable/inter')
await import('@fontsource/shippori-mincho-b1/800')
await import('../index.css')
const [{ createRoot }, { createElement }, prefs, motion, lyrics, model, translation, dictionary] = await Promise.all([
  import('react-dom/client'),
  import('react'),
  import('../lib/prefs'),
  import('../lib/motion'),
  import('../lib/lyrics'),
  import('../lib/lyricsModel'),
  import('../lib/translation'),
  import('../lib/japaneseDictionary'),
])
motion.watchAwake()
prefs.applyPrefs()

// The song and its words.
const raw: unknown = await (await fetch(`/tests/fixtures/lyrics-second/${song}.json`)).json()
const m = model.readModel(raw)
const words: Parameters<typeof lyrics.setPreviewLyrics>[0] = () => ({
  lines: m.lines.map((l) => [l.start_ms, l.text] as [number, string]),
  text: '',
  lang: m.lang,
  model: m,
})
const fixed = words(null as never)
lyrics.setPreviewLyrics(() => fixed)
const TITLES: Record<string, [string, string]> = {
  hindi: ['Chaand Phir Se', 'Preview Artist'],
  japanese: ['Yozora', 'Preview Artist'],
  spanish: ['Bajo la luna', 'Preview Artist'],
  mixed: ['Dil Ye Mera', 'Preview Artist'],
  plain: ['Morning Comes', 'Preview Artist'],
  unbroken: ['Nowhere to Wrap', 'Preview Artist'],
}
const [title, artist] = TITLES[song] ?? ['Preview', 'Preview Artist']
const line = m.lines[Math.min(lineAt, m.lines.length - 1)]
const position = line ? line.start_ms - 250 + (line.end_ms - line.start_ms) * through : 0
const item = {
  path: `preview/${song}.flac`,
  dir: 'preview',
  title,
  artists: [artist],
  album: 'Fixtures',
  album_artist: artist,
  duration_ms: 25000,
  format: 'FLAC',
  bit_depth: 16,
  sample_rate: 44100,
  lyrics: true,
  stars: 0,
  rg: { track_db: null, album_db: null },
}
;(window as unknown as { __venutaSetPlayer: (s: object) => void }).__venutaSetPlayer({
  queue: [item],
  index: 0,
  position_ms: position,
  duration_ms: 25000,
  playing: q.get('play') === '1',
  root: '',
})
// Playing: the player's reports, every 100 ms, as the engine sends them.
if (q.get('play') === '1') {
  let at = position
  window.setInterval(() => {
    at += 100
    ;(window as unknown as { __venutaSetPlayer: (s: object) => void }).__venutaSetPlayer({ position_ms: at % 25000 })
  }, 100)
}

// Translation models on this PC (stand-ins for the translation package's).
const MODELS = [
  { id: 'hi-en', from: 'hi', to: 'en', name: 'Hindi to English', size_bytes: 47_185_920, installed: true, progress: null },
  { id: 'ja-en', from: 'ja', to: 'en', name: 'Japanese to English', size_bytes: 61_865_984, installed: false, progress: null },
  { id: 'es-en', from: 'es', to: 'en', name: 'Spanish to English', size_bytes: 20_971_520, installed: false, progress: 0.4 },
]
translation.setTranslationBackend({
  models: () => Promise.resolve(MODELS),
  download: () => Promise.resolve(),
  remove: () => Promise.resolve(),
  translate: () => new Promise(() => {}),
})
if (q.get('dict') === 'downloading') dictionary.setDictionaryStatus({ state: 'downloading', done: 40, total: 100 })

// The view.
let shown: React.ReactNode
if (view === 'stage') shown = createElement((await import('../components/lyrics/BigScreen')).BigScreen)
else if (view === 'overlay') {
  // A desktop behind the strip.
  document.documentElement.style.background = theme === 'light' ? 'linear-gradient(135deg,#dfe6ee,#b8c4d2)' : 'linear-gradient(135deg,#1d2b3a,#3a2d4a)'
  shown = createElement((await import('../components/lyrics/DesktopLyrics')).DesktopLyrics)
} else if (view === 'mini') shown = createElement((await import('../components/player/MiniPlayer')).MiniPlayer)
else if (view === 'settings') {
  const { LyricLinesSettings } = await import('../screens/settings/LyricLinesSettings')
  shown = createElement('div', { className: 'thin-scroll h-full overflow-y-auto p-8 text-ink' }, createElement('h2', { className: 'title text-[22px]' }, 'Lyrics'), createElement(LyricLinesSettings))
} else {
  const { NowPlaying } = await import('../components/player/NowPlaying')
  const none = () => {}
  shown = createElement(
    'div',
    { className: 'relative h-full w-full text-ink' },
    createElement(NowPlaying, { open: true, root: '', panel: 'lyrics', onPanel: none, onClose: none, onAlbum: none, onArtist: none, onOnlineArtist: none }),
  )
}
createRoot(document.getElementById('root')!).render(shown)

// Ready once the type has loaded and two frames have been drawn.
await document.fonts.ready
await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)))
;(window as unknown as { __previewReady: boolean }).__previewReady = true
