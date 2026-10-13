import { AnimatePresence, motion } from 'motion/react'
import { useEffect, useState } from 'react'
import { open } from '@tauri-apps/plugin-dialog'
import { checkNewLibrary, getSettings, inApp, setSettings, type HealthReport, type Settings } from '../lib/engine'
import { DeepProgressBar } from './LibraryScreen'

const spring = { type: 'spring', stiffness: 380, damping: 34 } as const

const STEPS = ['Your music', 'The new library', 'Reading', 'Details online', 'Your questions'] as const

const SOUND_ISSUES = new Set(['damaged_audio', 'lossy_files', 'upsampled_hires', 'padded_24bit', 'limited_highs'])

/**
 * Setting up, once: where your music is, where the clean copy goes, reading it, looking up
 * details online, then Organize. Every step can be revisited; nothing is changed until you
 * build from Organize.
 */
export function SetupScreen(props: {
  root: string
  report: HealthReport | null
  busy: string
  lookup: { done: number; total: number; note?: string } | null
  onChooseMusic: () => Promise<void>
  onLookup: () => Promise<void>
  onCancelLookup: () => void
  onDone: () => void
}) {
  const [step, setStep] = useState(props.root ? 1 : 0)
  const [settings, setS] = useState<Settings | null>(null)
  const [error, setError] = useState('')
  const [looked, setLooked] = useState(false)

  useEffect(() => {
    getSettings().then(setS)
  }, [])
  useEffect(() => {
    if (props.root && step === 0) setStep(1)
  }, [props.root, step])

  const parent = props.root.replace(/[\\/]+$/, '').split(/[\\/]/).slice(0, -1).join('/') || props.root
  const suggestion = `${parent}/Music Library`

  async function chooseTarget(path: string) {
    try {
      await checkNewLibrary(props.root, path)
      const next = { ...(settings ?? { rules_dir: null, new_library: null }), new_library: path }
      await setSettings(next)
      setS(next)
      setError('')
      setStep(2)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  async function lookup() {
    await props.onLookup()
    setLooked(true)
    setStep(4)
  }

  const s = props.report?.summary
  const sound = new Set(
    (props.report?.issues ?? []).filter((i) => SOUND_ISSUES.has(i.id)).flatMap((i) => i.albums),
  ).size
  const keys = [settings?.discogs_token && 'Discogs', settings?.acoustid_key && 'AcoustID'].filter(Boolean)

  return (
    <div className="mx-auto max-w-[760px] pb-24 pt-10">
      <h1 className="text-[40px] font-semibold leading-none tracking-[-0.035em]">Set up your library</h1>
      <p className="mt-3 max-w-[58ch] text-[14.5px] leading-relaxed text-ink/60">
        A clean copy of your music is built in a new folder, with full details, lyrics and covers. Your current folder is
        only read. Nothing is written until you choose Build in Organize.
      </p>

      <ol className="mt-8 flex flex-wrap gap-2" aria-label="Steps">
        {STEPS.map((label, i) => {
          const done = i < step
          const on = i === step
          return (
            <li key={label}>
              <button
                onClick={() => (i <= step || (i === 2 && props.root)) && setStep(i)}
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
            <Step title="Where is your music?" text="Choose the folder that holds your albums. It is read, never changed.">
              <Primary onClick={() => void props.onChooseMusic()} disabled={!inApp()}>
                Choose your music folder
              </Primary>
            </Step>
          )}

          {step === 1 && (
            <Step
              title="Where should the clean library go?"
              text="An empty folder (or one this app made before). The new library is built here; your current folder stays as it is."
            >
              <div className="flex flex-wrap gap-2">
                {settings?.new_library && (
                  <Primary onClick={() => void chooseTarget(settings.new_library!)}>Use {settings.new_library}</Primary>
                )}
                {!settings?.new_library && <Primary onClick={() => void chooseTarget(suggestion)}>Use {suggestion}</Primary>}
                <Secondary
                  onClick={async () => {
                    const p = inApp() ? await open({ directory: true, title: 'Where should the new library be built?' }) : null
                    if (typeof p === 'string') await chooseTarget(p)
                  }}
                >
                  Choose another folder
                </Secondary>
              </div>
              {error && <p className="mt-3 text-[13px] text-problem">{error}</p>}
            </Step>
          )}

          {step === 2 && (
            <Step title="Reading your music" text={s ? 'Done. Here is what is there.' : props.busy || 'Reading every file once…'}>
              {s && (
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

          {step === 4 && (
            <Step
              title="Your questions are ready"
              text={`${looked ? 'Details are in. ' : ''}Organize shows the few things only you can decide, what was handled for you with proof, and what the build will change. Then build.`}
            >
              <Primary onClick={props.onDone}>Open Organize</Primary>
            </Step>
          )}
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
