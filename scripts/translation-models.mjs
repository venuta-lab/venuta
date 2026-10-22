// Writes crates/lyrics-translate/data/bergamot-models.json: the translation models Venuta can
// download, each file with its address, its download size and the SHA-256 it must have.
//
//   node scripts/translation-models.mjs
//
// Where it reads from (public, no account):
// - Mozilla's model registry (the translations project's own bucket): which models are
//   released, for which platform, and where each file is.
// - Firefox's Remote Settings records for the same models: the SHA-256 of every file once
//   unpacked (the registry gives only the model's). A model whose hashes don't agree between
//   the two is left out.
// - A HEAD request per file for its download size (shown before anything is downloaded).
//
// Only models marked "Release", "Release Desktop" or "Release Android" are kept. Nothing about
// the person running it is sent: the User-Agent is generic.

import { writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const UA = 'Venuta-dev (https://github.com/venuta-lab/venuta)'
const REGISTRY = 'https://storage.googleapis.com/moz-fx-translations-data--303e-prod-translations-data/db/models.json'
const RECORDS = 'https://firefox.settings.services.mozilla.com/v1/buckets/main/collections/translations-models-v2/records'
const OUT = join(dirname(fileURLToPath(import.meta.url)), '..', 'crates', 'lyrics-translate', 'data', 'bergamot-models.json')

const get = async (url) => {
  const r = await fetch(url, { headers: { 'User-Agent': UA } })
  if (!r.ok) throw new Error(`${url}: ${r.status}`)
  return r.json()
}
const head = async (url) => {
  const r = await fetch(url, { method: 'HEAD', headers: { 'User-Agent': UA } })
  if (!r.ok) throw new Error(`${url}: ${r.status}`)
  return Number(r.headers.get('content-length'))
}

const registry = await get(REGISTRY)
const records = (await get(RECORDS)).data
const names = new Intl.DisplayNames(['en'], { type: 'language' })
const langName = (tag) => {
  try {
    return names.of(tag) ?? tag
  } catch {
    return tag
  }
}
const KINDS = { model: 'model', vocab: 'vocab', srcVocab: 'srcvocab', trgVocab: 'trgvocab', lexicalShortlist: 'lex' }
const PLATFORM = { Release: 'all', 'Release Desktop': 'desktop', 'Release Android': 'android' }

const models = []
const skipped = []
for (const [pair, list] of Object.entries(registry.models)) {
  for (const m of list) {
    const platform = PLATFORM[m.releaseStatus]
    if (!platform) continue
    const modelHash = m.files.model?.uncompressedHash
    // The model is the same file in both places when its unpacked SHA-256 is.
    const modelRecord = records.find((r) => r.fileType === 'model' && r.decompressedHash === modelHash)
    if (!modelRecord) {
      skipped.push(`${pair} ${m.architecture}: not in Remote Settings`)
      continue
    }
    const files = []
    let ok = true
    for (const [key, f] of Object.entries(m.files)) {
      const kind = KINDS[key]
      const name = f.path.split('/').pop().replace(/\.gz$/, '')
      // Matched by what the file is (Firefox names a few files and languages its own way:
      // zh-Hans for the registry's zh, lex.50.50.zhen for its zh_hant).
      const record = records.find(
        (r) =>
          r.fileType === kind &&
          r.sourceLanguage === modelRecord.sourceLanguage &&
          r.targetLanguage === modelRecord.targetLanguage &&
          r.version === modelRecord.version &&
          r.filter_expression === modelRecord.filter_expression,
      )
      if (!record || (f.uncompressedHash && f.uncompressedHash !== record.decompressedHash)) {
        ok = false
        skipped.push(`${pair} ${m.architecture}: no matching record for ${name}`)
        break
      }
      const url = `${registry.baseUrl}/${f.path}`
      files.push({ kind, name, url, download_bytes: await head(url), bytes: record.decompressedSize, sha256: record.decompressedHash })
    }
    if (!ok) continue
    files.sort((a, b) => a.kind.localeCompare(b.kind))
    const from = modelRecord.sourceLanguage
    const to = modelRecord.targetLanguage
    models.push({
      id: `${from}-${to}`,
      from,
      to,
      from_name: langName(from),
      to_name: langName(to),
      arch: m.architecture,
      platform,
      version: modelRecord.version,
      files,
    })
  }
}
// One model per pair and platform, the newest (as Firefox takes it): the registry keeps an older
// release beside a newer one for some pairs, and files a few under two names (no-en is nb-en).
models.sort((a, b) => b.version.localeCompare(a.version, 'en', { numeric: true }))
const seen = new Set()
const unique = models.filter((m) => {
  const k = `${m.id}|${m.platform}`
  if (seen.has(k)) {
    skipped.push(`${m.id} ${m.platform} ${m.arch} ${m.version}: an older or second listing`)
    return false
  }
  seen.add(k)
  return true
})
unique.sort((a, b) => a.id.localeCompare(b.id) || a.platform.localeCompare(b.platform))
const out = {
  publisher: 'Mozilla, Firefox Translations (https://github.com/mozilla/translations)',
  registry: REGISTRY,
  generated: registry.generated,
  models: unique,
}
writeFileSync(OUT, JSON.stringify(out, null, 1) + '\n')
console.log(`${unique.length} models written to ${OUT}`)
for (const s of skipped) console.log(`left out: ${s}`)
