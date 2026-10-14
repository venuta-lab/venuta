// Venuta's icons: one V with a peacock feather and a flute's holes, in many materials. The one
// chosen in Settings is on the windows, the tray and the shortcuts on this computer.

import { invoke } from '@tauri-apps/api/core'
import { useEffect, useState } from 'react'
import { inApp } from './engine'

export interface AppIcon {
  id: string
  name: string
}

/** The icon Venuta has unless another is chosen (also the program's own). */
export const DEFAULT_ICON = 'dusk-glass'

export const APP_ICONS: AppIcon[] = [
  { id: 'dusk-glass', name: 'Dusk glass' },
  { id: 'pearl', name: 'Pearl' },
  { id: 'stained-glass', name: 'Stained glass' },
  { id: 'verdigris', name: 'Verdigris' },
  { id: 'ebony', name: 'Ebony on ivory' },
  { id: 'glow-stone', name: 'Glow stone' },
  { id: 'machined', name: 'Machined' },
  { id: 'carbon', name: 'Carbon' },
  { id: 'gold', name: 'Gold' },
  { id: 'crystal', name: 'Crystal' },
  { id: 'holographic', name: 'Holographic' },
  { id: 'frost', name: 'Frost' },
  { id: 'ember', name: 'Ember' },
  { id: 'steel-plate', name: 'Steel plate' },
  { id: 'jade-walnut', name: 'Jade on walnut' },
  { id: 'opal', name: 'Opal' },
  { id: 'neon-blue', name: 'Neon blue' },
  { id: 'neon-dusk', name: 'Neon dusk' },
  { id: 'emerald', name: 'Emerald' },
  { id: 'porcelain', name: 'Porcelain' },
  { id: 'nebula', name: 'Nebula' },
  { id: 'prism', name: 'Prism' },
  { id: 'liquid-metal', name: 'Liquid metal' },
  { id: 'ruby-ebony', name: 'Ruby on ebony' },
  { id: 'sapphire-rain', name: 'Sapphire rain' },
  { id: 'lava', name: 'Lava' },
  { id: 'mosaic', name: 'Mosaic' },
  { id: 'sandstone', name: 'Sandstone' },
  { id: 'marble-gold', name: 'Marble and gold' },
  { id: 'bronze-relic', name: 'Bronze relic' },
  { id: 'mesh', name: 'Mesh' },
  { id: 'amber-leather', name: 'Amber on leather' },
  { id: 'brushed-silver', name: 'Brushed silver' },
]

/** An icon's picture, from the app's own files. */
export const iconSrc = (id: string) => `/app-icons/${id}.png`

let current = DEFAULT_ICON
let asked = false
const listeners = new Set<(id: string) => void>()

/** The icon in use (the default until the app says otherwise). */
export function useAppIcon(): string {
  const [id, setId] = useState(current)
  useEffect(() => {
    listeners.add(setId)
    if (!asked && inApp()) {
      asked = true
      invoke<string>('app_icon').then((v) => {
        current = v
        listeners.forEach((l) => l(v))
      }, () => {})
    }
    return () => {
      listeners.delete(setId)
    }
  }, [])
  return id
}

/** Use this icon: the windows and tray at once, and the shortcuts that open Venuta. */
export async function chooseAppIcon(id: string) {
  const was = current
  current = id
  listeners.forEach((l) => l(id))
  try {
    if (inApp()) await invoke('set_app_icon', { id })
  } catch (e) {
    current = was
    listeners.forEach((l) => l(was))
    throw e
  }
}
