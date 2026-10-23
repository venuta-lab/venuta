import { useEffect, useState } from 'react'
import { playlistShelf, type PlaylistSummary } from '../../lib/engine'
import { PlaylistCard } from './PlaylistsHome'
import { useCardWidth } from '../listen/model'
import { CARD_FADE, ScrollRow } from '../../components/ScrollRow'

/**
 * "Your playlists" on the Music home: the ones you changed last first, smart ones after. Hidden
 * until there is a playlist with songs in it.
 */
export function PlaylistShelf(props: { root: string; onOpen: (name: string) => void; onSeeAll: () => void }) {
  const { root } = props
  const [lists, setLists] = useState<PlaylistSummary[]>([])
  const width = useCardWidth()
  useEffect(() => {
    let live = true
    const load = () =>
      playlistShelf(root)
        .then((l) => live && setLists(l))
        .catch(() => {})
    void load()
    window.addEventListener('venuta:playlists-changed', load)
    return () => {
      live = false
      window.removeEventListener('venuta:playlists-changed', load)
    }
  }, [root])

  const shown = lists
    .filter((p) => p.tracks - p.missing > 0)
    .sort((a, b) => Number(a.smart) - Number(b.smart) || b.modified - a.modified)
    .slice(0, 20)
  if (!shown.length) return null
  return (
    <section className="mt-10">
      <div className="mb-3.5 flex items-baseline gap-3">
        <h2 className="title text-[19px]">Your playlists</h2>
        <span className="text-[12.5px] text-ink/50">{lists.length.toLocaleString('en-IN')}</span>
        <button onClick={() => props.onSeeAll()} className="ml-auto rounded-full px-3 py-1 text-[12.5px] text-ink/65 hover:bg-ink/[0.08] hover:text-ink">
          See all
        </button>
      </div>
      <ScrollRow arrows={{ top: Math.round(width * 0.38) }} fade={CARD_FADE} className="-mx-3 -my-3 snap-x gap-5 px-3 py-3" style={{ scrollPaddingInline: 12 }}>
        {shown.map((p) => (
          <div key={p.name} className="shrink-0 snap-start" style={{ width }}>
            <PlaylistCard root={root} p={p} onOpen={() => props.onOpen(p.name)} />
          </div>
        ))}
      </ScrollRow>
    </section>
  )
}
