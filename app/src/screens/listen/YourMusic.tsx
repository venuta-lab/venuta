import { useEffect, useMemo } from 'react'
import { loadCovers, useCovers } from '../../lib/covers'
import { yourMusicTiles, type YourMusicTile } from '../../lib/yourMusic'
import type { Library } from './model'

/*
  "Your music": everything on this PC, as tiles among the catalog's moods and genres (Home, and
  the Search page above Moods & moments). Each wears a fan of your own covers and opens that part
  of your library. Nothing here goes online.
*/

export type LibraryView = 'albums' | 'artists' | 'songs'

export function YourMusic(props: {
  root: string
  lib: Library
  /** Open a Library view, Albums with `filter` when given. */
  onOpen: (view: LibraryView, filter?: string) => void
  className?: string
}) {
  const { root, lib } = props
  const tiles = useMemo(
    () => yourMusicTiles({ albums: lib.albums, artists: lib.artists, songs: lib.tracks.length, hiRes: lib.hiRes, categories: lib.categories }),
    [lib],
  )
  const dirs = useMemo(() => [...new Set(tiles.flatMap((t) => t.covers))], [tiles])
  useEffect(() => {
    if (root && dirs.length) void loadCovers(root, dirs, 160)
  }, [root, dirs])
  if (!tiles.length) return null
  return (
    <section className={props.className ?? 'mt-10'}>
      <div className="mb-3.5 flex items-baseline gap-2.5">
        <h2 className="title text-[19px]">Your music</h2>
        <span className="text-[12.5px] text-ink/50">On this PC</span>
      </div>
      <div className="grid grid-cols-[repeat(auto-fill,minmax(176px,1fr))] gap-3">
        {tiles.map((t) => (
          <YourMusicCard key={t.id} tile={t} onOpen={() => props.onOpen(t.opens.view, t.opens.filter)} />
        ))}
      </div>
    </section>
  )
}

/** Three of the set's covers fanned at the top right, the name and count across the bottom. The
 *  section's heading says these are on this PC, so the tiles don't each say Local. */
function YourMusicCard({ tile, onOpen }: { tile: YourMusicTile; onOpen: () => void }) {
  const cover = useCovers(160)
  const arts = tile.covers.map((d) => cover(d)?.src).filter((s): s is string => !!s)
  const fan = [
    { r: -10, x: '-30%', z: 1 },
    { r: 0, x: '-12%', z: 2 },
    { r: 10, x: '6%', z: 3 },
  ].slice(-arts.length)
  return (
    <button
      onClick={onOpen}
      className="group relative block aspect-[16/9] overflow-hidden rounded-2xl bg-ink/[0.05] text-left shadow-[inset_0_0_0_1px_var(--edge)] transition-[background-color,transform] duration-200 hover:bg-ink/[0.08] active:scale-[0.98]"
    >
      <span aria-hidden className="absolute inset-0 opacity-70" style={{ background: 'radial-gradient(120% 140% at 100% 100%, color-mix(in oklab, var(--accent) 30%, transparent), transparent 70%)' }} />
      <span aria-hidden className="absolute right-[6%] top-[9%] h-[52%] w-[40%]">
        {arts.map((src, i) => (
          <img
            key={src + i}
            src={src}
            alt=""
            draggable={false}
            className="absolute right-0 top-0 aspect-square h-full rounded-md object-cover shadow-[0_8px_20px_-6px_rgb(0_0_0/0.55)] transition-transform duration-300 group-hover:-translate-y-1"
            style={{ transform: `translateX(${fan[i].x}) rotate(${fan[i].r}deg)`, zIndex: fan[i].z }}
          />
        ))}
      </span>
      <span className="absolute inset-x-3.5 bottom-2.5">
        <span className="block truncate text-[15px] font-semibold tracking-[-0.01em]">{tile.title}</span>
        <span className="block truncate text-[12px] text-ink/55">{tile.count}</span>
      </span>
    </button>
  )
}
