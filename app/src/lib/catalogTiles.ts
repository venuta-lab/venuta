import { useEffect, useState } from 'react'
import { onlineMoods, type MoodTile } from './engine'

let catalogTiles: MoodTile[] | null = null
/** The catalog's mood and genre tiles (asked once, then kept for this visit). */
export function useCatalogTiles(): MoodTile[] {
  const [tiles, setTiles] = useState<MoodTile[]>(catalogTiles ?? [])
  useEffect(() => {
    if (catalogTiles) return
    let live = true
    onlineMoods()
      .then((s) => {
        catalogTiles = s.flatMap((x) => x.tiles)
        if (live) setTiles(catalogTiles)
      })
      .catch(() => {})
    return () => {
      live = false
    }
  }, [])
  return tiles
}
