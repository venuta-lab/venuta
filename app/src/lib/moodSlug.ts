/*
  A mood or genre tile's picture is found by its name (lib/moodArt.ts): "R&B & soul" is
  assets/moods/r-and-b-and-soul.webp. Pure, so tests/moodSlug.test.ts reads it.
*/

/** The file name a tile's picture has, from its title. */
export function moodSlug(title: string): string {
  return title
    .normalize('NFKD')
    .replace(/[̀-ͯ]/g, '')
    .toLowerCase()
    .replace(/&/g, ' and ')
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
}
