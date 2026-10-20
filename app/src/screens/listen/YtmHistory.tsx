import { useEffect, useState } from 'react'
import { askAddSongs } from '../../components/AddToPlaylist'
import { IconClose } from '../../components/Icons'
import { ytmHistory, ytmHistoryRemove, type YtmHistoryDay, type YtmHistoryPlay } from '../../lib/engine'
import { notify } from '../../lib/notify'
import { onlineProblem } from '../../lib/online'
import { enqueueItems, onlineEntryFrom, toRemoteItem } from '../../lib/player'
import { playWithRadio } from '../../lib/radio'
import { useYtmChoices } from '../../lib/ytmChoices'
import { visibleDays } from '../../lib/ytmHistory'
import { useNav } from './model'
import { OnlineTrackHeader, OnlineTrackRow } from './OnlineResults'
import { YtmSignInAgain } from './YtmLibrary'

/*
  Stats, YouTube Music: the account's own history, by day, as YouTube keeps it. It holds what was
  played on the phone and the web, and what Venuta told it when Count plays is on (Settings,
  Online); your own files never reach it, their history stays in Venuta.

  A play can be taken out with its ×: it leaves the list at once, and comes back with a word if
  YouTube Music doesn't take it out.
*/

export function YtmHistory() {
  const { root } = useNav()
  const [days, setDays] = useState<YtmHistoryDay[] | null>(null)
  const [problem, setProblem] = useState('')
  // Plays taken out (by key), shown gone while YouTube Music is told.
  const [gone, setGone] = useState<ReadonlySet<string>>(new Set())
  const counting = !!useYtmChoices()?.choices.count_plays
  useEffect(() => {
    let live = true
    ytmHistory()
      .then((d) => live && setDays(d))
      .catch((e) => {
        if (!live) return
        setProblem(onlineProblem(e))
        setDays([])
      })
    return () => {
      live = false
    }
  }, [])
  const remove = async (key: string, t: YtmHistoryPlay) => {
    if (!t.remove_token) return
    setGone((g) => new Set(g).add(key))
    try {
      await ytmHistoryRemove(t.remove_token)
      notify(`Took “${t.title}” out of your YouTube Music history.`)
    } catch (e) {
      setGone((g) => {
        const back = new Set(g)
        back.delete(key)
        return back
      })
      notify(onlineProblem(e))
    }
  }
  const shown = days ? visibleDays(days, gone) : null
  return (
    <div className="mt-6">
      <YtmSignInAgain />
      <p className="mb-5 max-w-[70ch] text-[12.5px] leading-relaxed text-ink/55">
        {counting
          ? 'What your YouTube Music account has played, here and everywhere else. Venuta tells it the online songs you play; your own files stay in Venuta.'
          : 'What your YouTube Music account has played on your phone and the web. To add what you play here, switch on Count plays in Settings, Online.'}
      </p>
      {problem ? (
        <p role="status" className="py-6 text-[13px] text-ink/55">
          {problem}
        </p>
      ) : shown === null ? (
        <p className="py-6 text-[13px] text-ink/45">Reading your YouTube Music history…</p>
      ) : !shown.length ? (
        <p className="py-6 text-[13px] text-ink/55">Nothing in your YouTube Music history yet.</p>
      ) : (
        shown.map((d) => {
          return (
            <section key={d.title} className="mb-8">
              <h3 className="mb-2 text-[14px] font-semibold text-ink/70">{d.title}</h3>
              <OnlineTrackHeader />
              <div className="mt-1 grid gap-0.5">
                {d.plays.map(({ key, track: t }) => (
                  <div key={key} className="group flex items-center gap-1">
                    <div className="min-w-0 flex-1">
                      <OnlineTrackRow
                        track={t}
                        onPlay={() => void playWithRadio(root, toRemoteItem(t))}
                        onQueue={(next) => void enqueueItems(root, [toRemoteItem(t)], next)}
                        onAdd={() => askAddSongs([{ online: onlineEntryFrom(t) }], t.title)}
                      />
                    </div>
                    <button
                      aria-label={`Remove ${t.title} from your history`}
                      title="Remove from history"
                      onClick={() => void remove(key, t)}
                      disabled={!t.remove_token}
                      className="grid h-7 w-7 shrink-0 place-items-center rounded-full text-ink/45 opacity-0 hover:bg-ink/10 hover:text-ink focus:opacity-100 group-hover:opacity-100 disabled:invisible"
                    >
                      <IconClose width={14} height={14} />
                    </button>
                  </div>
                ))}
              </div>
            </section>
          )
        })
      )}
    </div>
  )
}
