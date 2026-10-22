import { createRoot } from 'react-dom/client'
import '@fontsource-variable/sora'
import '../../../index.css'
import type { Lyrics } from '../../../lib/lyrics'
import { setAccentVars } from '../../../lib/prefs'
import { accentRoles, ACCENTS } from '../../../lib/themes'
import type { TogetherViewV2 } from '../../../lib/togetherView'
import type { InvitePreview, RoomActions } from '../model'
import { RoomChipV2 } from '../RoomChipV2'

/*
  The room panel v2 for one state of the window's fixture (tests/fixtures/together-views.json),
  in the app's own CSS and Venuta's dark or light theme, open under a stand-in title bar: what
  shoot.mjs photographs in headless Chrome. Dev server only; no app calls (every action does
  nothing). ?case=<the case's name>&theme=dark|light, and &show=end|hand (the host's questions, scrolled
  to) or &open=<id> (a person's numbers); without a case, a list of them.
*/

const params = new URLSearchParams(location.search)
const theme = params.get('theme') === 'light' ? 'light' : 'dark'
const el = document.documentElement
el.dataset.style = 'venuta'
el.dataset.theme = theme
el.dataset.surface = 'solid'
el.dataset.accent = 'yours'
el.dataset.backdrop = 'off'
el.dataset.motion = 'reduced'
document.body.style.background = 'var(--solid)'
// The default accent (Iris), as Lumen shows it in this theme.
setAccentVars(accentRoles(ACCENTS[0].seed, theme === 'light'))

// Words for the remote's lyrics (made up for the preview; songs in the fixture have none).
const LYRICS: Lyrics = {
  lines: [
    [100_000, 'The first line of the song'],
    [106_000, 'And the second one after it'],
    [112_000, 'A third line, a little later'],
  ],
  text: '',
}

const preview: InvitePreview = { host: 'Asha', open: true, until: 0, listening: 3, now_playing: 'Song 1' }
const act = new Proxy({} as RoomActions, {
  get: (_, key) => (key === 'readInvite' ? () => Promise.resolve(preview) : () => {}),
})

/** What starts open: the end question, the hand-over list, or a person's numbers. */
const show = params.get('show') as 'end' | 'hand' | null
const openRow = params.get('open')
const START = { ask: show === 'end' || show === 'hand' ? show : undefined, open: openRow ? Number(openRow) : undefined }

/** The stand-in window: a title bar with the chip, its panel open. */
const windowWith = (v: TogetherViewV2) => (
  <div className="min-h-screen text-ink">
    <div className="flex h-11 items-center gap-3 border-b border-edge px-4">
      <span className="text-[13px] font-semibold text-ink/70">Venuta</span>
      <span className="flex-1" />
      <RoomChipV2 v={v} act={act} env={{ since: () => 0, lyrics: LYRICS, start: START }} startOpen />
    </div>
  </div>
)

async function main() {
  const { cases } = (await fetch('/tests/fixtures/together-views.json').then((r) => r.json())) as { cases: { name: string; view: TogetherViewV2 }[] }
  const root = createRoot(document.getElementById('root')!)
  const name = params.get('case')
  const found = cases.find((c) => c.name === name)
  if (found) {
    root.render(windowWith(found.view))
    // Drawn, with its fonts: shoot.mjs waits for this.
    await document.fonts.ready
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        // The host's questions are at the panel's end.
        const panel = document.querySelector('[role="dialog"] > div')
        if (START.ask && panel) panel.scrollTop = panel.scrollHeight
        requestAnimationFrame(() => (document.body.dataset.ready = '1'))
      }),
    )
    return
  }
  root.render(
    <ul className="p-6 text-[13px] text-ink">
      {cases.map((c) => (
        <li key={c.name}>
          <a className="underline" href={`?case=${encodeURIComponent(c.name)}&theme=${theme}`}>
            {c.name}
          </a>
        </li>
      ))}
    </ul>,
  )
}
void main()
