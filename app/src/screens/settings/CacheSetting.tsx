import { invoke } from '@tauri-apps/api/core'
import { useEffect, useState } from 'react'
import { confirmAsk } from '../../components/Confirm'
import { gb, inApp } from '../../lib/engine'
import { notify } from '../../lib/notify'

/*
  What Venuta keeps only to be quicker (crates/engine cache.rs): answers looked up online, lyrics
  found online, waveforms and spectrograms. Cleared, all of it comes back when it's needed.
*/

interface CacheSizes {
  answers: number
  lyrics: number
  pictures: number
}

const total = (s: CacheSizes) => s.answers + s.lyrics + s.pictures
const size = (b: number) => (b === 0 ? 'nothing' : b < 1e6 ? `${Math.max(1, Math.round(b / 1e3))} KB` : b < 1e9 ? `${(b / 1e6).toFixed(1)} MB` : gb(b))

export function CacheSetting() {
  const [sizes, setSizes] = useState<CacheSizes | null>(null)
  const [clearing, setClearing] = useState(false)
  const read = () =>
    void invoke<CacheSizes>('cache_sizes')
      .then(setSizes)
      .catch(() => {})
  useEffect(() => {
    if (inApp()) read()
  }, [])
  const clear = async () => {
    const ok = await confirmAsk({
      title: 'Clear the cache?',
      text: 'Venuta looks things up again as you need them: tags and covers, catalog pages, online lyrics, waveforms. Your settings, likes, playlists, history and lyric choices stay.',
      confirm: 'Clear cache',
    })
    if (!ok) return
    setClearing(true)
    try {
      const was = await invoke<CacheSizes>('clear_cache')
      notify(`Cleared ${size(total(was))}.`)
    } catch (e) {
      notify(`Couldn't clear the cache: ${String(e)}`)
    } finally {
      setClearing(false)
      read()
    }
  }
  if (!sizes) return null
  return (
    <div className="flex flex-wrap items-center gap-4 border-b border-edge py-4">
      <div className="min-w-[220px] flex-1">
        <div className="text-[14px] font-medium">Cache</div>
        <div className="mt-0.5 text-[12.5px] leading-relaxed text-ink/55">
          {total(sizes)
            ? `${size(total(sizes))} kept only to be quicker: answers looked up online ${size(sizes.answers)}, lyrics ${size(sizes.lyrics)}, waveforms and spectrograms ${size(sizes.pictures)}. Cleared, it all comes back when needed.`
            : 'Empty. Venuta keeps answers looked up online, lyrics and waveforms here, to be quicker the next time.'}
        </div>
      </div>
      <button onClick={() => void clear()} disabled={clearing || total(sizes) === 0} className="btn btn-sm">
        {clearing ? 'Clearing…' : 'Clear cache'}
      </button>
    </div>
  )
}
