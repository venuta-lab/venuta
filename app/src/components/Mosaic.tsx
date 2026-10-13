import { useEffect } from 'react'
import { loadCovers, useCovers } from '../lib/covers'

/**
 * A playlist's cover, made from its songs: four different albums as a 2×2 grid, else the first
 * album's cover, else a tile in the accent (a sparkle for smart playlists, a note otherwise).
 */
export function Mosaic(props: { root: string; dirs: string[]; smart?: boolean; className?: string }) {
  const { root, dirs } = props
  const key = dirs.slice(0, 4).join('|')
  useEffect(() => {
    if (root && key) void loadCovers(root, key.split('|'))
  }, [root, key])
  const cover = useCovers(320)
  const arts = dirs
    .slice(0, 4)
    .map((d) => cover(d)?.src)
    .filter((s): s is string => !!s)
  return (
    <div className={`relative overflow-hidden bg-ink/[0.06] ${props.className ?? ''}`}>
      {arts.length >= 4 ? (
        <div className="grid h-full w-full grid-cols-2 grid-rows-2">
          {arts.slice(0, 4).map((src, i) => (
            <img key={i} src={src} alt="" draggable={false} className="h-full w-full object-cover" />
          ))}
        </div>
      ) : arts.length > 0 ? (
        <img src={arts[0]} alt="" draggable={false} className="h-full w-full object-cover" />
      ) : (
        <div
          className="grid h-full w-full place-items-center text-ink/70"
          style={{ background: 'linear-gradient(135deg, color-mix(in oklab, var(--accent) 50%, transparent), color-mix(in oklab, var(--accent) 10%, transparent))' }}
        >
          <svg width="34%" height="34%" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
            {props.smart ? (
              <path d="M12 3.5l1.9 4.6 4.6 1.9-4.6 1.9L12 16.5l-1.9-4.6L5.5 10l4.6-1.9zM18.5 15.5l.8 1.9 1.9.8-1.9.8-.8 1.9-.8-1.9-1.9-.8 1.9-.8z" />
            ) : (
              <>
                <path d="M9 18V6l11-2v12" />
                <circle cx="6" cy="18" r="3" />
                <circle cx="17" cy="16" r="3" />
              </>
            )}
          </svg>
        </div>
      )}
      {props.smart && arts.length > 0 && (
        <span className="on-art absolute left-2 top-2 grid h-6 w-6 place-items-center rounded-full text-[12px]" title="Smart playlist" aria-label="Smart playlist">
          ✦
        </span>
      )}
    </div>
  )
}
