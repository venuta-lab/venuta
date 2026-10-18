import { Switch } from '../../components/ui'
import type { LyricWindow } from '../../lib/lyricLook'
import { closeLyricWindow, openLyricWindow, setLyricWindow, useBackgrounds, useLyricWindow, useLyricWindowShown, type LyricBackground } from '../../lib/lyricWindows'

/*
  Settings > Listening > Lyrics windows: the big screen and the desktop lyrics, each with its own
  background, place and lock (the same as their own toolbars, for when nothing is playing).
*/

const WINDOWS: { win: LyricWindow; name: string; line: string }[] = [
  { win: 'stage', name: 'Big screen', line: 'Lyrics on a whole screen or a window: for a TV, a second monitor, a room.' },
  { win: 'overlay', name: 'Desktop lyrics', line: 'The lines over every window. Locked, clicks go through them.' },
]

function Row(props: { title: string; hint?: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-wrap items-center gap-4 border-b border-edge py-3 last:border-b-0">
      <div className="min-w-[200px] flex-1">
        <div className="text-[13.5px]">{props.title}</div>
        {props.hint && <div className="mt-0.5 text-[12px] leading-relaxed text-ink/55">{props.hint}</div>}
      </div>
      {props.children}
    </div>
  )
}

function One({ win, name, line }: { win: LyricWindow; name: string; line: string }) {
  const s = useLyricWindow(win)
  const shown = useLyricWindowShown(win)
  const all = useBackgrounds()
  return (
    <div className="mt-4 rounded-[18px] border border-edge px-4">
      <div className="flex flex-wrap items-center gap-3 border-b border-edge py-3">
        <div className="min-w-[200px] flex-1">
          <div className="text-[14px] font-medium">{name}</div>
          <div className="mt-0.5 text-[12.5px] leading-relaxed text-ink/55">{line}</div>
        </div>
        <button onClick={() => void (shown ? closeLyricWindow(win) : openLyricWindow(win))} className="btn btn-sm">
          {shown ? 'Hide' : 'Show'}
        </button>
      </div>
      <Row title="Background" hint={all.find((b) => b.id === s.background)?.line}>
        <select value={s.background} onChange={(e) => void setLyricWindow(win, { background: e.target.value as LyricBackground })} aria-label={`${name} background`} className="field h-8 rounded-full px-3 text-[13px]">
          {all.map((b) => (
            <option key={b.id} value={b.id}>
              {b.name}
            </option>
          ))}
        </select>
      </Row>
      <Row title="Fill the screen" hint="Off: a window of its own, moved and sized where you leave it.">
        <Switch on={s.full} onChange={(on) => void setLyricWindow(win, { full: on })} label={`${name}: fill the screen`} />
      </Row>
      <Row title="Over every window">
        <Switch on={s.onTop} onChange={(on) => void setLyricWindow(win, { onTop: on })} label={`${name}: over every window`} />
      </Row>
      <Row title="Locked" hint="Clicks go through it to what is under it. Point at it to find its lock and open it.">
        <Switch on={s.locked} onChange={(on) => void setLyricWindow(win, { locked: on })} label={`${name}: locked`} />
      </Row>
    </div>
  )
}

export function LyricWindowsSettings() {
  return (
    <>
      <h3 className="mt-8 text-[13px] font-semibold text-ink/60">Lyrics windows</h3>
      {WINDOWS.map((w) => (
        <One key={w.win} {...w} />
      ))}
    </>
  )
}
