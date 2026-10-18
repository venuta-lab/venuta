import { invoke } from '@tauri-apps/api/core'
import { inApp } from './engine'
import { notify } from './notify'

/*
  How large the whole window draws (Ctrl + / Ctrl - / Ctrl 0): steps from 67% to 150%, kept on
  this computer, so text and controls can be larger without changing a theme.
*/

export const ZOOM_STEPS = [0.67, 0.75, 0.8, 0.9, 1, 1.1, 1.25, 1.5]
const STEPS = ZOOM_STEPS
const KEY = 'ui-zoom'

export function readZoom(): number {
  try {
    const v = Number(localStorage.getItem(KEY))
    return STEPS.includes(v) ? v : 1
  } catch {
    return 1
  }
}

export async function applyZoom(scale: number) {
  if (!inApp()) return
  await invoke('set_ui_zoom', { scale }).catch(() => {})
}

/** Draw at this size (one of the steps) and keep it; Settings shows the change at once. */
export function setZoom(scale: number) {
  if (!STEPS.includes(scale)) return
  try {
    localStorage.setItem(KEY, String(scale))
  } catch {
    // This visit only.
  }
  void applyZoom(scale)
  window.dispatchEvent(new Event(ZOOM_EVENT))
}

/** Sent when the size changes (Settings listens, so Ctrl + shows there too). */
export const ZOOM_EVENT = 'venuta:zoom'

/** One step larger (1), smaller (-1), or back to as designed (0). */
export function zoomBy(step: -1 | 0 | 1) {
  const now = readZoom()
  const i = STEPS.indexOf(now)
  const next = step === 0 ? 1 : STEPS[Math.max(0, Math.min(STEPS.length - 1, i + step))]
  setZoom(next)
  notify(next === 1 ? 'Drawn as designed (100%).' : `Drawn at ${Math.round(next * 100)}%.`)
}
