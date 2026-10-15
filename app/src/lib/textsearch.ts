// Search across scripts. Names in Indian scripts are read in Roman letters by rule (no
// dictionary, nothing online), and both what is searched and what is typed are folded to one
// loose spelling, so "kesariya", "kesaria" and "केसरिया" all find the same song.

// --- Indian scripts to Roman letters -------------------------------------------------------

// The Indian scripts' Unicode blocks are laid out alike, 0x80 apart from Devanagari's, so a
// letter in any of them is read as the Devanagari letter in the same place.
const INDIC_FIRST = 0x0900
const INDIC_LAST = 0x0d7f

const CONSONANTS: Record<string, string> = {
  क: 'k', ख: 'kh', ग: 'g', घ: 'gh', ङ: 'n',
  च: 'ch', छ: 'chh', ज: 'j', झ: 'jh', ञ: 'ny',
  ट: 't', ठ: 'th', ड: 'd', ढ: 'dh', ण: 'n',
  त: 't', थ: 'th', द: 'd', ध: 'dh', न: 'n', ऩ: 'n',
  प: 'p', फ: 'ph', ब: 'b', भ: 'bh', म: 'm',
  य: 'y', र: 'r', ऱ: 'r', ल: 'l', ळ: 'l', ऴ: 'l', व: 'v',
  श: 'sh', ष: 'sh', स: 's', ह: 'h',
  // With a nukta, written as one letter.
  क़: 'q', ख़: 'kh', ग़: 'gh', ज़: 'z', ड़: 'd', ढ़: 'dh', फ़: 'f', य़: 'y',
}

// A nukta (़) after a letter changes it the same way.
const NUKTA: Record<string, string> = { k: 'q', kh: 'kh', g: 'gh', j: 'z', d: 'd', dh: 'dh', ph: 'f', y: 'y' }

// Vowel signs, and the vowel letters that stand alone. Long ī and ū are "ee" and "oo" inside a
// word and "i" and "u" at its end (teri, tu), ā is "a" at the end (tera, kesariya).
const SIGNS: Record<string, string> = {
  'ा': 'aa', 'ि': 'i', 'ी': 'ee', 'ु': 'u', 'ू': 'oo', 'ृ': 'ri', 'ॄ': 'ri',
  'ॅ': 'e', 'ॆ': 'e', 'े': 'e', 'ै': 'ai', 'ॉ': 'o', 'ॊ': 'o', 'ो': 'o', 'ौ': 'au',
}
const VOWELS: Record<string, string> = {
  अ: 'a', आ: 'aa', इ: 'i', ई: 'ee', उ: 'u', ऊ: 'oo', ऋ: 'ri', ॠ: 'ri', ऌ: 'li',
  ऍ: 'e', ऎ: 'e', ए: 'e', ऐ: 'ai', ऑ: 'o', ऒ: 'o', ओ: 'o', औ: 'au',
}
const VIRAMA = '्'
const NUKTA_SIGN = '़'
const NASAL = new Set(['ं', 'ँ'])
const VISARGA = 'ः'
const PUNCT: Record<string, string> = { '।': '.', '॥': '.', ॐ: 'om', ऽ: '' }

/** One letter of a word: a consonant (or none, for a vowel standing alone) and its vowel. */
interface Unit {
  c: string
  /** null: the vowel a consonant carries unwritten ("a"), '' when it has none. */
  v: string | null
  nasal?: boolean
  visarga?: boolean
  raw?: string
}

const asDevanagari = (ch: string) => {
  const cp = ch.codePointAt(0) ?? 0
  return cp > 0x097f && cp <= INDIC_LAST ? String.fromCodePoint(INDIC_FIRST + ((cp - INDIC_FIRST) % 0x80)) : ch
}

const isIndic = (ch: string) => {
  const cp = ch.codePointAt(0) ?? 0
  return cp >= INDIC_FIRST && cp <= INDIC_LAST
}

const sounds = (u: Unit | undefined) => !!u && u.v !== ''

function readWord(word: string): string {
  const units: Unit[] = []
  for (const raw of word.normalize('NFC')) {
    const ch = asDevanagari(raw)
    const last = units[units.length - 1]
    if (ch in CONSONANTS) units.push({ c: CONSONANTS[ch], v: null })
    else if (ch === NUKTA_SIGN) {
      if (last && last.c in NUKTA) last.c = NUKTA[last.c]
    } else if (ch in SIGNS) {
      if (last) last.v = SIGNS[ch]
    } else if (ch === VIRAMA) {
      if (last) last.v = ''
    } else if (ch in VOWELS) units.push({ c: '', v: VOWELS[ch] })
    else if (NASAL.has(ch)) {
      if (last) last.nasal = true
      else units.push({ c: 'n', v: '' })
    } else if (ch === VISARGA) {
      if (last) last.visarga = true
    } else if (ch >= '०' && ch <= '९') units.push({ c: '', v: '', raw: String(ch.charCodeAt(0) - 0x0966) })
    else if (ch in PUNCT) units.push({ c: '', v: '', raw: PUNCT[ch] })
  }
  // ज्ञ is said "gy" (gyaan); ञ elsewhere is an "n".
  units.forEach((u, i) => {
    if (u.c !== 'ny') return
    const before = units[i - 1]
    if (before?.c === 'j' && before.v === '') {
      before.c = 'g'
      u.c = 'y'
    } else u.c = 'n'
  })
  // Hindi drops the unwritten "a" at the end of a word (dil, not dila), and inside one between
  // two sounded letters (dhad-kan), but not before -iya (kesariya, rasiya), nor where three
  // consonants would meet (zin-da-gi: the nasal is one of them).
  const n = units.length
  if (n > 1 && units[n - 1].c && units[n - 1].v === null) units[n - 1].v = ''
  for (let i = n - 2; i >= 1; i--) {
    const u = units[i]
    const next = units[i + 1]
    const suffix = (next.v === 'i' || next.v === 'ee') && units[i + 2]?.c === 'y'
    const after = units[i - 1]
    if (u.c && u.v === null && sounds(after) && !after.nasal && next.c && sounds(next) && !suffix) u.v = ''
  }
  let out = ''
  units.forEach((u, i) => {
    if (u.raw !== undefined) {
      out += u.raw
      return
    }
    const end = i === n - 1
    let v = u.v ?? 'a'
    // A long vowel at the end is written short (tera, teri, tu), unless it is nasal (haan).
    if (end && !u.nasal) v = v === 'aa' ? 'a' : v === 'ee' ? 'i' : v === 'oo' ? 'u' : v
    out += u.c + v
    if (u.nasal) {
      const nextC = units[i + 1]?.c ?? ''
      // Before n or m the nasal is heard in it (maine); before p, b it is an m (sambhal).
      out += end && v === 'e' ? 'in' : /^(n|m)/.test(nextC) ? '' : /^(p|b)/.test(nextC) ? 'm' : 'n'
    }
    if (u.visarga) out += 'h'
  })
  return out
}

/** Text with every word in an Indian script read in Roman letters; everything else as it is. */
export function toLatin(text: string): string {
  let out = ''
  let word = ''
  for (const ch of text) {
    const punct = PUNCT[asDevanagari(ch)]
    if (isIndic(ch) && punct === undefined) {
      word += ch
      continue
    }
    if (word) out += readWord(word)
    word = ''
    out += isIndic(ch) ? punct : ch
  }
  return word ? out + readWord(word) : out
}

const hasIndic = (text: string) => /[ऀ-ൿ]/.test(text)

// --- One loose spelling -------------------------------------------------------------------

function foldWord(w: string): string {
  return w
    .replace(/ph/g, 'f')
    .replace(/ck/g, 'k')
    .replace(/q/g, 'k')
    .replace(/x/g, 'ks')
    .replace(/w/g, 'v')
    .replace(/z/g, 'j')
    .replace(/c(?!h)/g, 'k')
    .replace(/([bcdfgjklmnprstv])h/g, '$1')
    .replace(/aa/g, 'a')
    .replace(/ee|ii/g, 'i')
    .replace(/oo|uu/g, 'u')
    .replace(/iy(?=[aeiou])/g, 'i')
    .replace(/(.)\1+/g, '$1')
}

/** What a name is searched by: Roman letters, no accents, no case, one loose spelling. */
export function searchKey(text: string): string {
  const latin = hasIndic(text) ? toLatin(text) : text
  return latin
    .normalize('NFD')
    .replace(/\p{M}/gu, '')
    .toLowerCase()
    .split(/[^\p{L}\p{N}]+/u)
    .filter(Boolean)
    .map(foldWord)
    .join(' ')
}

/** Edits between `a` and `b`, or more than `max` once it is clear there are. */
function distance(a: string, b: string, max: number): number {
  if (Math.abs(a.length - b.length) > max) return max + 1
  let prev = Array.from({ length: b.length + 1 }, (_, j) => j)
  for (let i = 1; i <= a.length; i++) {
    const row = [i]
    let best = i
    for (let j = 1; j <= b.length; j++) {
      row[j] = Math.min(prev[j] + 1, row[j - 1] + 1, prev[j - 1] + (a[i - 1] === b[j - 1] ? 0 : 1))
      best = Math.min(best, row[j])
    }
    if (best > max) return max + 1
    prev = row
  }
  return prev[b.length]
}

/** Words this long may have one letter wrong, missing or extra. */
const FORGIVING = 5

/** Does a word of the text begin like `token`, but for one slip (never in its first letter)? */
function nearly(token: string, words: string[]): boolean {
  for (const w of words) {
    if (w[0] !== token[0]) continue
    for (let len = token.length - 1; len <= token.length + 1; len++) {
      if (len > w.length || len < 1) continue
      if (distance(token, w.slice(0, len), 1) <= 1) return true
    }
  }
  return false
}

// A search key's words, kept: a list is searched again at every key pressed.
const split = new Map<string, string[]>()
function wordsOf(key: string): string[] {
  let w = split.get(key)
  if (!w) {
    if (split.size > 50_000) split.clear()
    w = key.split(' ')
    split.set(key, w)
  }
  return w
}

/** How well a search key holds a query: not at all, with a slip, or exactly. */
export const NO = 0
export const NEARLY = 1
export const EXACTLY = 2

/** A word this long may also be found inside another word ("hman" in Rahman), ranked after. */
const INSIDE = 4

// The first letter of each word of a key ("kuch kuch hota hai" → "kkhh"), kept like the words.
const firsts = new Map<string, string>()
function initialsOf(key: string): string {
  let i = firsts.get(key)
  if (i === undefined) {
    if (firsts.size > 50_000) firsts.clear()
    i = wordsOf(key)
      .map((w) => w[0] ?? '')
      .join('')
    firsts.set(key, i)
  }
  return i
}

/** Initials typed, spelled the way a key's words begin (c and q as k, w as v, z as j). */
const asInitials = (word: string) =>
  word
    .normalize('NFD')
    .replace(/\p{M}/gu, '')
    .toLowerCase()
    .replace(/[cq]/g, 'k')
    .replace(/w/g, 'v')
    .replace(/z/g, 'j')

/** One letter over and over ("aaaa"): no song is found by that. */
const repeated = (word: string) => word.length >= 3 && /^(.)\1+$/u.test(word)

/**
 * A test for a typed query, the same for every search box. For a search key (from
 * `searchKey`) it says how well the key holds every word typed, in any order:
 *
 * - EXACTLY when each word begins a word of the key ("rahm" in "a r rahman"), or is a run of
 *   initials ("kkhh" for Kuch Kuch Hota Hai, "ar" for A.R.);
 * - NEARLY when a word of four letters or more is inside a word ("hman"), or a word of five or
 *   more is one letter off ("rahmna"): found, but ranked after;
 * - NO otherwise, and always for a word that is one letter over and over.
 *
 * With `exact`, no slips (for long texts such as lyrics, where a slip finds too much and costs
 * too much).
 */
export function matcher(query: string, opts: { exact?: boolean } = {}): (key: string) => number {
  const typed = query
    .normalize('NFD')
    .replace(/\p{M}/gu, '')
    .toLowerCase()
    .split(/[^\p{L}\p{N}]+/u)
    .filter(Boolean)
  if (typed.some(repeated)) return () => NO
  const tokens = searchKey(query).split(' ').filter(Boolean)
  if (!tokens.length) return () => EXACTLY
  // A typed word as initials, when it is written in Roman letters (others have no initials here).
  const initials = typed.map((w) => (/^[a-z]+$/.test(w) && w.length >= 2 ? asInitials(w) : ''))
  return (key) => {
    let how = EXACTLY
    for (let i = 0; i < tokens.length; i++) {
      const t = tokens[i]
      if (key.startsWith(t) || key.includes(' ' + t)) continue
      const ini = initials[i]
      if (ini && tokens.length === typed.length && initialsOf(key).includes(ini)) continue
      if (t.length >= INSIDE && key.includes(t)) {
        how = NEARLY
        continue
      }
      if (opts.exact || t.length < FORGIVING || !nearly(t, wordsOf(key))) return NO
      how = NEARLY
    }
    return how
  }
}

/** Items that match, exact ones first (each group in the order given). */
export function ranked<T>(items: T[], how: (item: T) => number): T[] {
  const exact: T[] = []
  const near: T[] = []
  for (const it of items) {
    const h = how(it)
    if (h === EXACTLY) exact.push(it)
    else if (h === NEARLY) near.push(it)
  }
  return near.length ? exact.concat(near) : exact
}


const kept = new WeakMap<object, string>()

/** The search key of an item's text, worked out once per item (lists are searched per key). */
export function keyFor(item: object, text: () => string): string {
  let k = kept.get(item)
  if (k === undefined) {
    k = searchKey(text())
    kept.set(item, k)
  }
  return k
}
