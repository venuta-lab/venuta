import { useEffect, useState } from 'react'
import { askAddSongs } from '../../components/AddToPlaylist'
import { ytmHistory, type YtmHistoryDay } from '../../lib/engine'
import { onlineProblem } from '../../lib/online'
import { enqueueItems, onlineEntryFrom, toRemoteItem } from '../../lib/player'
import { playWithRadio } from '../../lib/radio'
import { useYtmChoices } from '../../lib/ytmChoices'
import { useNav } from './model'
import { OnlineTrackHeader, OnlineTrackRow } from './OnlineResults'
import { YtmSignInAgain } from './YtmLibrary'

/*
  Stats, YouTube Music: the account's own history, by day, as YouTube keeps it. It holds what was
  played on the phone and the web, and what Venuta told it when Count plays is on (Settings,
  Online); your own files never reach it, their history stays in Venuta.
*/

export function YtmHistory() {
  const { root } = useNav()
  const [days, setDays] = useState<YtmHistoryDay[] | null>(null)
  const [problem, setProblem] = useState('')
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
      ) : days === null ? (
        <p className="py-6 text-[13px] text-ink/45">Reading your YouTube Music history…</p>
      ) : !days.length ? (
        <p className="py-6 text-[13px] text-ink/55">Nothing in your YouTube Music history yet.</p>
      ) : (
        days.map((d) => {
          return (
            <section key={d.title} className="mb-8">
              <h3 className="mb-2 text-[14px] font-semibold text-ink/70">{d.title}</h3>
              <OnlineTrackHeader />
              <div className="mt-1 grid gap-0.5">
                {d.tracks.map((t, i) => (
                  <OnlineTrackRow
                    key={`${t.track_id}:${i}`}
                    track={t}
                    onPlay={() => void playWithRadio(root, toRemoteItem(t))}
                    onQueue={(next) => void enqueueItems(root, [toRemoteItem(t)], next)}
                    onAdd={() => askAddSongs([{ online: onlineEntryFrom(t) }], t.title)}
                  />
                ))}
              </div>
            </section>
          )
        })
      )}
    </div>
  )
}
