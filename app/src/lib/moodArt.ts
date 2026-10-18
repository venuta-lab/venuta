import { moodSlug } from './moodSlug'

/*
  Pictures for the catalog's mood and genre tiles, shipped with the app so they show offline:
  CC0 and public-domain photos (where each came from: assets/moods/SOURCES.md), in two tones of
  its tile's colour so they read as one set. A tile without one keeps its colour wash.
*/

const ART = import.meta.glob('../assets/moods/*.webp', { eager: true, query: '?url', import: 'default' }) as Record<string, string>

/** The picture for this tile, if Venuta has one. */
export function moodArt(title: string): string | undefined {
  return ART[`../assets/moods/${moodSlug(title)}.webp`]
}
