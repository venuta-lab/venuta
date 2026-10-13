import { AnimatePresence, motion } from 'motion/react'
import { useState } from 'react'
import { save as saveDialog } from '@tauri-apps/plugin-dialog'
import { wantedExport, wantRemove, type WantedAlbum } from '../../lib/engine'
import { notify } from '../../lib/notify'
import { Detail, Segmented } from '../../components/ui'
import { plural } from './common'

const TIERS = {
  1: { label: 'Get the whole album', line: 'Several songs from these are in your playlists: the album is worth having.' },
  2: { label: 'Worth a listen', line: 'A song or two you saved; the rest of the album may be good too.' },
  3: { label: 'Just the song', line: 'One song from a compilation or a single. Probably skip the album.' },
} as const

/** Albums your playlists want and your library doesn't have, best first. It never downloads anything. */
export function WantList(props: { root: string; wanted: WantedAlbum[]; onBack: () => void; onOpen: (playlist: string) => void; onChanged: () => void }) {
  const { wanted } = props
  const counts = { 1: 0, 2: 0, 3: 0 } as Record<1 | 2 | 3, number>
  for (const w of wanted) counts[w.tier]++
  const first = ([1, 2, 3] as const).find((t) => counts[t] > 0) ?? 1
  const [tier, setTier] = useState<'1' | '2' | '3'>(String(first) as '1')
  const shown = wanted.filter((w) => String(w.tier) === tier)

  async function exportFile() {
    const to = await saveDialog({ defaultPath: 'To get.csv', filters: [{ name: 'Spreadsheet', extensions: ['csv'] }] })
    if (!to) return
    try {
      const n = await wantedExport(props.root, to)
      notify(`Saved ${plural(n, 'album')} to ${to}.`)
    } catch (e) {
      notify(`Couldn't save the list: ${e instanceof Error ? e.message : String(e)}`)
    }
  }

  async function remove(w: WantedAlbum) {
    try {
      await wantRemove(w.album, w.artist)
      props.onChanged()
      notify(`Took “${w.album}” off the list.`)
    } catch (e) {
      notify(`Couldn't change the list: ${e instanceof Error ? e.message : String(e)}`)
    }
  }

  async function copy(w: WantedAlbum) {
    try {
      await navigator.clipboard.writeText(`${w.album} ${w.artist}`)
      notify(`Copied “${w.album}”, ready to search for in a store.`)
    } catch {
      notify("Couldn't reach the clipboard.")
    }
  }

  return (
    <Detail title="To get" onBack={props.onBack}>
      <h1 className="title text-[34px] leading-[1.1]">To get</h1>
      <p className="mt-2 max-w-[66ch] text-[13.5px] leading-relaxed text-ink/60">
        {wanted.length
          ? `${plural(wanted.length, 'album')} with songs your playlists want and your library doesn't have yet, best first. When you add one to your library, its songs take their places in your playlists. Venuta never downloads anything.`
          : "Nothing to get. When a playlist you bring over has songs your library doesn't, their albums are listed here."}
      </p>
      {wanted.length > 0 && (
        <>
          <div className="mt-6 flex flex-wrap items-center gap-3">
            <Segmented
              label="Which albums"
              tabs
              value={tier}
              onChange={setTier}
              options={([1, 2, 3] as const).map((t) => ({
                value: String(t) as '1',
                disabled: counts[t] === 0,
                label: (
                  <>
                    {TIERS[t].label} <span className="ml-1 tabular-nums text-ink/45">{counts[t]}</span>
                  </>
                ),
              }))}
            />
            <button onClick={() => void exportFile()} className="btn-ghost btn-sm ml-auto">
              Save as a spreadsheet…
            </button>
          </div>
          <p className="mt-3 text-[12.5px] text-ink/50">{TIERS[Number(tier) as 1].line}</p>
          <AnimatePresence mode="wait" initial={false}>
            <motion.div
              key={tier}
              initial={{ opacity: 0, y: 6 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.16 }}
              className="mt-5 grid gap-1"
              role="list"
            >
              {shown.map((w) => (
                <div key={`${w.album}\u0000${w.artist}`} role="listitem" className="group flex items-center gap-4 rounded-xl px-3 py-2.5 transition-colors hover:bg-ink/[0.05]">
                  <span className="grid h-11 w-11 shrink-0 place-items-center rounded-lg bg-ink/[0.07] text-[13px] font-semibold tabular-nums text-ink/60" title={`${plural(w.saved_tracks, 'song')} saved`}>
                    {w.saved_tracks}
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-[14px] font-medium">{w.album}</span>
                    <span className="block truncate text-[12.5px] text-ink/55">
                      {w.artist}
                      {w.why ? `. ${w.why}` : ''}
                    </span>
                  </span>
                  <span className="hidden min-w-0 max-w-[34%] flex-wrap justify-end gap-1.5 md:flex">
                    {w.playlists.slice(0, 3).map((p) => (
                      <button key={p} onClick={() => props.onOpen(p)} className="max-w-[180px] truncate rounded-full bg-ink/[0.07] px-2.5 py-1 text-[11.5px] text-ink/70 transition-colors hover:bg-ink/[0.12] hover:text-ink" title={`Open ${p}`}>
                        {p}
                      </button>
                    ))}
                    {w.playlists.length > 3 && <span className="px-1 py-1 text-[11.5px] text-ink/45">+{w.playlists.length - 3}</span>}
                  </span>
                  <span className="flex shrink-0 gap-1 opacity-0 transition-opacity focus-within:opacity-100 group-hover:opacity-100">
                    <button onClick={() => void copy(w)} className="btn-ghost btn-sm" title="Copy the album and artist">
                      Copy name
                    </button>
                    {w.kept && (
                      <button onClick={() => void remove(w)} className="btn-ghost btn-sm" title="Take it off the list">
                        Remove
                      </button>
                    )}
                  </span>
                </div>
              ))}
            </motion.div>
          </AnimatePresence>
        </>
      )}
    </Detail>
  )
}
