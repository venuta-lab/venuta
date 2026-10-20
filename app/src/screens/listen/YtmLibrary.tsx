import { askAddSongs } from '../../components/AddToPlaylist'
import { deleteYtmUpload } from '../../lib/actions'
import { ytmSignIn, type OnlineSuggestion, type YtmTrack } from '../../lib/engine'
import { notify } from '../../lib/notify'
import { current, enqueueItems, onlineEntryFrom, playItems, queueKey, toRemoteItem, toggle, usePlayerValue, type From } from '../../lib/player'
import { useYtmPart, useYtmStatus, type YtmPart } from '../../lib/ytmLibrary'
import { useNav } from './model'
import { OnlineTrackHeader, OnlineTrackRow } from './OnlineResults'
import { useOnlineCards } from './useOnlineCards'

/*
  Your YouTube Music library inside Library, beside your own music: the account's albums, artists,
  podcasts and uploads as cards, its songs as rows, each part read the first time it shows. An
  upload can be deleted from its menu (asked first). When
  YouTube stops taking the sign-in, one line says so and offers to sign in again; your own music
  is never touched by it.
*/

/** The line when the sign-in ran out: everything local carries on. */
export function YtmSignInAgain() {
  const { expired } = useYtmStatus()
  if (!expired) return null
  return (
    <div role="status" className="mb-5 flex flex-wrap items-center gap-3 rounded-2xl bg-ink/[0.05] px-4 py-3 text-[13px] text-ink/70">
      <span className="min-w-0 flex-1">YouTube Music needs you to sign in again. Your own music and playlists are all here meanwhile.</span>
      <button onClick={() => void ytmSignIn().catch((e) => notify(String(e)))} className="btn btn-sm">
        Sign in again
      </button>
    </div>
  )
}

/** What a part says before and around its cards. */
function Waiting({ problem, loading, empty, count }: { problem: string; loading: boolean; empty: string; count: number }) {
  if (problem) return <p role="status" className="py-8 text-[13px] text-ink/55">{problem}</p>
  if (loading && !count) return <p className="py-8 text-[13px] text-ink/45">Reading your YouTube Music library…</p>
  if (!count) return <p className="py-8 text-[13px] text-ink/55">{empty}</p>
  return null
}

/** A part of the account's library as a grid of cards (albums, artists, podcasts, uploads). */
export function YtmGrid({ part, label, empty }: { part: YtmPart; label: string; empty: string }) {
  const { value, problem, loading } = useYtmPart<OnlineSuggestion[]>(part)
  const cards = useOnlineCards(value ?? [], label)
  return (
    <div className="pb-16">
      <YtmSignInAgain />
      <Waiting problem={problem} loading={loading} empty={empty} count={cards.length} />
      {cards.length > 0 && (
        <div className="grid grid-cols-[repeat(auto-fill,168px)] gap-x-5 gap-y-7">
          {cards.map((c) => (
            <div key={c.key}>{c.node}</div>
          ))}
        </div>
      )}
    </div>
  )
}

const keyOf = (t: YtmTrack) => `online:${t.provider}:${t.track_id}`

/** The account's songs (saved, or uploaded) as rows: click plays from there. */
export function YtmSongs({ part, label, empty }: { part: 'songs' | 'upload_songs'; label: string; empty: string }) {
  const { root } = useNav()
  const { value, problem, loading } = useYtmPart<YtmTrack[]>(part)
  const tracks = value ?? []
  const items = tracks.map(toRemoteItem)
  const from: From = { kind: 'search', ref: label, label }
  const nowKey = usePlayerValue((st) => {
    const c = current(st)
    return c ? queueKey(c) : ''
  })
  return (
    <div className="pb-16">
      <YtmSignInAgain />
      <Waiting problem={problem} loading={loading} empty={empty} count={tracks.length} />
      {tracks.length > 0 && (
        <>
          <p className="mb-3 text-[12.5px] text-ink/55">
            {tracks.length.toLocaleString('en-IN')} {tracks.length === 1 ? 'song' : 'songs'}
            {part === 'upload_songs' ? '. Only you can play your uploads.' : ''}
          </p>
          <OnlineTrackHeader />
          <div className="mt-1 grid gap-0.5">
            {tracks.map((t, i) => (
              <OnlineTrackRow
                key={`${keyOf(t)}:${i}`}
                track={t}
                now={keyOf(t) === nowKey}
                onPlay={() => (keyOf(t) === nowKey ? void toggle() : void playItems(root, items, i, from))}
                onQueue={(next) => void enqueueItems(root, [toRemoteItem(t)], next)}
                onAdd={() => askAddSongs([{ online: onlineEntryFrom(t) }], t.title)}
                facts={{ upload: !!t.delete_id }}
                work={{ deleteUpload: t.delete_id ? () => void deleteYtmUpload(t.delete_id ?? '', t.title, 'song') : undefined }}
              />
            ))}
          </div>
        </>
      )}
    </div>
  )
}
