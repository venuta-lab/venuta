// How a song is put into words and colour where the app can't read it from a file: lengths not
// known yet, an online song's quality, the online cover's colour. Pure, so tests run it as is.

/** 3:05, 1:02:10. */
export function clock(ms: number): string {
  const t = Math.max(0, Math.floor(ms / 1000))
  const h = Math.floor(t / 3600)
  const m = Math.floor((t % 3600) / 60)
  const s = String(t % 60).padStart(2, '0')
  return h ? `${h}:${String(m).padStart(2, '0')}:${s}` : `${m}:${s}`
}

/** A song's length for reading: "–" while it isn't known (an online song before it opens). */
export function songLength(ms: number | null | undefined): string {
  return ms && ms > 0 ? clock(ms) : '–'
}

/** An online song's quality in words, "AAC 129 kbps", from what the stream says; '' until it
 *  has opened (no signal yet). The codec is left out when the stream's kind isn't known. */
export function onlineQuality(s: { source: { codec: string }; file: { bitrate_kbps: number } } | null): string {
  if (!s) return ''
  const kbps = s.file.bitrate_kbps ? `${Math.round(s.file.bitrate_kbps)} kbps` : ''
  const codec = s.source.codec === 'Audio' || s.source.codec === 'Remote' ? '' : s.source.codec
  return [codec, kbps].filter(Boolean).join(' ')
}

/** A steady hue for a name: the same album always gets the same colour. */
export function hueOf(seed: string): number {
  let h = 0
  for (const c of seed.trim().toLowerCase()) h = (h * 31 + c.codePointAt(0)!) >>> 0
  return h % 360
}
