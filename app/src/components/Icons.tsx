// Minimal line icons, drawn on a 24px grid with round caps to match Sora's geometry.
import type { SVGProps } from 'react'

const base = {
  width: 22,
  height: 22,
  viewBox: '0 0 24 24',
  fill: 'none',
  stroke: 'currentColor',
  strokeWidth: 1.7,
  strokeLinecap: 'round',
  strokeLinejoin: 'round',
} as const

type P = SVGProps<SVGSVGElement>

export const IconHealth = (p: P) => (
  <svg {...base} {...p}>
    <path d="M3 12h4l2.5-6 4 12 2.5-6H21" />
  </svg>
)
export const IconBrowse = (p: P) => (
  <svg {...base} {...p}>
    <rect x="3.5" y="3.5" width="7" height="7" rx="2" />
    <rect x="13.5" y="3.5" width="7" height="7" rx="2" />
    <rect x="3.5" y="13.5" width="7" height="7" rx="2" />
    <rect x="13.5" y="13.5" width="7" height="7" rx="2" />
  </svg>
)
export const IconReview = (p: P) => (
  <svg {...base} {...p}>
    <path d="M4 6h10M4 12h7M4 18h10" />
    <path d="m15 15 2.5 2.5L21 13" />
  </svg>
)
export const IconIncoming = (p: P) => (
  <svg {...base} {...p}>
    <path d="M12 3v11m0 0-4-4m4 4 4-4" />
    <path d="M4 15v3a3 3 0 0 0 3 3h10a3 3 0 0 0 3-3v-3" />
  </svg>
)
export const IconPlaylists = (p: P) => (
  <svg {...base} {...p}>
    <path d="M4 6h12M4 11h12M4 16h7" />
    <circle cx="17.5" cy="17.5" r="2.5" />
    <path d="M20 17.5V9.5l-2.5 1" />
  </svg>
)
export const IconCards = (p: P) => (
  <svg {...base} {...p}>
    <path d="M8 3h8l4 4v12a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2Z" />
    <path d="M10 3v4M13 3v4M16 3v4" />
  </svg>
)
export const IconChevron = (p: P) => (
  <svg {...base} width={18} height={18} {...p}>
    <path d="m9 6 6 6-6 6" />
  </svg>
)
export const IconRefresh = (p: P) => (
  <svg {...base} width={18} height={18} {...p}>
    <path d="M20 12a8 8 0 1 1-2.3-5.6M20 4v4.5h-4.5" />
  </svg>
)
export const IconFolder = (p: P) => (
  <svg {...base} width={18} height={18} {...p}>
    <path d="M3.5 7.5a2 2 0 0 1 2-2h4l2 2h7a2 2 0 0 1 2 2v7a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2Z" />
  </svg>
)
