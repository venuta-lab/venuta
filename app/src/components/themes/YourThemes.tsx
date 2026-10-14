import { useEffect, useState } from 'react'
import { open as openDialog, save as saveDialog } from '@tauri-apps/plugin-dialog'
import { inApp } from '../../lib/engine'
import { notify } from '../../lib/notify'
import {
  addBundled,
  bundledPacks,
  duplicateTheme,
  exportTheme,
  freeId,
  loadPacks,
  packFailure,
  setThemeAside,
  showThemeFile,
  usePacks,
  type Bundled,
  type Installed,
} from '../../lib/packs'
import { fontStack, FONT_SLOTS, packInfo, tokensIn, type Pack } from '../../lib/packFormat'
import { getPrefs, resolvedTheme, setPrefs, usePrefs } from '../../lib/prefs'
import { styleInfo } from '../../lib/themes'
import { confirmAsk } from '../Confirm'
import { useContextMenu } from '../../screens/listen/parts'
import { CardWith, ThemeCard } from './ThemeCard'
import { openStudio } from './ThemeStudio'
import { openImport } from './ThemeImport'
import { copyThemePrompt } from './themePrompt'

/** Put a theme of yours on screen (and keep the choice). */
export const chooseTheme = (pack: Pack) => setPrefs({ style: pack.extends, pack: pack.id })

/** A pack's values for its card: in the mode it shows in now (or the one it has). */
function cardTokens(pack: Pack): Record<string, string> {
  const now = resolvedTheme()
  const mode = pack.modes.includes(now) ? now : pack.modes[0]
  const t = tokensIn(pack, mode)
  for (const slot of FONT_SLOTS) {
    const s = fontStack(pack, slot)
    if (s) t[`font-${slot}`] = s
  }
  return t
}

/** Your themes and the ones that come with Venuta, in Settings (Look and feel). */
export function YourThemes() {
  const p = usePrefs()
  const packs = usePacks()
  const [more, setMore] = useState<Bundled[]>([])
  useEffect(() => {
    void bundledPacks().then(setMore)
  }, [])
  const failure = p.pack ? packFailure() : ''
  const notYours = more.filter((b) => !packs.some((t) => t.id === b.id))

  async function importFile() {
    const path = await openDialog({ multiple: false, filters: [{ name: 'Venuta theme', extensions: ['venuta-theme', 'zip'] }] })
    if (typeof path === 'string') openImport(path)
  }

  return (
    <section aria-labelledby="your-themes" className="mt-8">
      <div className="flex flex-wrap items-end justify-between gap-3">
        <div className="min-w-0">
          <h3 id="your-themes" className="title text-[17px]">
            Your themes
          </h3>
          <p className="mt-1 max-w-[52ch] text-[12.5px] leading-relaxed text-ink/55">
            Themes you make or are given, kept with your library. Each is built on one of the themes above and keeps what it does.
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          <button className="btn" onClick={() => openStudio({ create: true })} disabled={!inApp()}>
            New theme
          </button>
          <button className="btn" onClick={() => void importFile()} disabled={!inApp()}>
            Import…
          </button>
        </div>
      </div>

      {failure && (
        <div role="alert" className="glass mt-4 flex flex-wrap items-center gap-3 rounded-xl px-4 py-3 text-[12.5px] leading-relaxed">
          <span className="min-w-0 flex-1">
            Your theme couldn't load: {failure} {styleInfo(p.style).name}, the theme it's built on, is showing instead.
          </span>
          <button className="btn" onClick={() => setPrefs({ pack: '' })}>
            Use {styleInfo(p.style).name}
          </button>
        </div>
      )}

      {packs.length > 0 ? (
        <div role="radiogroup" aria-label="Your themes" className="mt-4 grid grid-cols-[repeat(auto-fill,minmax(200px,1fr))] gap-3">
          {packs.map((t) => (
            <PackCard key={t.id} t={t} on={p.pack === t.id} />
          ))}
        </div>
      ) : (
        <p className="mt-4 rounded-xl border border-dashed border-[var(--edge-strong)] px-4 py-5 text-[12.5px] leading-relaxed text-ink/60">
          None yet. <b className="font-medium text-ink/80">New theme</b> starts one from the theme in use, the album playing, or a few colours, and the
          whole app shows it as you choose. Or add one of the themes below and change it.
        </p>
      )}

      {notYours.length > 0 && (
        <>
          <h3 className="title mt-8 text-[17px]">More themes</h3>
          <p className="mt-1 max-w-[52ch] text-[12.5px] leading-relaxed text-ink/55">Made as theme files, like yours: choose one to add it to your themes, then use it as it is or change it.</p>
          <div role="radiogroup" aria-label="More themes" className="mt-4 grid grid-cols-[repeat(auto-fill,minmax(200px,1fr))] gap-3">
            {notYours.map((b) => (
              <ThemeCard
                key={b.id}
                name={b.pack.name}
                preview={b.pack.extends}
                tokens={cardTokens(b.pack)}
                albumLight={packInfo(b.pack).accent === 'yours'}
                on={false}
                onPick={() =>
                  void addBundled(b)
                    .then(() => {
                      chooseTheme(b.pack)
                      notify(`Added ${b.pack.name} to your themes.`)
                    })
                    .catch((e) => notify(`Couldn't add ${b.pack.name}: ${String(e)}`))
                }
              />
            ))}
          </div>
        </>
      )}

      <p className="mt-6 text-[12px] leading-relaxed text-ink/50">
        Making themes by hand or with an AI assistant: every value is described in Venuta's THEMES guide.{' '}
        <button className="text-accent-ink underline-offset-2 hover:underline" onClick={() => void copyThemePrompt()}>
          Copy a prompt for an AI assistant
        </button>
      </p>
    </section>
  )
}

function PackCard({ t, on }: { t: Installed; on: boolean }) {
  const pack = t.pack
  const [notes, setNotes] = useState(false)
  const actions = (): [string, () => void][] => [
    ...(pack ? ([['Use', () => chooseTheme(pack)], ['Edit in Theme Studio', () => openStudio({ id: t.id })]] as [string, () => void][]) : []),
    ['Duplicate', () => void duplicate(t)],
    ...(pack ? ([['Export…', () => void exportOne(t.id, pack.name)]] as [string, () => void][]) : []),
    ['Show the file', () => void showThemeFile(t.id).catch((e) => notify(String(e)))],
    ['Remove…', () => void remove(t)],
  ]
  const { onContextMenu, menu } = useContextMenu(actions)
  const problems = t.problems.filter((x) => !x.fatal)
  const name = pack?.name ?? t.id
  return (
    <CardWith
      card={
        pack ? (
          <ThemeCard
            name={pack.name}
            preview={pack.extends}
            tokens={cardTokens(pack)}
            albumLight={packInfo(pack).accent === 'yours'}
            on={on}
            onPick={() => chooseTheme(pack)}
            onContextMenu={onContextMenu}
          />
        ) : (
          <div onContextMenu={onContextMenu} className="flex aspect-[16/10] flex-col justify-end rounded-xl border border-dashed border-[var(--edge-strong)] p-3 text-[12px] leading-snug text-ink/65">
            <span className="text-problem">Can't be used</span>
            {t.problems[0]?.message}
          </div>
        )
      }
    >
      <button
        aria-label={`More for ${name}`}
        title="More"
        onClick={onContextMenu}
        className="glass absolute right-1.5 top-1.5 flex h-7 w-7 items-center justify-center rounded-full text-[15px] leading-none text-ink/75 opacity-0 shadow-[inset_0_0_0_1px_var(--edge)] transition-opacity hover:text-ink focus-visible:opacity-100 [div:hover>&]:opacity-100"
      >
        …
      </button>
      {!pack && <div className="mt-2 px-0.5 text-[13.5px] font-medium text-ink/80">{t.id}</div>}
      {problems.length > 0 && (
        <div className="mt-1 px-0.5">
          <button className="text-[11.5px] text-fix" onClick={() => setNotes((v) => !v)} aria-expanded={notes}>
            {problems.length === 1 ? '1 note' : `${problems.length} notes`}
          </button>
          {notes && (
            <ul className="mt-1 space-y-1 text-[11.5px] leading-snug text-ink/60">
              {problems.map((x, i) => (
                <li key={i}>
                  <code className="text-ink/75">{x.at}</code> {x.message}
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
      {menu}
    </CardWith>
  )
}

async function duplicate(t: Installed) {
  const to = freeId(`${t.id}-copy`)
  try {
    await duplicateTheme(t.id, to)
    await loadPacks()
    notify(`Made a copy: ${to}.`, { label: 'Edit', run: () => openStudio({ id: to }) })
  } catch (e) {
    notify(`Couldn't copy the theme: ${String(e)}`)
  }
}

async function exportOne(id: string, name: string) {
  const dest = await saveDialog({ defaultPath: `${name}.venuta-theme`, filters: [{ name: 'Venuta theme', extensions: ['venuta-theme'] }] })
  if (!dest) return
  try {
    await exportTheme(id, dest)
    notify(`Saved ${name} as a theme file you can share.`)
  } catch (e) {
    notify(`Couldn't save the theme file: ${String(e)}`)
  }
}

async function remove(t: Installed) {
  const name = t.pack?.name ?? t.id
  const ok = await confirmAsk({
    title: `Remove ${name}?`,
    text: 'It moves to Set aside in your library, with everything it holds, so nothing is lost.',
    confirm: 'Remove',
    danger: true,
  })
  if (!ok) return
  try {
    if (getPrefs().pack === t.id) setPrefs({ pack: '' })
    await setThemeAside(t.id)
    await loadPacks()
    notify(`Moved ${name} to Set aside.`)
  } catch (e) {
    notify(`Couldn't remove the theme: ${String(e)}`)
  }
}
