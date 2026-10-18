import { askAddSongs } from '../../components/AddToPlaylist'
import { IconPlay } from '../../components/Icons'
import { RemoteArt } from '../../components/RemoteArt'
import { onlineCollection, type MoodTile, type OnlineCollection } from '../../lib/engine'
import { notify } from '../../lib/notify'
import { onlineProblem } from '../../lib/online'
import { enqueueItems, onlineEntryFrom, playAllItems, toRemoteItem } from '../../lib/player'
import { menuFor } from '../../lib/actions'
import { menuFromEvent } from '../../lib/menu'
import { useNav } from './model'

/*
  Cards for the browse side of the online catalog: a playlist or podcast to open (or play from
  its round button), and a mood or genre tile. Same size, rounding and hover as album cards, so
  a shelf of them reads as one family with the rest of Music; colors come from the theme, with
  the catalog's own color for a mood only as a soft wash.
*/

/** A playlist's or podcast's songs, then `then` with them; a problem is said, not thrown. */
async function withCollection(c: OnlineCollection, then: (items: ReturnType<typeof toRemoteItem>[]) => void) {
  try {
    const page = await onlineCollection(c.collection_id)
    if (!page.tracks.length) {
      notify(c.kind === 'podcast' ? 'No episodes to play.' : 'This playlist has no songs to play.')
      return
    }
    then(page.tracks.map(toRemoteItem))
  } catch (e) {
    notify(onlineProblem(e))
  }
}

/** A playlist or podcast card: the card opens it; its round button plays it from the start. */
export function CollectionCard({ collection: c, width = 168 }: { collection: OnlineCollection; width?: number }) {
  const { root, openOnlineCollection } = useNav()
  const from = { kind: 'search' as const, ref: c.title, label: `Online: ${c.title}` }
  const play = () => void withCollection(c, (items) => void playAllItems(root, items, from))
  const menu = () =>
    menuFor(
      'album',
      { online: true },
      {
        open: () => openOnlineCollection(c),
        play,
        next: () => void withCollection(c, (items) => void enqueueItems(root, items, true)),
        queue: () => void withCollection(c, (items) => void enqueueItems(root, items, false)),
        playlist: () =>
          void onlineCollection(c.collection_id)
            .then((p) => p.tracks.length && askAddSongs(p.tracks.map((t) => ({ online: onlineEntryFrom(t) })), c.title))
            .catch((e) => notify(onlineProblem(e))),
      },
    )
  return (
    <div className="group relative shrink-0" style={{ width }}>
      <button onClick={() => openOnlineCollection(c)} onContextMenu={(e) => menuFromEvent(e, menu())} title={`Open “${c.title}”`} className="block w-full text-left">
        <span className="relative grid aspect-square w-full place-items-center overflow-hidden rounded-2xl bg-ink/[0.06] text-[28px] text-ink/25 shadow-lg shadow-black/20">
          <RemoteArt url={c.artwork_url} seed={c.title} size={width} imgClassName="h-full w-full object-cover" />
        </span>
        <span className="mt-2.5 block truncate text-[13.5px] font-medium">{c.title}</span>
        {c.subtitle && <span className="block truncate text-[12px] text-ink/55">{c.subtitle}</span>}
      </button>
      <button
        onClick={play}
        aria-label={`Play “${c.title}”`}
        title="Play"
        className="absolute right-2.5 grid h-10 w-10 place-items-center rounded-full bg-accent text-on-accent opacity-0 shadow-lg transition-opacity focus-visible:opacity-100 group-hover:opacity-100"
        style={{ top: width - 50 }}
      >
        <IconPlay width={17} height={17} />
      </button>
    </div>
  )
}

/** A mood or genre tile: its name over a soft wash of the catalog's color for it. */
export function MoodCard({ tile, onOpen }: { tile: MoodTile; onOpen: () => void }) {
  const color = /^#[0-9a-f]{6}$/i.test(tile.color) ? tile.color : 'var(--accent)'
  return (
    <button
      onClick={onOpen}
      className="group relative flex h-[76px] items-end overflow-hidden rounded-2xl bg-ink/[0.05] px-4 pb-3 text-left shadow-[inset_0_0_0_1px_var(--edge)] transition-[background-color,transform] duration-200 hover:bg-ink/[0.08] active:scale-[0.98]"
    >
      <span
        aria-hidden
        className="absolute inset-0 opacity-60 transition-opacity duration-200 group-hover:opacity-90"
        style={{ background: `radial-gradient(120% 140% at 100% 0%, color-mix(in oklab, ${color} 45%, transparent), transparent 70%)` }}
      />
      <span aria-hidden className="absolute bottom-0 left-0 top-0 w-1" style={{ background: color }} />
      <span className="relative text-[14px] font-semibold tracking-[-0.01em]">{tile.title}</span>
    </button>
  )
}
