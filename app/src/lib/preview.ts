import { useEffect, useState } from 'react'
import { assetUrl, inApp } from './engine'

/*
  Clips for deciding: a few seconds of a song from your own files, starting about a third of
  the way in so you hear the song rather than its intro. One clip plays at a time, app-wide.
*/
const CLIP_SECONDS = 12

let audio: HTMLAudioElement | null = null
let queue: string[] = []
let root = ''
let playing: string | null = null
let timer = 0
const listeners = new Set<(p: string | null) => void>()

function notify() {
  listeners.forEach((l) => l(playing))
}

function next() {
  window.clearTimeout(timer)
  const path = queue.shift()
  if (!path) {
    stopClips()
    return
  }
  if (!audio) audio = new Audio()
  const a = audio
  playing = path
  notify()
  a.src = assetUrl(`${root.replace(/[\\/]+$/, '')}/${path}`)
  a.onloadedmetadata = () => {
    if (Number.isFinite(a.duration)) a.currentTime = Math.max(0, a.duration * 0.33)
    a.play().catch(() => stopClips())
    timer = window.setTimeout(next, CLIP_SECONDS * 1000)
  }
  a.onended = next
  a.onerror = () => next()
}

/** Play clips of these songs one after another (library-relative paths). */
export function playClips(libraryRoot: string, paths: string[]) {
  if (!inApp() || !paths.length) return
  root = libraryRoot
  queue = [...paths]
  next()
}

export function stopClips() {
  window.clearTimeout(timer)
  queue = []
  audio?.pause()
  playing = null
  notify()
}

export const clipsAvailable = () => inApp()

/** The path playing now, or null. */
export function usePlaying(): string | null {
  const [p, set] = useState(playing)
  useEffect(() => {
    listeners.add(set)
    return () => {
      listeners.delete(set)
    }
  }, [])
  return p
}
