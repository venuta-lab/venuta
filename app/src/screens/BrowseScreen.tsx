import { AnimatePresence, motion } from 'motion/react'
import { useMemo, useState } from 'react'
import { createPortal } from 'react-dom'
import type { AlbumSummary } from '../lib/engine'
import { setAccent, useCovers } from '../lib/covers'

const spring = { type: 'spring', stiffness: 300, damping: 30 } as const

/** Album wall. Click a cover and it lifts out into a detail view (shared-element transition). */
export function BrowseScreen(props: { albums: AlbumSummary[] }) {
  const cover = useCovers(320)
  const [query, setQuery] = useState('')
  const [selected, setSelected] = useState<AlbumSummary | null>(null)

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase()
    const list = q
      ? props.albums.filter((a) => `${a.title} ${a.album_artist} ${a.year ?? ''}`.toLowerCase().includes(q))
      : props.albums
    return [...list].sort((a, b) => a.title.localeCompare(b.title))
  }, [props.albums, query])

  return (
    <div className="pb-16">
      <div className="sticky top-0 z-10 flex items-center gap-4 pb-5 pt-6">
        <h1 className="text-[34px] font-semibold tracking-[-0.03em]">Albums</h1>
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search albums, artists, years"
          className="glass ml-auto w-[320px] rounded-full px-5 py-2.5 text-[14px] text-ink placeholder:text-ink/45 focus:outline-none"
        />
      </div>

      <div className="grid grid-cols-[repeat(auto-fill,minmax(168px,1fr))] gap-x-5 gap-y-7">
        {shown.map((a) => {
          const c = cover(a.dir)
          return (
            <motion.button
              key={a.dir}
              onClick={() => {
                setAccent(c?.accent)
                setSelected(a)
              }}
              onHoverStart={() => setAccent(c?.accent)}
              whileHover={{ y: -6 }}
              transition={spring}
              className="group text-left"
            >
              <motion.div
                layoutId={`cover-${a.dir}`}
                className="aspect-square overflow-hidden rounded-2xl bg-white/[0.06] shadow-xl shadow-black/35"
              >
                {c && (
                  <img
                    src={c.src}
                    alt=""
                    draggable={false}
                    loading="lazy"
                    className="h-full w-full object-cover transition-transform duration-500 group-hover:scale-[1.04]"
                  />
                )}
              </motion.div>
              <div className="mt-2.5 truncate text-[14px] font-medium">{a.title}</div>
              <div className="truncate text-[12.5px] text-ink/55">
                {a.album_artist}
                {a.year ? `, ${a.year}` : ''}
              </div>
            </motion.button>
          )
        })}
      </div>

      {/* Portal: a fixed overlay inside the animated page container would be positioned
          relative to that container, not the window. */}
      {createPortal(
        <AnimatePresence>
          {selected && (
            <AlbumDetail album={selected} src={cover(selected.dir)?.src} onClose={() => setSelected(null)} />
          )}
        </AnimatePresence>,
        document.body,
      )}
    </div>
  )
}

function AlbumDetail(props: { album: AlbumSummary; src?: string; onClose: () => void }) {
  const a = props.album
  const minutes = Math.round(a.duration_ms / 60000)
  return (
    <motion.div
      className="fixed inset-0 z-40 grid place-items-center p-10"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      onClick={props.onClose}
    >
      <div className="absolute inset-0 bg-black/45 backdrop-blur-md" />
      <motion.div
        onClick={(e) => e.stopPropagation()}
        className="glass relative flex w-full max-w-[900px] items-center gap-10 rounded-[28px] p-8"
        initial={{ y: 30, opacity: 0 }}
        animate={{ y: 0, opacity: 1 }}
        exit={{ y: 20, opacity: 0 }}
        transition={spring}
        drag="y"
        dragConstraints={{ top: 0, bottom: 0 }}
        dragElastic={0.4}
        onDragEnd={(_, info) => {
          if (Math.abs(info.offset.y) > 120) props.onClose()
        }}
      >
        <motion.div
          layoutId={`cover-${a.dir}`}
          className="aspect-square w-[340px] shrink-0 overflow-hidden rounded-2xl shadow-2xl shadow-black/50"
          transition={spring}
        >
          {props.src && <img src={props.src} alt="" draggable={false} className="h-full w-full object-cover" />}
        </motion.div>
        <div className="min-w-0">
          <h2 className="text-[36px] font-semibold leading-tight tracking-[-0.03em]">{a.title}</h2>
          <p className="mt-2 text-[17px] text-ink/75">{a.album_artist}</p>
          <p className="mt-6 text-[14px] text-ink/55">
            {a.year ? `Released ${a.year}. ` : ''}
            {a.tracks} tracks, {minutes} minutes.
          </p>
          <p className="mt-1 break-all text-[12px] text-ink/40">{a.dir}</p>
          <p className="mt-8 text-[12px] text-ink/40">Swipe down or click outside to close.</p>
        </div>
      </motion.div>
    </motion.div>
  )
}
