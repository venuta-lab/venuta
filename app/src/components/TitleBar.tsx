import { invoke } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { useEffect, useState } from 'react'
import { inApp } from '../lib/engine'
import { ActivityIndicator } from './Activity'
import { Brand } from './Brand'
import { askOpenLink } from '../lib/openLink'
import { IconSearch } from './Icons'
import { RoomChip } from './together/RoomChip'
import { YtmProfileButton } from './YtmProfile'

/** Where a screen may put its own search in the title bar, in place of Jump to. */
export const TITLE_SEARCH = 'title-search'

// Frameless window: this strip is the drag handle and holds the window controls.
export function TitleBar(props: { onSearch: () => void; onActivity: () => void }) {
  const win = inApp() ? getCurrentWindow() : null
  // A copy for testing, beside the one in use: never mistaken for it.
  const [test, setTest] = useState(false)
  useEffect(() => {
    if (inApp()) invoke<boolean>('is_test_copy').then(setTest, () => {})
  }, [])
  // Three parts: the two sides grow alike, so the search sits in the middle; when the window
  // narrows, the search shrinks first and never covers what is beside it.
  return (
    <div data-tauri-drag-region className="@container/title relative z-30 flex h-10 shrink-0 items-center">
      <div data-tauri-drag-region className="flex min-w-max flex-1 basis-0 items-center">
        <span data-tauri-drag-region className="pointer-events-none ml-5 flex items-center">
          <Brand size={29} word={17} />
        </span>
        {test && (
          <span data-tauri-drag-region className="ml-2 rounded-full bg-[#e5484d] px-2 py-0.5 text-[11px] font-semibold text-white">
            Test copy
          </span>
        )}
      </div>
      {/* The music screen puts its search here (ListenScreen, through TITLE_SEARCH). Elsewhere the same
          box stands in for it and opens Search; Ctrl+K is the command palette everywhere. */}
      <div className="mx-3 min-w-0 shrink grow-0 basis-[min(420px,36vw)] [&:has(>#title-search>*)>.jump]:hidden">
        <div id={TITLE_SEARCH} />
        <button
          onClick={props.onSearch}
          title="Search your music and the online catalog (/). Ctrl+K: go anywhere or run a command"
          className="jump flex w-full items-center gap-2 rounded-full bg-ink/[0.06] px-3.5 py-1 text-[12.5px] text-ink/50 shadow-[inset_0_0_0_1px_var(--edge)] transition-colors hover:bg-ink/[0.1] hover:text-ink/70"
        >
          <IconSearch width={14} height={14} />
          <span className="flex-1 truncate text-left">Search songs, albums, artists, playlists</span>
          <kbd>/</kbd>
        </button>
      </div>
      <div data-tauri-drag-region className="flex h-full min-w-max flex-1 basis-0 items-center justify-end">
        <RoomChip />
        <button
          onClick={askOpenLink}
          aria-label="Open a link"
          title="Open a link: a song, album, artist or playlist on YouTube Music or Spotify (Ctrl+O)"
          className="mr-1 grid h-8 w-8 place-items-center rounded-full text-ink/60 transition-colors hover:bg-ink/10 hover:text-ink"
        >
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
            <path d="M10 14a4.5 4.5 0 0 0 6.4 0l3-3a4.5 4.5 0 0 0-6.4-6.4l-1 1" />
            <path d="M14 10a4.5 4.5 0 0 0-6.4 0l-3 3a4.5 4.5 0 0 0 6.4 6.4l1-1" />
          </svg>
        </button>
        <ActivityIndicator onAll={props.onActivity} />
        <YtmProfileButton />
        <WinButton label="Minimise" onClick={() => win?.minimize()}>
          <path d="M5 12h14" />
        </WinButton>
        <WinButton label="Maximise" onClick={() => win?.toggleMaximize()}>
          <rect x="6" y="6" width="12" height="12" rx="2" />
        </WinButton>
        <WinButton label="Close" danger onClick={() => win?.close()}>
          <path d="m7 7 10 10M17 7 7 17" />
        </WinButton>
      </div>
    </div>
  )
}

function WinButton(props: {
  label: string
  danger?: boolean
  onClick: () => void
  children: React.ReactNode
}) {
  return (
    <button
      aria-label={props.label}
      onClick={props.onClick}
      className={`grid h-full w-12 place-items-center text-ink/70 transition-colors hover:text-ink ${
        props.danger ? 'hover:bg-[#e5484d]/80' : 'hover:bg-ink/10'
      }`}
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round">
        {props.children}
      </svg>
    </button>
  )
}
