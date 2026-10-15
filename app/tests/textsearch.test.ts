// node --test tests/   (Node runs the TypeScript as is)
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { matcher, searchKey, toLatin } from '../src/lib/textsearch.ts'

test('Devanagari reads in Roman letters the way songs are usually spelled', () => {
  assert.equal(toLatin('केसरिया'), 'kesariya')
  assert.equal(toLatin('तुम ही हो'), 'tum hi ho')
  assert.equal(toLatin('धड़कन'), 'dhadkan')
  assert.equal(toLatin('कमल'), 'kamal')
  assert.equal(toLatin('समझना'), 'samajhna')
  assert.equal(toLatin('इश्क़'), 'ishq')
  assert.equal(toLatin('मैं'), 'main')
  assert.equal(toLatin('तुम्हें'), 'tumhein')
  assert.equal(toLatin('ज्ञान'), 'gyaan')
  assert.equal(toLatin('फूलों'), 'phoolon')
  assert.equal(toLatin('न'), 'na')
  assert.equal(toLatin('दिलबर'), 'dilbar')
  assert.equal(toLatin('सीखना'), 'seekhna')
  assert.equal(toLatin('तेरी'), 'teri')
  assert.equal(toLatin('हाँ'), 'haan')
  assert.equal(toLatin('मैंने'), 'maine')
  assert.equal(toLatin('संग'), 'sang')
  assert.equal(toLatin('ज़िंदगी'), 'zindagi')
  assert.equal(toLatin('दर्द'), 'dard')
})

test('lines keep their spaces, punctuation and Latin words', () => {
  assert.equal(toLatin('तेरा मेरा, Kesariya!'), 'tera mera, Kesariya!')
  assert.equal(toLatin('दिल।'), 'dil.')
})

test('other Indian scripts read through the same rules', () => {
  assert.equal(toLatin('ਦਿਲ'), 'dil') // Gurmukhi
  assert.equal(toLatin('દિલ'), 'dil') // Gujarati
})

test('the other way round: Roman letters stay as they are', () => {
  assert.equal(toLatin('Kesariya (From "Brahmastra")'), 'Kesariya (From "Brahmastra")')
})

const finds = (query: string, text: string) => matcher(query)(searchKey(text))

test('a song in Devanagari is found by its name in Roman letters, spelled loosely', () => {
  for (const q of ['kesariya', 'kesaria', 'kesariyaa', 'Kesariya', 'kesar']) {
    assert.ok(finds(q, 'केसरिया'), q)
  }
  assert.ok(finds('tum hi ho', 'तुम ही हो'))
  assert.ok(finds('dhadkan', 'धड़कन'))
  assert.ok(finds('ishq', 'इश्क़ वाला लव'))
  assert.ok(finds('phoolon', 'फूलों का तारों का'))
  assert.ok(finds('foolon', 'फूलों का तारों का'))
})

test('and a song in Roman letters is found by typing Devanagari', () => {
  assert.ok(finds('केसरिया', 'Kesariya'))
  assert.ok(finds('तुम ही हो', 'Tum Hi Ho'))
})

test('every word must be there, in any order', () => {
  assert.ok(finds('arijit kesariya', 'Kesariya Arijit Singh'))
  assert.ok(!finds('arijit kesariya', 'Kesariya Pritam'))
})

test('an exact matcher forgives nothing', () => {
  assert.ok(!matcher('kesarya', { exact: true })(searchKey('Kesariya')))
  assert.ok(matcher('kesaria', { exact: true })(searchKey('केसरिया')))
})

test('one slip is forgiven in a longer word, not in a short one', () => {
  assert.ok(finds('kesarya', 'Kesariya'))
  assert.ok(finds('arjit', 'Arijit Singh'))
  assert.ok(!finds('tun', 'Tum Hi Ho'))
  assert.ok(!finds('rahman', 'Arijit Singh'))
})

test('accents do not matter', () => {
  assert.ok(finds('beyonce', 'Beyoncé'))
  assert.ok(finds('Beyoncé', 'Beyonce'))
})

test('an empty query matches everything', () => {
  assert.ok(finds('', 'anything'))
  assert.ok(finds('   ', 'anything'))
})

// The 14 October review (W7): the same rule for every search box, its forgiveness scaled to the query.
const how = (query: string, text: string) => matcher(query)(searchKey(text))

test('a short word matches the start of a word, never the middle of one', () => {
  assert.ok(!finds('qa', 'Escape_point'), 'qa is not inside Escape')
  assert.ok(!finds('ar', 'Kesariya'))
  assert.ok(!finds('ar', 'Sharma Ji'))
  assert.ok(finds('ar', 'Arijit Singh'))
  assert.ok(finds('sh', 'Shilpa Rao'))
})

test('one letter over and over finds nothing', () => {
  assert.ok(!finds('a'.repeat(200), 'A.R. Rahman'))
  assert.ok(!finds('aaaa', 'Kesariya'))
  assert.ok(!finds('zzz', 'Zzz Lullaby'), 'not even a word like it')
})

test('initials find the words they begin', () => {
  assert.ok(finds('kkhh', 'Kuch Kuch Hota Hai'))
  assert.ok(finds('ar rahman', 'A.R. Rahman'))
  assert.ok(!finds('kkhh', 'Kal Ho Naa Ho'))
})

test('a longer word inside another still matches, after the words that begin with it', () => {
  assert.ok(finds('hman', 'A.R. Rahman'))
  assert.ok(how('rahm', 'A.R. Rahman') > how('hman', 'A.R. Rahman'), 'the start of a word ranks first')
  assert.ok(how('kesariya', 'Kesariya') > how('kesarya', 'Kesariya'), 'exact before a slip')
})

test('every typed word must still be there, and the review\'s searches still find their songs', () => {
  assert.ok(finds('kesariya', 'Kesariya'))
  assert.ok(finds('kesariyaa', 'Kesariya'))
  assert.ok(finds('केसरिया', 'Kesariya'))
  assert.ok(finds('tum ho', 'Tum Hi Ho'))
  assert.ok(finds('rahmna', 'A.R. Rahman'))
})

test('one spelling table for the search box and for matching playlists (shared with the engine)', async () => {
  const { readFileSync } = await import('node:fs')
  const { cases } = JSON.parse(readFileSync(new URL('./fixtures/spelling-keys.json', import.meta.url), 'utf8')) as { cases: [string, string][] }
  for (const [name, key] of cases) assert.equal(searchKey(name), key, name)
})
