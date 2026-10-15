/*
  A playlist's name is its file's name, so some letters can't be in it. This is the engine's own
  rule (crates/engine/src/plan.rs, sanitize with 100), kept the same by a shared table of cases
  (app/tests/fixtures/playlist-names.json) that both test: < > : " / \ | ? * and control
  characters become spaces, runs of spaces become one, it is at most 100 letters, dots and
  spaces at the end go, and the names Windows keeps for itself get a _ after them.

  Pure, so tests/playlistName.test.ts reads it as it is.
*/

const MAX = 100
const NOT_IN_NAMES = /[<>:"/\\|?*\p{Cc}]/gu
const RESERVED = new Set(['CON', 'PRN', 'AUX', 'NUL', 'COM1', 'COM2', 'COM3', 'LPT1', 'LPT2', 'LPT3'])

/** The name a typed name is saved as. */
export function savedName(typed: string): string {
  let out = typed
    .normalize('NFC')
    .replace(NOT_IN_NAMES, ' ')
    .split(/\p{White_Space}+/u)
    .filter(Boolean)
    .join(' ')
  const letters = [...out]
  if (letters.length > MAX) out = letters.slice(0, MAX).join('').replace(/\p{White_Space}+$/u, '')
  out = out.replace(/[. ]+$/, '')
  return RESERVED.has(out.toUpperCase()) ? `${out}_` : out
}

/**
 * What to say under the name field while it is typed, when the name saved won't be the name
 * typed (null when it will be).
 */
export function nameNote(typed: string): string | null {
  const t = typed.trim()
  if (!t) return null
  const saved = savedName(typed)
  if (saved === t) return null
  if (!saved) return 'A name needs at least one letter or number.'
  const bad = [...new Set(t.match(NOT_IN_NAMES) ?? [])].filter((c) => c.trim())
  const why: string[] = []
  if (bad.length) why.push(`${bad.join(' ')} can't be in a name`)
  if ([...t.normalize('NFC')].length > MAX) why.push(`a name is at most ${MAX} letters`)
  if (RESERVED.has(saved.slice(0, -1).toUpperCase()) && saved.endsWith('_')) why.push('Windows keeps that name for itself')
  return `Saved as “${saved}”${why.length ? `: ${why.join('; ')}.` : '.'}`
}
