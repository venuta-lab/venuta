import { invoke } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { useEffect, useState } from 'react'
import { inApp } from '../lib/engine'
import { ActivityIndicator } from './Activity'
import { Brand } from './Brand'
import { askOpenLink } from '../lib/openLink'
import { IconSearch } from './Icons'

// Frameless window: this strip is the drag handle and holds the window controls.
export function TitleBar(props: { onPalette: () => void }) {
  const win = inApp() ? getCurrentWindow() : null
  // A copy for testing, beside the one in use: never mistaken for it.
  const [test, setTest] = useState(false)
  useEffect(() => {
    if (inApp()) invoke<boolean>('is_test_copy').then(setTest, () => {})
  }, [])
  return (
    <div data-tauri-drag-region className="relative z-30 flex h-10 shrink-0 items-center pl-5">
      <span data-tauri-drag-region className="pointer-events-none flex items-center">
        <Brand size={29} word={17} />
      </span>
      {test && (
        <span data-tauri-drag-region className="ml-2 rounded-full bg-[#e5484d] px-2 py-0.5 text-[11px] font-semibold text-white">
          Test copy
        </span>
      )}
      <button
        onClick={props.onPalette}
        title="Go to a place, an album or an artist, or run a command"
        className="absolute left-1/2 top-1/2 flex w-[min(320px,32vw)] -translate-x-1/2 -translate-y-1/2 items-center gap-2 rounded-full bg-ink/[0.06] px-3.5 py-1 text-[12.5px] text-ink/50 shadow-[inset_0_0_0_1px_var(--edge)] transition-colors hover:bg-ink/[0.1] hover:text-ink/70"
      >
        <IconSearch width={14} height={14} />
        <span className="flex-1 truncate text-left">Jump to…</span>
        <kbd>Ctrl K</kbd>
      </button>
      <div className="ml-auto flex h-full items-center">
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
        <ActivityIndicator />
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
