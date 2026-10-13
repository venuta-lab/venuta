import { getCurrentWindow } from '@tauri-apps/api/window'
import { inApp } from '../lib/engine'
import { ActivityIndicator } from './Activity'
import { IconSearch } from './Icons'

// Frameless window: this strip is the drag handle and holds the window controls.
export function TitleBar(props: { onPalette: () => void }) {
  const win = inApp() ? getCurrentWindow() : null
  return (
    <div data-tauri-drag-region className="relative z-30 flex h-10 shrink-0 items-center pl-5">
      <span data-tauri-drag-region className="text-[13px] font-medium tracking-tight text-ink/70">
        Venuta
      </span>
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
