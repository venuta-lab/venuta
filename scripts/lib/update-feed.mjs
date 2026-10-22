// The update manifest (latest.json) every installed copy of Venuta reads, and the check that a
// release's manifest works for every version ever released.
//
// Each released version asks the address in its own tauri.conf.json (plugins.updater.endpoints),
// looks for the platform `windows-x86_64-nsis`, then `windows-x86_64` (tauri-plugin-updater 2),
// and installs only an installer signed by the key built into it. A manifest that fails any of
// that for any version leaves that version where it is, so the release stops instead.
import { createHash, createPublicKey, verify } from 'node:crypto'
import { execFileSync } from 'node:child_process'

export const REPO = 'venuta-lab/venuta'
/** What the updater looks for in `platforms`, in the order it looks. */
export const PLATFORM_KEYS = ['windows-x86_64-nsis', 'windows-x86_64']
const LATEST = new RegExp(`^https://github\\.com/${REPO}/releases/latest/download/([^/?#]+)$`)
const FIXED = new RegExp(`^https://github\\.com/${REPO}/releases/download/([^/]+)/([^/?#]+)$`)

export const setupName = (version) => `Venuta_${version}_x64-setup.exe`
export const downloadUrl = (version) => `https://github.com/${REPO}/releases/download/v${version}/${setupName(version)}`

/** The manifest for a release: both Windows platforms, one installer, one signature. */
export function makeManifest({ version, notes, pubDate, signature }) {
  const platforms = {}
  for (const k of PLATFORM_KEYS) platforms[k] = { signature, url: downloadUrl(version) }
  return { version, notes, pub_date: pubDate, platforms }
}

/** The signature again (after Authenticode signing changed the installer's bytes), everywhere. */
export function setSignature(manifest, signature) {
  for (const k of PLATFORM_KEYS) if (manifest.platforms[k]) manifest.platforms[k].signature = signature
  return manifest
}

const SEMVER = /^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?(?:\+[0-9A-Za-z.-]+)?$/
export const isSemver = (v) => typeof v === 'string' && SEMVER.test(v)

/** -1, 0 or 1, as semver orders them (a pre-release comes before its release). */
export function compareSemver(a, b) {
  const [, ...x] = SEMVER.exec(a) ?? []
  const [, ...y] = SEMVER.exec(b) ?? []
  if (!x.length || !y.length) throw new Error(`not a version: ${!x.length ? a : b}`)
  for (let i = 0; i < 3; i++) if (+x[i] !== +y[i]) return +x[i] > +y[i] ? 1 : -1
  if (!x[3] || !y[3]) return x[3] === y[3] ? 0 : x[3] ? -1 : 1
  const p = x[3].split('.')
  const q = y[3].split('.')
  for (let i = 0; i < Math.max(p.length, q.length); i++) {
    if (p[i] === undefined) return -1
    if (q[i] === undefined) return 1
    const n = /^\d+$/.test(p[i])
    const m = /^\d+$/.test(q[i])
    if (n && m && +p[i] !== +q[i]) return +p[i] > +q[i] ? 1 : -1
    if (n !== m) return n ? -1 : 1
    if (!n && p[i] !== q[i]) return p[i] > q[i] ? 1 : -1
  }
  return 0
}

/** Key id as minisign prints it, and the Ed25519 key, from tauri.conf.json's `pubkey`. */
export function decodeKey(pubkey) {
  const lines = Buffer.from(pubkey, 'base64').toString().trim().split('\n')
  const bin = Buffer.from(lines[1], 'base64') // "Ed" + key id (8) + key (32)
  if (bin.length !== 42 || bin.subarray(0, 2).toString() !== 'Ed') throw new Error('not a minisign public key')
  const key = createPublicKey({ key: Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), bin.subarray(10, 42)]), format: 'der', type: 'spki' })
  return { keyId: hexId(bin.subarray(2, 10)), key }
}

const hexId = (b) => Buffer.from(b).reverse().toString('hex').toUpperCase()

/** A manifest signature: algorithm, key id, the signature, the trusted comment and its own signature. */
export function decodeSignature(signature) {
  const lines = Buffer.from(signature, 'base64').toString().split('\n')
  const bin = Buffer.from(lines[1] ?? '', 'base64') // alg (2) + key id (8) + signature (64)
  if (bin.length !== 74 || !/^trusted comment: /.test(lines[2] ?? '')) throw new Error('not a minisign signature')
  const trusted = lines[2].replace(/^trusted comment: /, '')
  const fields = Object.fromEntries(trusted.split('\t').map((f) => [f.slice(0, f.indexOf(':')), f.slice(f.indexOf(':') + 1)]))
  return { alg: bin.subarray(0, 2).toString(), keyId: hexId(bin.subarray(2, 10)), sig: bin.subarray(10, 74), trusted, fields, global: Buffer.from(lines[3] ?? '', 'base64') }
}

/** What the updater checks: the key, the trusted comment's own signature, and the bytes (if given). */
export function verifySignature(pubkey, signature, data) {
  const k = decodeKey(pubkey)
  const s = decodeSignature(signature)
  const sameKey = k.keyId === s.keyId
  const commentOk = sameKey && verify(null, Buffer.concat([s.sig, Buffer.from(s.trusted)]), k.key, s.global)
  let bytesOk = null
  if (data) {
    const message = s.alg === 'ED' ? createHash('blake2b512').update(data).digest() : s.alg === 'Ed' ? data : null
    bytesOk = sameKey && !!message && verify(null, message, k.key, s.sig)
  }
  return { sameKey, keyId: s.keyId, expected: k.keyId, commentOk, bytesOk, fields: s.fields }
}

/**
 * Whether every version in `clients` finds this manifest, accepts it and can verify it.
 * - assets: the file names the release carries (unknown: not checked).
 * - installer: the installer's bytes, to check the signature over them.
 * - prerelease: the release is a pre-release; copies on the stable address never see it.
 * - strict: a new release's manifest (both platform names).
 */
export function checkManifest(manifest, { clients, assets = null, installer = null, prerelease = false, strict = false }) {
  const problems = []
  const version = manifest?.version
  if (!isSemver(version)) problems.push(`version "${version}" is not a semver version`)
  else if (!prerelease && /-/.test(version)) problems.push(`version ${version} is a pre-release version on a stable release`)
  const platforms = manifest?.platforms ?? {}
  const present = PLATFORM_KEYS.filter((k) => platforms[k])
  if (!present.length) problems.push(`no Windows platform (${PLATFORM_KEYS.join(' or ')})`)
  if (strict) for (const k of PLATFORM_KEYS) if (!platforms[k]) problems.push(`no ${k} platform`)
  const entries = present.map((k) => platforms[k])
  if (entries.some((e) => e.url !== entries[0].url || e.signature !== entries[0].signature)) problems.push('the Windows platforms point at different files')
  const entry = entries[0]
  if (entry && isSemver(version)) {
    if (entry.url !== downloadUrl(version)) problems.push(`url ${entry.url} is not ${downloadUrl(version)}`)
    try {
      const s = decodeSignature(entry.signature)
      if (s.fields.file !== setupName(version)) problems.push(`the signature is for the file ${s.fields.file}, not ${setupName(version)}`)
      if (s.fields.version !== undefined && s.fields.version !== version) problems.push(`the signature is for version ${s.fields.version}, not ${version}`)
    } catch (e) {
      problems.push(`signature: ${e.message}`)
    }
  }
  if (assets) for (const f of [setupName(version), 'latest.json']) if (!assets.includes(f)) problems.push(`the release has no ${f}`)
  if (!clients?.length) problems.push('no released versions to check against (fetch the tags)')

  const rows = (clients ?? []).map((c) => {
    const why = []
    const row = { client: c.tag, version: c.version, endpoint: c.endpoints?.[0] ?? null, platform: present[0] ?? null, ok: false, skipped: false, why }
    if (prerelease && (c.endpoints ?? []).every((u) => LATEST.test(u))) {
      row.skipped = true
      row.ok = true
      why.push('pre-release: this version reads the stable address and keeps the current release')
      return row
    }
    // The endpoint has to land on this manifest.
    const reaches = (c.endpoints ?? []).some((u) => {
      const latest = LATEST.exec(u)
      if (latest) {
        if (assets && !assets.includes(latest[1])) why.push(`asks releases/latest/download/${latest[1]}, which the release does not carry (needs latest.json)`)
        return !assets || assets.includes(latest[1])
      }
      const fixed = FIXED.exec(u)
      if (fixed) {
        if (fixed[1] !== `v${version}`) why.push(`asks the fixed release ${fixed[1]}, never this one`)
        return fixed[1] === `v${version}` && (!assets || assets.includes(fixed[2]))
      }
      why.push(`asks ${u}, which this check can't follow`)
      return false
    })
    if (!reaches) return row
    if (!present.length) why.push('finds no Windows platform, so its check fails')
    if (isSemver(version) && isSemver(c.version) && compareSemver(version, c.version) < 0) why.push(`is newer (${c.version}) than the release (${version})`)
    if (!['passive', 'basicUi', 'quiet', undefined].includes(c.installMode)) why.push(`unknown install mode ${c.installMode}`)
    if (entry) {
      try {
        const v = verifySignature(c.pubkey, entry.signature, installer)
        if (!v.sameKey) why.push(`trusts key ${v.expected}; the signature is by key ${v.keyId}`)
        else if (!v.commentOk) why.push("the signature's trusted comment doesn't verify")
        if (installer && v.sameKey && !v.bytesOk) {
          why.push("the installer's bytes don't match the signature")
          if (!problems.includes("the installer's bytes don't match the signature")) problems.push("the installer's bytes don't match the signature")
        }
      } catch (e) {
        why.push(`signature: ${e.message}`)
      }
    }
    row.ok = why.length === 0 && problems.length === 0
    return row
  })
  return { ok: problems.length === 0 && rows.every((r) => r.ok), version, problems, rows }
}

/** Every released version's updater settings, from its tag, and the working tree's. */
export function clientsFromGit(root) {
  const git = (...a) => execFileSync('git', a, { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] })
  const tags = git('tag', '--list', 'v*')
    .split('\n')
    .map((t) => t.trim())
    .filter((t) => isSemver(t.slice(1)))
    .sort((a, b) => compareSemver(a.slice(1), b.slice(1)))
  return tags.map((tag) => {
    const conf = JSON.parse(git('show', `${tag}:app/src-tauri/tauri.conf.json`))
    const lock = git('show', `${tag}:Cargo.lock`)
    const plugin = /name = "tauri-plugin-updater"\nversion = "([^"]+)"/.exec(lock)?.[1] ?? null
    const u = conf.plugins?.updater ?? {}
    return { tag, version: conf.version, endpoints: u.endpoints ?? [], pubkey: u.pubkey, installMode: u.windows?.installMode, plugin }
  })
}

/** The result as lines to read in a terminal or a CI log. */
export function report(r) {
  const out = [`Manifest ${r.version}: ${r.ok ? 'every released version can update to it' : 'NOT every released version can update to it'}`]
  for (const p of r.problems) out.push(`  problem: ${p}`)
  for (const row of r.rows) out.push(`  ${row.ok ? 'ok  ' : 'FAIL'} ${row.client.padEnd(8)} ${row.skipped ? 'skipped' : row.platform ?? '-'}${row.why.length ? `: ${row.why.join('; ')}` : ''}`)
  return out.join('\n')
}
