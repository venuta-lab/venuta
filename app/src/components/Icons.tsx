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
export const IconMusic = (p: P) => (
  <svg {...base} {...p}>
    <rect x="3.5" y="3.5" width="17" height="17" rx="4" />
    <circle cx="12" cy="12" r="4.2" />
    <circle cx="12" cy="12" r="0.8" fill="currentColor" />
  </svg>
)
/** A note: listening. */
export const IconListen = (p: P) => (
  <svg {...base} {...p}>
    <path d="M9 18.5V6l11-2.5v12.5" />
    <circle cx="6.5" cy="18.5" r="2.5" />
    <circle cx="17.5" cy="16" r="2.5" />
  </svg>
)
/** A tag: the details inside your files. */
export const IconTag = (p: P) => (
  <svg {...base} {...p}>
    <path d="M3.5 12.6V4.8c0-.7.6-1.3 1.3-1.3h7.8c.4 0 .7.1.9.4l7 7c.5.5.5 1.3 0 1.8l-7.8 7.8c-.5.5-1.3.5-1.8 0l-7-7a1.3 1.3 0 0 1-.4-.9Z" />
    <circle cx="8.5" cy="8.5" r="1.4" />
  </svg>
)
export const IconInbox = (p: P) => (
  <svg {...base} {...p}>
    <path d="M4 13.5 6.2 5.6A2 2 0 0 1 8.1 4h7.8a2 2 0 0 1 1.9 1.6L20 13.5" />
    <path d="M4 13.5V18a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-4.5h-4.5a3.5 3.5 0 0 1-7 0Z" />
  </svg>
)
export const IconActivity = (p: P) => (
  <svg {...base} {...p}>
    <circle cx="12" cy="12" r="8.5" />
    <path d="M12 7.5V12l3 2" />
  </svg>
)
export const IconSettings = (p: P) => (
  <svg {...base} {...p}>
    <path d="M4 7h9M17 7h3M4 17h3M11 17h9" />
    <circle cx="15" cy="7" r="2" />
    <circle cx="9" cy="17" r="2" />
  </svg>
)
export const IconSearch = (p: P) => (
  <svg {...base} width={18} height={18} {...p}>
    <circle cx="11" cy="11" r="6.5" />
    <path d="m16 16 4 4" />
  </svg>
)
export const IconSun = (p: P) => (
  <svg {...base} width={18} height={18} {...p}>
    <circle cx="12" cy="12" r="4" />
    <path d="M12 2.5v2M12 19.5v2M2.5 12h2M19.5 12h2M5.3 5.3l1.4 1.4M17.3 17.3l1.4 1.4M5.3 18.7l1.4-1.4M17.3 6.7l1.4-1.4" />
  </svg>
)
/** Play: a rounded triangle, filled so it reads at small sizes. */
export const IconPlay = (p: P) => (
  <svg {...base} width={18} height={18} {...p}>
    {/* Optically centred: the triangle's box sits 1px right of the middle, its weight on it. */}
    <path d="M7 5.6v12.8c0 .8.9 1.3 1.6.9l10-6.4a1 1 0 0 0 0-1.8l-10-6.4c-.7-.4-1.6.1-1.6.9Z" fill="currentColor" />
  </svg>
)
export const IconPause = (p: P) => (
  <svg {...base} width={18} height={18} {...p}>
    <rect x="6.5" y="5" width="3.6" height="14" rx="1.2" fill="currentColor" />
    <rect x="13.9" y="5" width="3.6" height="14" rx="1.2" fill="currentColor" />
  </svg>
)
export const IconShuffle = (p: P) => (
  <svg {...base} width={18} height={18} {...p}>
    <path d="M4 7h2.5c2 0 3.2 1 4.3 2.7l2.4 4.6c1 1.7 2.3 2.7 4.3 2.7H20" />
    <path d="M4 17h2.5c1.4 0 2.4-.5 3.2-1.3M13.3 8.3C14.1 7.5 15.1 7 16.5 7H20" />
    <path d="m17.5 4.5 2.5 2.5-2.5 2.5M17.5 14.5l2.5 2.5-2.5 2.5" />
  </svg>
)
export const IconBack = (p: P) => (
  <svg {...base} width={18} height={18} {...p}>
    <path d="m15 6-6 6 6 6" />
  </svg>
)
export const IconMore = (p: P) => (
  <svg {...base} width={18} height={18} {...p}>
    <circle cx="5.5" cy="12" r="1.3" fill="currentColor" stroke="none" />
    <circle cx="12" cy="12" r="1.3" fill="currentColor" stroke="none" />
    <circle cx="18.5" cy="12" r="1.3" fill="currentColor" stroke="none" />
  </svg>
)
export const IconPrevious = (p: P) => (
  <svg {...base} width={18} height={18} {...p}>
    <rect x="5" y="5.5" width="2.6" height="13" rx="1" fill="currentColor" stroke="none" />
    <path d="M19 6.6v10.8c0 .8-.9 1.3-1.6.8L9.7 13a1.2 1.2 0 0 1 0-2l7.7-5.2c.7-.5 1.6 0 1.6.8Z" fill="currentColor" stroke="none" />
  </svg>
)
export const IconNext = (p: P) => (
  <svg {...base} width={18} height={18} {...p}>
    <rect x="16.4" y="5.5" width="2.6" height="13" rx="1" fill="currentColor" stroke="none" />
    <path d="M5 6.6v10.8c0 .8.9 1.3 1.6.8l7.7-5.2a1.2 1.2 0 0 0 0-2L6.6 5.8C5.9 5.3 5 5.8 5 6.6Z" fill="currentColor" stroke="none" />
  </svg>
)
export const IconRepeat = (p: P) => (
  <svg {...base} width={18} height={18} {...p}>
    <path d="M4.5 11V9.5A3.5 3.5 0 0 1 8 6h11.5M16.5 3l3 3-3 3" />
    <path d="M19.5 13v1.5A3.5 3.5 0 0 1 16 18H4.5M7.5 21l-3-3 3-3" />
  </svg>
)
export const IconRepeatOne = (p: P) => (
  <svg {...base} width={18} height={18} {...p}>
    <path d="M4.5 11V9.5A3.5 3.5 0 0 1 8 6h11.5M16.5 3l3 3-3 3" />
    <path d="M19.5 13v1.5A3.5 3.5 0 0 1 16 18H4.5M7.5 21l-3-3 3-3" />
    <path d="M11.2 10.6 12.4 10v5" strokeWidth={1.5} />
  </svg>
)
export const IconQueue = (p: P) => (
  <svg {...base} width={18} height={18} {...p}>
    <path d="M4 6.5h11M4 11.5h11M4 16.5h6.5" />
    <path d="M15.5 14.5v5.2l4-2.6-4-2.6Z" fill="currentColor" stroke="none" />
  </svg>
)
export const IconLyrics = (p: P) => (
  <svg {...base} width={18} height={18} {...p}>
    <path d="M5 5.5h14a1.5 1.5 0 0 1 1.5 1.5v8a1.5 1.5 0 0 1-1.5 1.5h-6.5L8 20v-3.5H5A1.5 1.5 0 0 1 3.5 15V7A1.5 1.5 0 0 1 5 5.5Z" />
    <path d="M7.5 9.5h9M7.5 12.5h6" />
  </svg>
)
export const IconVolume = (p: P) => (
  <svg {...base} width={18} height={18} {...p}>
    <path d="M4 9.5v5h3.5l4.5 4V5.5l-4.5 4H4Z" fill="currentColor" strokeWidth={1.2} />
    <path d="M15.5 9a4 4 0 0 1 0 6M18 6.5a7.5 7.5 0 0 1 0 11" />
  </svg>
)
export const IconMute = (p: P) => (
  <svg {...base} width={18} height={18} {...p}>
    <path d="M4 9.5v5h3.5l4.5 4V5.5l-4.5 4H4Z" fill="currentColor" strokeWidth={1.2} />
    <path d="m16 9.5 5 5M21 9.5l-5 5" />
  </svg>
)
export const IconExpand = (p: P) => (
  <svg {...base} width={18} height={18} {...p}>
    <path d="M14.5 4.5h5v5M9.5 19.5h-5v-5M19.5 4.5 13.5 10.5M4.5 19.5l6-6" />
  </svg>
)
export const IconClose = (p: P) => (
  <svg {...base} width={18} height={18} {...p}>
    <path d="m6.5 6.5 11 11M17.5 6.5l-11 11" />
  </svg>
)
/** A small window in the corner of a big one: the mini player. */
export const IconMini = (p: P) => (
  <svg {...base} width={18} height={18} {...p}>
    <rect x="3.5" y="5" width="17" height="14" rx="2.5" />
    <rect x="11.5" y="11.5" width="7" height="5.5" rx="1.2" fill="currentColor" stroke="none" />
  </svg>
)
// Lyrics, elsewhere: a screen for the room, the lines over the desktop, their style, their timing.
export const IconBigScreen = (p: P) => (
  <svg {...base} {...p}>
    <rect x="2.5" y="4" width="19" height="13" rx="2.5" />
    <path d="M8 21h8M12 17v4M6.5 10V8h2.5M17.5 11v2H15" />
  </svg>
)
export const IconDesktopLyrics = (p: P) => (
  <svg {...base} {...p}>
    <rect x="2.5" y="3.5" width="19" height="15" rx="2.5" />
    <rect x="5.5" y="12" width="13" height="3.5" rx="1.75" />
    <path d="M9 21.5h6" />
  </svg>
)
export const IconLyricStyle = (p: P) => (
  <svg {...base} {...p}>
    <path d="M3 19 8 5l5 14M4.8 14h6.4" />
    <path d="M20.5 19v-4.2a3 3 0 0 0-5.2-2" />
    <circle cx="17.5" cy="16.5" r="2.6" />
  </svg>
)
export const IconTiming = (p: P) => (
  <svg {...base} {...p}>
    <circle cx="12" cy="13.5" r="7.5" />
    <path d="M12 9.5v4l2.5 2M10 2.5h4M18.5 6l1.5-1.5" />
  </svg>
)
export const IconAlbum = (p: P) => (
  <svg {...base} {...p}>
    <circle cx="12" cy="12" r="8.5" />
    <circle cx="12" cy="12" r="2.5" />
  </svg>
)
export const IconArtist = (p: P) => (
  <svg {...base} {...p}>
    <circle cx="12" cy="8" r="4" />
    <path d="M4.5 20.5a7.5 7.5 0 0 1 15 0" />
  </svg>
)
export const IconLock = (p: P) => (
  <svg {...base} {...p}>
    <rect x="5" y="10.5" width="14" height="10" rx="2.5" />
    <path d="M8.5 10.5V7.5a3.5 3.5 0 0 1 7 0v3" />
  </svg>
)
export const IconWeb = (p: P) => (
  <svg {...base} {...p}>
    <circle cx="12" cy="12" r="8.5" />
    <path d="M3.5 12h17M12 3.5c2.5 2.6 3.5 5.5 3.5 8.5s-1 5.9-3.5 8.5c-2.5-2.6-3.5-5.5-3.5-8.5s1-5.9 3.5-8.5Z" />
  </svg>
)
export const IconPlaylistAdd = (p: P) => (
  <svg {...base} {...p}>
    <path d="M4 6h11M4 12h11M4 18h7M18 14v7M14.5 17.5h7" />
  </svg>
)
