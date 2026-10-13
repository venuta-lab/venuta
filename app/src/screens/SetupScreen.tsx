import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useState } from 'react'
import { open } from '@tauri-apps/plugin-dialog'
import { checkHome, getSettings, inApp, type HealthReport, type Settings } from '../lib/engine'
import { DeepProgressBar } from './LibraryScreen'

const spring = { type: 'spring', stiffness: 380, damping: 34 } as const

const STEPS = ['Your library', 'Music to bring in', 'Reading', 'Details online', 'Your questions'] as const

const SOUND_ISSUES = new Set(['damaged_audio', 'lossy_files', 'upsampled_hires', 'padded_24bit', 'limited_highs'])

const norm = (p: string) => p.replace(/[\\/]+/g, '/').replace(/\/$/, '').toLowerCase()

/**
 * Setting up, once: where your library lives (Venuta keeps everything there), the music to
 * bring in (or none), reading it, looking up details online, then Organize. Every step can be
 * revisited; your music folder is only read, and nothing is built until you choose Build.
 */
export function SetupScreen(props: {
  root: string
  home: string | null
  /** The library's Music folder, where its albums are. */
  music: string | null
  suggestion: string
  report: HealthReport | null
  busy: string
  lookup: { done: number; total: number; note?: string } | null
  onChooseHome: (path: string) => Promise<void>
  onChooseMusic: () => Promise<void>
  onStartEmpty: () => void
  onLookup: () => Promise<void>
  onCancelLookup: () => void
  onDone: () => void
}) {
  const bringing = !!props.root && !!props.music && norm(props.root) !== norm(props.music)
  const [step, setStep] = useState(props.home ? (bringing ? 2 : 1) : 0)
  const [settings, setS] = useState<Settings | null>(null)
  const [error, setError] = useState('')
  const [working, setWorking] = useState(false)
  const [looked, setLooked] = useState(false)
  const [empty, setEmpty] = useState(false)

  useEffect(() => {
    getSettings().then(setS)
  }, [])
  // A music folder was chosen: on to reading it.
  const chosen = bringing && step === 1
  useEffect(() => {
    if (chosen) setStep(2)
  }, [chosen])

  async function chooseHome(path: string) {
    setWorking(true)
    try {
      await checkHome(path)
      await props.onChooseHome(path)
      setError('')
      setStep(1)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setWorking(false)
    }
  }

  async function lookup() {
    await props.onLookup()
    setLooked(true)
    setStep(4)
  }

  function startEmpty() {
    setEmpty(true)
    props.onStartEmpty()
    setStep(4)
  }

  const s = props.report?.summary
  const sound = new Set(
    (props.report?.issues ?? []).filter((i) => SOUND_ISSUES.has(i.id)).flatMap((i) => i.albums),
  ).size
  const keys = [settings?.discogs_token && 'Discogs', settings?.acoustid_key && 'AcoustID'].filter(Boolean)
  const reachable = (i: number) => i <= step || (i <= 3 && bringing)

  return (
    <div className="mx-auto max-w-[760px] pb-24 pt-10">
      <h1 className="text-[40px] font-semibold leading-none tracking-[-0.035em]">Set up your library</h1>
      <p className="mt-3 max-w-[58ch] text-[14.5px] leading-relaxed text-ink/60">
        Your library is one folder: your music, tagged and with covers and lyrics, plus everything Venuta knows about it. Music
        you already have is only read; a clean copy is built in your library when you choose Build.
      </p>

      <ol className="mt-8 flex flex-wrap gap-2" aria-label="Steps">
        {STEPS.map((label, i) => {
          const done = i < step
          const on = i === step
          return (
            <li key={label}>
              <button
                onClick={() => reachable(i) && setStep(i)}
                aria-current={on ? 'step' : undefined}
                className={`relative flex items-center gap-2 rounded-full px-3.5 py-1.5 text-[12.5px] ${on ? 'text-ink' : done ? 'text-ink/75' : 'text-ink/40'}`}
              >
                {on && <motion.span layoutId="setup-step" transition={spring} className="absolute inset-0 rounded-full bg-ink/[0.12]" />}
                <span
                  className="relative grid h-5 w-5 place-items-center rounded-full text-[11px] font-semibold"
                  style={{ background: done || on ? 'var(--accent)' : 'color-mix(in oklab, var(--ink) 12%, transparent)', color: done || on ? 'var(--on-accent)' : undefined }}
                >
                  {done ? '✓' : i + 1}
                </span>
                <span className="relative">{label}</span>
              </button>
            </li>
          )
        })}
      </ol>

      <AnimatePresence mode="wait">
        <motion.section
          key={step}
          initial={{ opacity: 0, y: 10 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: -6 }}
          transition={{ duration: 0.2 }}
          className="glass mt-6 rounded-[24px] p-7"
        >
          {step === 0 && (
            <Step
              title="Where should your library live?"
              text="A new or empty folder, or a library Venuta made before. Your music is built here, and Venuta keeps its own data with it (in a hidden .venuta folder), so the whole library moves or backs up as one folder."
            >
              {props.home ? (
                <div className="flex flex-wrap items-center gap-3">
                  <span className="text-[14px]">Your library is {props.home}.</span>
                  <Primary onClick={() => setStep(1)}>Continue</Primary>
                </div>
              ) : (
                <div className="flex flex-wrap gap-2">
                  <Primary onClick={() => void chooseHome(props.suggestion)} disabled={working || !inApp()}>
                    Use {props.suggestion}
                  </Primary>
                  <Secondary
                    onClick={async () => {
                      const p = inApp() ? await open({ directory: true, title: 'Where should your library live?' }) : null
                      if (typeof p === 'string') await chooseHome(p)
                    }}
                  >
                    Choose another folder
                  </Secondary>
                </div>
              )}
              {error && <p className="mt-3 text-[13px] text-problem">{error}</p>}
            </Step>
          )}

          {step === 1 && (
            <Step
              title="Do you have music to bring in?"
              text="Choose the folder that holds your albums. It is only read, never changed; a clean copy is built in your library. No collection yet? Start empty and add music as you get it."
            >
              <div className="flex flex-wrap gap-2">
                <Primary onClick={() => void props.onChooseMusic()} disabled={!inApp()}>
                  Choose your music folder
                </Primary>
                <Secondary onClick={startEmpty}>Start empty</Secondary>
              </div>
            </Step>
          )}

          {step === 2 && (
            <Step title="Reading your music" text={s ? 'Done. Here is what is there.' : props.busy || 'Reading every file once…'}>
              {s && s.tracks === 0 && (
                <div>
                  <p className="text-[14px] text-ink/70">No songs were found in {props.root}. Choose the folder that holds your albums.</p>
                  <div className="mt-5 flex flex-wrap gap-2">
                    <Primary onClick={() => void props.onChooseMusic()}>Choose another folder</Primary>
                    <Secondary onClick={startEmpty}>Start empty instead</Secondary>
                  </div>
                </div>
              )}
              {s && s.tracks > 0 && (
                <>
                  <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
                    <Stat n={s.albums} label="albums" />
                    <Stat n={s.tracks} label="songs" />
                    <Stat n={Math.round(s.hours)} label="hours" />
                    <Stat n={sound} label={sound === 1 ? 'album with sound problems' : 'albums with sound problems'} />
                  </div>
                  <div className="mt-6">
                    <Primary onClick={() => setStep(3)}>Continue</Primary>
                  </div>
                </>
              )}
            </Step>
          )}

          {step === 3 && (
            <Step
              title="Look up details online"
              text="Original years, composers and lyricists, sharper covers and synced lyrics, from free sources. It takes a while the first time and much less afterwards; you can stop and continue later."
            >
              <p className="text-[13px] text-ink/55">
                Using MusicBrainz, Apple, Deezer and LRCLIB{keys.length ? `, plus ${keys.join(' and ')} with your keys` : ''}. Sources and
                keys are in Settings, Online sources.
              </p>
              <AnimatePresence>
                {props.lookup && <DeepProgressBar deep={props.lookup} label="Looking up" unit="steps" onCancel={props.onCancelLookup} />}
              </AnimatePresence>
              <div className="mt-6 flex flex-wrap gap-2">
                <Primary onClick={() => void lookup()} disabled={!!props.lookup || !inApp()}>
                  {props.lookup ? 'Looking up…' : 'Start looking up'}
                </Primary>
                <Secondary onClick={() => setStep(4)}>Skip for now</Secondary>
              </div>
            </Step>
          )}

          {step === 4 &&
            (empty ? (
              <Step
                title="Your library is ready"
                text="Add music you've bought or downloaded in Organize, New music. It comes in tagged, named and with covers and lyrics. You can bring in a music folder later from Settings."
              >
                <Primary onClick={props.onDone}>Open Organize</Primary>
              </Step>
            ) : (
              <Step
                title="Your questions are ready"
                text={`${looked ? 'Details are in. ' : ''}Organize shows the few things only you can decide, what was handled for you with proof, and what the build will change. Then build.`}
              >
                <Primary onClick={props.onDone}>Open Organize</Primary>
              </Step>
            ))}
        </motion.section>
      </AnimatePresence>
    </div>
  )
}

function Step(props: { title: string; text: string; children: React.ReactNode }) {
  return (
    <div>
      <h2 className="text-[24px] font-semibold tracking-[-0.02em]">{props.title}</h2>
      <p className="mt-2 max-w-[60ch] text-[14px] leading-relaxed text-ink/65">{props.text}</p>
      <div className="mt-6">{props.children}</div>
    </div>
  )
}

function Primary(props: { onClick: () => void; disabled?: boolean; children: React.ReactNode }) {
  return (
    <motion.button
      whileTap={{ scale: 0.97 }}
      onClick={props.onClick}
      disabled={props.disabled}
      className="rounded-full bg-accent px-6 py-2.5 text-[14px] font-medium text-on-accent disabled:opacity-40"
    >
      {props.children}
    </motion.button>
  )
}

function Secondary(props: { onClick: () => void; children: React.ReactNode }) {
  return (
    <button onClick={props.onClick} className="rounded-full bg-ink/[0.08] px-5 py-2.5 text-[14px] hover:bg-ink/[0.14]">
      {props.children}
    </button>
  )
}

function Stat(props: { n: number; label: string }) {
  return (
    <div className="rounded-2xl bg-ink/[0.05] px-4 py-3">
      <div className="text-[26px] font-semibold tracking-[-0.02em]">{props.n.toLocaleString('en-IN')}</div>
      <div className="text-[12.5px] text-ink/55">{props.label}</div>
    </div>
  )
}
