import { useEffect, useState } from 'react'
import { hueOf } from '../lib/onlineSong'
import { devicePx, isLocalAsset, sizedArt } from '../lib/artSize'
import { isLocalOnly } from '../lib/localOnly'

/**
 * Cover for an online catalog row. Accepts http(s) picture URLs only; on load failure
 * falls back to the online cover, so broken icons never sit in the list.
 */
export function RemoteArt(props: {
  url?: string | null
  /** What the online cover is made from when there is no picture: the album, else the title. */
  seed?: string
  /** Shown instead of the online cover when there is no usable picture (an artist's letter). */
  fallback?: React.ReactNode
  className?: string
  /** Extra classes on the image itself. */
  imgClassName?: string
  /** How wide it is drawn, in CSS pixels: the picture is asked for at that size. */
  size?: number
}) {
  const src = artUrl(props.url, props.size ?? 96)
  const [failed, setFailed] = useState(false)
  useEffect(() => setFailed(false), [src])
  if (!src || failed) {
    return <>{props.fallback ?? <OnlineCover seed={props.seed ?? ''} />}</>
  }
  return (
    <img
      src={src}
      alt=""
      draggable={false}
      loading="lazy"
      onError={() => setFailed(true)}
      className={props.imgClassName ?? props.className ?? 'h-full w-full object-cover'}
    />
  )
}

/** A cover address cleaned (see cleanArtUrl) and asked for at the size it is drawn, `cssPx` wide. */
export function artUrl(raw: string | null | undefined, cssPx: number): string | undefined {
  // A picture on this computer (a room song's cover): as it is, Local only too.
  if (raw && isLocalAsset(raw)) return raw
  // Local only: no picture is fetched from the catalog (the drawn cover shows instead).
  if (isLocalOnly()) return undefined
  const url = cleanArtUrl(raw)
  return url ? sizedArt(url, devicePx(cssPx)) : undefined
}

/** Keep only http(s) picture addresses (and pictures on this computer); upgrade protocol-relative
 *  and bare http. */
export function cleanArtUrl(raw?: string | null): string | undefined {
  const s = (raw ?? '').trim()
  if (!s) return undefined
  if (isLocalAsset(s)) return s
  let url = s
  if (url.startsWith('//')) url = `https:${url}`
  else if (url.startsWith('http://')) url = `https://${url.slice('http://'.length)}`
  if (!/^https:\/\//i.test(url)) return undefined
  // Never treat an audio/stream host as cover art.
  if (/googlevideo\.com|\.m3u8(\?|$)|mime=audio/i.test(url)) return undefined
  return url
}

/**
 * The cover of an online song or album without a picture: the same everywhere (row, card, bar,
 * Now Playing), a soft two-tone gradient whose hue comes from the album (so one album's songs
 * share it) and a note. Fills its box; the box gives the shape.
 */
export function OnlineCover({ seed }: { seed: string }) {
  const hue = hueOf(seed)
  return (
    <span
      aria-hidden
      className="grid h-full w-full place-items-center text-white/80"
      style={{ background: `linear-gradient(140deg, oklch(0.72 0.09 ${hue}), oklch(0.46 0.08 ${(hue + 40) % 360}))` }}
    >
      <svg viewBox="0 0 24 24" className="h-[42%] w-[42%] drop-shadow-[0_1px_2px_rgb(0_0_0/0.25)]" fill="currentColor">
        <path d="M12 3v10.55A4 4 0 1 0 14 17V7h4V3z" />
      </svg>
    </span>
  )
}
