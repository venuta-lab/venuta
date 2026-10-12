import { getCurrentWindow } from '@tauri-apps/api/window'
import { inApp } from '../lib/engine'

// Frameless window: this strip is the drag handle and holds the window controls.
export function TitleBar() {
  const win = inApp() ? getCurrentWindow() : null
  return (
    <div data-tauri-drag-region className="relative z-30 flex h-10 shrink-0 items-center pl-5">
      <span data-tauri-drag-region className="text-[13px] font-medium tracking-tight text-ink/70">
        Music Librarian
      </span>
      <div className="ml-auto flex h-full">
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
        props.danger ? 'hover:bg-[#e5484d]/80' : 'hover:bg-white/10'
      }`}
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round">
        {props.children}
      </svg>
    </button>
  )
}
