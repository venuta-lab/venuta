import { useEffect, useState } from 'react'

/**
 * Cover for an online catalog row. Accepts http(s) picture URLs only; on load failure
 * falls back to a quiet placeholder so broken icons never sit in the list.
 */
export function RemoteArt(props: {
  url?: string | null
  /** Shown when there is no usable picture (letter, note, etc.). */
  fallback?: React.ReactNode
  className?: string
  /** Extra classes on the image itself. */
  imgClassName?: string
}) {
  const src = cleanArtUrl(props.url)
  const [failed, setFailed] = useState(false)
  useEffect(() => setFailed(false), [src])
  if (!src || failed) {
    return <>{props.fallback ?? <span className="text-ink/30">♪</span>}</>
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

/** Keep only http(s) picture addresses; upgrade protocol-relative and bare http. */
export function cleanArtUrl(raw?: string | null): string | undefined {
  const s = (raw ?? '').trim()
  if (!s) return undefined
  let url = s
  if (url.startsWith('//')) url = `https:${url}`
  else if (url.startsWith('http://')) url = `https://${url.slice('http://'.length)}`
  if (!/^https:\/\//i.test(url)) return undefined
  // Never treat an audio/stream host as cover art.
  if (/googlevideo\.com|\.m3u8(\?|$)|mime=audio/i.test(url)) return undefined
  return url
}
