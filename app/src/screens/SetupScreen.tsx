import { motion } from 'motion/react'
import { useState } from 'react'
import { checkHome, inApp } from '../lib/engine'
import { useEscape } from '../lib/layers'

/**
 * The first time: add your music, or skip and listen to the catalog straight away. Either way
 * Venuta keeps its library (playlists, likes, what it knows) in one folder, made where it
 * suggests; it can be moved in Settings. Reading, looking up details and Organize's questions
 * are where they always are (Health, Organize), not steps to get through first.
 */
export function SetupScreen(props: {
  home: string | null
  suggestion: string
  /** Make the library here; the folder its own music goes in. */
  onChooseHome: (path: string) => Promise<string>
  /** Choose the folder with your music and open it; whether one was chosen. */
  onAddMusic: () => Promise<boolean>
  /** No music of your own for now: the library's own (empty) folder opens. */
  onSkip: (music: string) => void
  /** Leave (offered once there is a library to go back to). */
  onClose?: () => void
}) {
  const [working, setWorking] = useState(false)
  const [error, setError] = useState('')
  useEscape(!!props.onClose, () => props.onClose?.())

  /** The library's own music folder, making the library first if there isn't one. */
  async function library(): Promise<string> {
    if (props.home) return `${props.home.replace(/[\\/]+$/, '')}/Music`
    await checkHome(props.suggestion)
    return props.onChooseHome(props.suggestion)
  }

  async function run(then: (music: string) => Promise<void> | void) {
    setWorking(true)
    setError('')
    try {
      await then(await library())
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setWorking(false)
    }
  }

  return (
    <div className="grid min-h-full place-items-center px-6 pb-24 pt-16">
      <motion.div
        initial={{ opacity: 0, y: 12 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ type: 'spring', stiffness: 260, damping: 30 }}
        className="flex max-w-[560px] flex-col items-center text-center"
      >
        <h1 className="title text-[44px] leading-[1.05]">Welcome to Venuta</h1>
        <p className="mt-4 max-w-[46ch] text-[15px] leading-relaxed text-ink/65">
          Your music and the online catalog, together. Add the folder your albums are in, or start listening now and add
          yours any time.
        </p>
        <div className="mt-9 flex flex-wrap justify-center gap-3">
          <button
            onClick={() =>
              void run(async () => {
                await props.onAddMusic()
              })
            }
            disabled={working || !inApp()}
            className="btn-primary btn-lg"
          >
            Add your music
          </button>
          <button onClick={() => void run((music) => props.onSkip(music))} disabled={working || !inApp()} className="btn btn-lg">
            Skip for now
          </button>
        </div>
        <p className="mt-5 max-w-[50ch] text-[12.5px] leading-relaxed text-ink/50">
          Your music is only read, never changed. Later, add music from Home or from Organize, New music.
        </p>
        {error && (
          <p className="mt-4 text-[13px] text-problem" role="alert">
            {error}
          </p>
        )}
        {props.onClose && (
          <button onClick={props.onClose} className="btn-ghost btn-sm mt-8" title="Back (Esc)">
            Not now
          </button>
        )}
      </motion.div>
    </div>
  )
}
