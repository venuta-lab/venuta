// Writes the fixture songs (contract shape, docs/design/lyrics-second-line.md). Every line is
// written for these fixtures: none is from a real song.
import { writeFileSync } from 'node:fs'

const w = (start_ms, end_ms, text) => ({ start_ms, end_ms, text })
/** Words timed evenly across a line, from parallel lists of words. */
function timed(start, end, words) {
  const step = (end - start - 400) / words.length
  return words.map((t, i) => w(Math.round(start + i * step), Math.round(start + (i + 1) * step), t))
}
const second = (text, words, lang, source) => (words.length ? { text, words, lang, source } : { text, lang, source })
function line(start, end, text, script, opts = {}) {
  const ow = opts.words ? timed(start, end, opts.words) : []
  const roman = opts.roman
    ? second(opts.roman, opts.romanWords && ow.length ? opts.romanWords.map((t, i) => w(ow[i].start_ms, ow[i].end_ms, t)) : [], opts.romanLang, opts.romanSource)
    : null
  const translation = opts.tr ? second(opts.tr, [], opts.trLang, opts.trSource) : null
  // As the engine writes it: empty words and missing second lines left out.
  const out = { start_ms: start, end_ms: end, text, script }
  if (ow.length) out.words = ow
  if (roman) out.roman = roman
  if (translation) out.translation = translation
  return out
}
const song = (lang, lines) => ({ lines, lang, version: 1 })
const save = (name, value) => writeFileSync(new URL(`./${name}.json`, import.meta.url), JSON.stringify(value, null, 2) + '\n')

// Hindi: pronunciation paired from another copy of the lyrics, translated on this PC, timed word by word.
const HI = { romanLang: 'hi-Latn', romanSource: { kind: 'paired', by: 'lyricsplus' }, trLang: 'en', trSource: { kind: 'translated', by: 'opus-mt' } }
const hiLine = (start, text, roman, tr) => {
  const words = text.split(' ')
  const romanWords = roman.split(' ')
  return line(start, start + 4000, text, 'Deva', { ...HI, words, roman, romanWords, tr })
}
save(
  'hindi',
  song('hi', [
    hiLine(1000, 'आज फिर से चाँद निकला है', 'Aaj phir se chaand nikla hai', 'Tonight the moon has come out again'),
    hiLine(5000, 'तेरी बातों में खो गया दिल', 'Teri baaton mein kho gaya dil', 'My heart got lost in your words'),
    hiLine(
      9000,
      'सपनों की इस नगरी में हम दोनों साथ चलेंगे जब तक साँसें चलती रहेंगी',
      'Sapnon ki is nagri mein hum dono saath chalenge jab tak saansein chalti rahengi',
      'In this city of dreams the two of us will walk together for as long as we keep breathing',
    ),
    hiLine(13000, 'हर रात तुझे याद करता हूँ', 'Har raat tujhe yaad karta hoon', 'Every night I think of you'),
    hiLine(17000, 'चल कहीं दूर चलें हम', 'Chal kahin door chalein hum', 'Come, let us go somewhere far away'),
    line(21000, 25000, '', 'Zyyy'),
  ]),
)

// Japanese: pronunciation from the lyrics (Netease), its translation in Chinese only.
const JA = { romanLang: 'ja-Latn', romanSource: { kind: 'lyrics', by: 'netease' }, trLang: 'zh-Hans', trSource: { kind: 'lyrics', by: 'netease' } }
const jaLine = (start, words, romanWords, tr, join = '') =>
  line(start, start + 4000, words.join(join), 'Jpan', { ...JA, words, roman: romanWords.join(' ').replace(' ,', ','), romanWords, tr })
save(
  'japanese',
  song('ja', [
    jaLine(1000, ['夜空', 'に', '星', 'が', '光る'], ['yozora', 'ni', 'hoshi', 'ga', 'hikaru'], '夜空中星星闪耀'),
    jaLine(5000, ['君', 'の', '声', 'が', '聞こえる'], ['kimi', 'no', 'koe', 'ga', 'kikoeru'], '我听见你的声音'),
    jaLine(9000, ['明日', 'も', 'また', '会おう'], ['ashita', 'mo', 'mata', 'aou'], '明天也再见面吧'),
    jaLine(
      13000,
      ['遠く', 'まで', '歩いて', 'いこう', '、', '二人', 'で', '手', 'を', 'つないで'],
      ['tooku', 'made', 'aruite', 'ikou', ',', 'futari', 'de', 'te', 'wo', 'tsunaide'],
      '牵着手，我们一起走向远方',
    ),
  ]),
)

// The same Japanese song translated into English by Google Cloud Translation with your key: every
// row under every line, and the attribution Google asks for beside each translated line.
const JA_GOOGLE = { ...JA, trLang: 'en', trSource: { kind: 'translated', by: 'google' } }
const jaGoogleLine = (start, words, romanWords, tr) =>
  line(start, start + 4000, words.join(''), 'Jpan', { ...JA_GOOGLE, words, roman: romanWords.join(' ').replace(' ,', ','), romanWords, tr })
save(
  'japanese-google',
  song('ja', [
    jaGoogleLine(1000, ['夜空', 'に', '星', 'が', '光る'], ['yozora', 'ni', 'hoshi', 'ga', 'hikaru'], 'Stars shine in the night sky'),
    jaGoogleLine(5000, ['君', 'の', '声', 'が', '聞こえる'], ['kimi', 'no', 'koe', 'ga', 'kikoeru'], 'I can hear your voice'),
    jaGoogleLine(9000, ['明日', 'も', 'また', '会おう'], ['ashita', 'mo', 'mata', 'aou'], "Let's meet again tomorrow"),
    jaGoogleLine(
      13000,
      ['遠く', 'まで', '歩いて', 'いこう', '、', '二人', 'で', '手', 'を', 'つないで'],
      ['tooku', 'made', 'aruite', 'ikou', ',', 'futari', 'de', 'te', 'wo', 'tsunaide'],
      "Let's walk far away together, holding hands",
    ),
  ]),
)

// Spanish: in Latin letters already, so a translation only (made on this PC), no word timing.
const ES = { trLang: 'en', trSource: { kind: 'translated', by: 'firefox-translations' } }
save(
  'spanish',
  song('es', [
    line(1000, 5000, 'Bajo la luna bailamos otra vez', 'Latn', { ...ES, tr: 'Under the moon we dance again' }),
    line(5000, 9000, 'Tu sonrisa ilumina la ciudad', 'Latn', { ...ES, tr: 'Your smile lights up the city' }),
    line(9000, 13000, 'Y aunque la noche sea larga, no tengo miedo de perderme contigo', 'Latn', {
      ...ES,
      tr: 'And even if the night is long, I am not afraid to get lost with you',
    }),
    line(13000, 17000, 'Quédate conmigo hasta el amanecer', 'Latn', { ...ES, tr: 'Stay with me until dawn' }),
  ]),
)

// Hindi and English mixed: English lines have nothing under them; one translation corrected by hand.
const MIX = { romanLang: 'hi-Latn', romanSource: { kind: 'made' }, trLang: 'en', trSource: { kind: 'translated', by: 'opus-mt' } }
save(
  'mixed',
  song('hi', [
    line(1000, 5000, 'दिल ये मेरा, baby please stay', 'Deva', { ...MIX, roman: 'Dil ye mera, baby please stay', tr: 'This heart of mine, baby please stay' }),
    line(5000, 9000, 'Hold me close tonight', 'Latn'),
    line(9000, 13000, 'तू ही मेरी dream girl है', 'Deva', { ...MIX, roman: 'Tu hi meri dream girl hai', tr: 'You alone are my dream girl' }),
    line(13000, 17000, 'We can dance until the morning light', 'Latn'),
    line(17000, 21000, 'धड़कन बोले बस तेरा नाम', 'Deva', { ...MIX, roman: 'Dhadkan bole bas tera naam', tr: 'My heartbeat says only your name', trSource: { kind: 'corrected' } }),
  ]),
)

// English, nothing extra.
save(
  'plain',
  song('en', [
    line(1000, 5000, 'Morning comes in slowly', 'Latn'),
    line(5000, 9000, 'Coffee cooling on the window sill', 'Latn'),
    line(9000, 13000, 'We will find our way back home', 'Latn'),
    line(13000, 17000, 'Before the streetlights fade', 'Latn'),
  ]),
)

// Lines with nowhere to wrap: one very long word, a line whose words are joined by no-break
// spaces (as some lyrics send them), a long line in a script written without spaces (Thai), and
// a short one after them.
const NB = '\u00a0'
save(
  'unbroken',
  song('hi', [
    line(1000, 5000, 'Llanfairpwllgwyngyllgogerychwyrndrobwllllantysiliogogogoch', 'Latn', { ...ES, tr: 'A town with a very long name' }),
    line(5000, 9000, ['सपनों', 'की', 'इस', 'नगरी', 'में', 'हम', 'दोनों', 'साथ', 'चलेंगे', 'हमेशा'].join(NB), 'Deva', {
      ...MIX,
      roman: 'Sapnon ki is nagri mein hum dono saath chalenge hamesha',
      tr: 'In this city of dreams the two of us will always walk together',
    }),
    line(9000, 13000, 'ฉันจะรอเธออยู่ตรงนี้ตลอดไปไม่ว่าจะนานแค่ไหนก็ตามเพราะหัวใจของฉันเป็นของเธอ', 'Thai'),
    line(13000, 17000, 'Short and sweet', 'Latn'),
  ]),
)
