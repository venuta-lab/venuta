import { useState } from 'react'
import type { OnlineArtistPage } from '../../lib/engine'
import type { From } from '../../lib/player'
import { artistShelves } from './artistSongs'
import { useNav } from './model'
import { OnlineShelfRow } from './OnlineCards'
import type { MixedSong } from './onlineSearch'
import { MixedSongs } from './SearchAll'

/*
  The catalog's part of an artist's page: their top songs (yours marked and played from your
  file), then every row of their page in its order, each with its See all. Their online page
  shows it under the banner; their page in your library shows it under your albums.
*/

/** Top songs shown before "See all". */
const TOP_SHOWN = 5

export function OnlineArtistSections(props: { page: OnlineArtistPage; name: string; songs: MixedSong[]; from: From }) {
  const nav = useNav()
  const [allSongs, setAllSongs] = useState(false)
  const shelves = artistShelves(props.page)
  const { songs, name } = props
  return (
    <>
      {songs.length > 0 && (
        <div className="mt-10">
          <MixedSongs
            query={name}
            from={props.from}
            title="Top songs"
            songs={allSongs ? songs : songs.slice(0, TOP_SHOWN)}
            seeAllLabel={allSongs ? 'Show fewer' : `See all ${songs.length}`}
            onSeeAll={songs.length > TOP_SHOWN ? () => setAllSongs((a) => !a) : undefined}
          />
        </div>
      )}
      {shelves.map((s) => (
        <OnlineShelfRow key={s.title} shelf={s} label={name} onSeeAll={s.more ? () => s.more && nav.openOnlineShelf(s.title, name, s.more) : undefined} />
      ))}
    </>
  )
}
