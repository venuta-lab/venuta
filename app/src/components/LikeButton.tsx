import { toggleLiked, useLiked } from '../lib/likes'
import { fromRoomHost, type QueueItem } from '../lib/player'
import { IconHeart } from './Icons'

/**
 * The heart on a song: filled in the accent when liked. `quiet`: shown on hover only until
 * liked (rows), so a list of songs doesn't read as a list of hearts.
 */
export function LikeButton({ root, item, quiet = false, size = 18 }: { root: string; item: QueueItem; quiet?: boolean; size?: number }) {
  const liked = useLiked(item)
  // A friend's file streamed in a room: nothing of yours to like.
  if (fromRoomHost(item)) return null
  return (
    <button
      onClick={(e) => {
        e.stopPropagation()
        void toggleLiked(root, item)
      }}
      onDoubleClick={(e) => e.stopPropagation()}
      aria-pressed={liked}
      aria-label={liked ? `Remove “${item.title}” from Liked songs` : `Add “${item.title}” to Liked songs`}
      title={liked ? 'Remove from Liked songs' : 'Add to Liked songs'}
      className={`grid h-8 w-8 shrink-0 place-items-center rounded-full transition-[opacity,color,transform] duration-150 hover:bg-ink/[0.1] active:scale-90 ${
        liked ? 'text-accent-ink' : 'text-ink/60 hover:text-ink'
      } ${quiet && !liked ? 'opacity-0 focus-visible:opacity-100 group-hover:opacity-100' : ''}`}
    >
      <IconHeart filled={liked} width={size} height={size} />
    </button>
  )
}
