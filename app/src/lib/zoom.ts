import { invoke } from '@tauri-apps/api/core'
import { inApp } from './engine'
import { notify } from './notify'

/*
  How large the whole window draws (Ctrl + / Ctrl - / Ctrl 0): steps from 67% to 150%, kept on
  this computer, so text and controls can be larger without changing a theme.
*/

const STEPS = [0.67, 0.75, 0.8, 0.9, 1, 1.1, 1.25, 1.5]
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

/** One step larger (1), smaller (-1), or back to as designed (0). */
export function zoomBy(step: -1 | 0 | 1) {
  const now = readZoom()
  const i = STEPS.indexOf(now)
  const next = step === 0 ? 1 : STEPS[Math.max(0, Math.min(STEPS.length - 1, i + step))]
  try {
    localStorage.setItem(KEY, String(next))
  } catch {
    // This visit only.
  }
  void applyZoom(next)
  notify(next === 1 ? 'Drawn as designed (100%).' : `Drawn at ${Math.round(next * 100)}%.`)
}
