import { useEffect, useState, useSyncExternalStore } from 'react'

/*
  Work running in the background, app-wide: reading the music folder, making covers, looking
  up, checking, building, syncing. Screens start work through `track`, so it stays visible (in
  the title bar) after you leave the screen, and a screen that opens again can pick its bar back
  up with `useJob`.
*/
export interface Job {
  /** Unique per run; `id` names the kind of work and can repeat. */
  key: number
  id: string
  /** What is happening, as a short sentence ("Reading your music"). */
  label: string
  done: number
  /** 0 while the size of the work is not known yet. */
  total: number
  /** What the numbers count ("songs", "albums"). */
  unit?: string
  /** The thing being worked on right now, if known. */
  note?: string
  cancel?: () => void
  started: number
}

export type Update = (done: number, total: number, note?: string) => void

let jobs: Job[] = []
let next = 1
const listeners = new Set<() => void>()

function emit() {
  listeners.forEach((l) => l())
}

function put(job: Job) {
  const i = jobs.findIndex((j) => j.key === job.key)
  jobs = i < 0 ? [...jobs, job] : jobs.map((j, k) => (k === i ? job : j))
  emit()
}

function remove(key: number) {
  jobs = jobs.filter((j) => j.key !== key)
  emit()
}

/** Run `work` as a visible job; `update` moves its progress. The job ends with the promise. */
export async function track<T>(
  spec: { id: string; label: string; unit?: string; cancel?: () => void },
  work: (update: Update, relabel: (label: string) => void) => Promise<T>,
): Promise<T> {
  let job: Job = { ...spec, key: next++, done: 0, total: 0, started: Date.now() }
  put(job)
  // Progress can arrive hundreds of times a second; the bar only needs a few frames' worth.
  let frame = 0
  const flush = () => {
    frame = 0
    if (jobs.some((j) => j.key === job.key)) put(job)
  }
  const update: Update = (done, total, note) => {
    job = { ...job, done, total, note }
    if (!frame) frame = window.setTimeout(flush, 100)
  }
  const relabel = (label: string) => {
    job = { ...job, label, done: 0, total: 0, note: undefined }
    put(job)
  }
  try {
    return await work(update, relabel)
  } finally {
    window.clearTimeout(frame)
    remove(job.key)
  }
}

export const isRunning = (id: string) => jobs.some((j) => j.id === id)

function subscribe(l: () => void) {
  listeners.add(l)
  return () => {
    listeners.delete(l)
  }
}

export function useJobs(): Job[] {
  return useSyncExternalStore(subscribe, () => jobs)
}

/** The latest run of one kind of work, or undefined when none is running. */
export function useJob(id: string): Job | undefined {
  return useJobs().findLast((j) => j.id === id)
}

/** "1,234 of 4,573 songs", "1,250 found", or '' while nothing is counted yet. */
export function counted(job: Pick<Job, 'done' | 'total' | 'unit'>): string {
  const n = (v: number) => v.toLocaleString('en-IN')
  if (job.total > 0) return `${n(job.done)} of ${n(job.total)}${job.unit ? ` ${job.unit}` : ''}`
  if (job.done > 0) return `${n(job.done)}${job.unit ? ` ${job.unit}` : ''} found`
  return ''
}

/** "about 4 min left", once there is enough of a run to estimate from. */
export function timeLeft(job: Job, now: number): string {
  const spent = now - job.started
  if (job.total <= 0 || job.done <= 0 || spent < 4000 || job.done >= job.total) return ''
  const left = (spent * (job.total - job.done)) / job.done / 1000
  if (left < 50) return 'less than a minute left'
  if (left < 3600) return `about ${Math.round(left / 60)} min left`
  const h = Math.floor(left / 3600)
  return `about ${h} h ${Math.round((left - h * 3600) / 60)} min left`
}

/** 0–1, or null when the amount of work is not known (show an indeterminate bar). */
export const fraction = (job: Pick<Job, 'done' | 'total'>) =>
  job.total > 0 ? Math.min(1, job.done / job.total) : null

/** The time, ticking each second while `on`. */
export function useNow(on = true) {
  const [now, set] = useState(() => Date.now())
  useEffect(() => {
    if (!on) return
    const t = window.setInterval(() => set(Date.now()), 1000)
    return () => window.clearInterval(t)
  }, [on])
  return now
}
