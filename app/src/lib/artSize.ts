// Asking the catalog's picture host for a cover at the size it is drawn.
//
// Catalog cover addresses carry their size inside them (`…=w60-h60-l90-rj` on album art,
// `…=s120-c-k…` on artist pictures), and the host serves any size asked for. A 40 px row asks for
// a small file and Now Playing for a large one, so neither is blurry nor wasteful. Addresses with a
// signed size (video frames with `sqp=`) and other hosts are left as they are. The same rules live
// in Rust (media-access `sized_art`).

const SIZES = [120, 226, 360, 544, 800, 1200]

/** The size asked for: the smallest cached size at least `px`, at most 1200. */
function bucket(px: number): number {
  return SIZES.find((s) => s >= px) ?? 1200
}

/** The same picture at `px` pixels square, when its address has a size to change. */
export function sizedArt(url: string, px: number): string {
  const size = bucket(Math.max(1, Math.round(px)))
  let host = ''
  try {
    host = new URL(url).hostname
  } catch {
    return url
  }
  if (!host.endsWith('googleusercontent.com') && !host.endsWith('ggpht.com')) return url
  const eq = url.lastIndexOf('=')
  if (eq < 0) return url
  const options = url.slice(eq + 1)
  if (options.includes('/')) return url
  let changed = false
  const parts = options.split('-').map((part) => {
    if (/^[whs]\d+$/.test(part)) {
      changed = true
      return `${part[0]}${size}`
    }
    return part
  })
  return changed ? `${url.slice(0, eq + 1)}${parts.join('-')}` : url
}

/** Pixels to ask for a picture drawn `cssPx` wide on this screen (sharp at 125 % and 150 %). */
export function devicePx(cssPx: number): number {
  const ratio = typeof window !== 'undefined' ? window.devicePixelRatio || 1 : 1
  return cssPx * Math.min(ratio, 3)
}
