// node --test scripts/tests/
// The release's update manifest, checked the way every installed version reads it. A throwaway
// key signs a fake installer here; the real key never comes near a test.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { createHash, generateKeyPairSync, randomBytes, sign } from 'node:crypto'
import { readFileSync } from 'node:fs'
import {
  PLATFORM_KEYS,
  checkManifest,
  compareSemver,
  decodeKey,
  downloadUrl,
  makeManifest,
  setSignature,
  setupName,
} from '../lib/update-feed.mjs'

const ENDPOINT = 'https://github.com/venuta-lab/venuta/releases/latest/download/latest.json'

/** A minisign key pair as Tauri writes it: the public key's text, base64 again. */
function keyPair() {
  const { publicKey, privateKey } = generateKeyPairSync('ed25519')
  const raw = Buffer.from(publicKey.export({ format: 'jwk' }).x, 'base64url')
  const keyId = randomBytes(8)
  const line = Buffer.concat([Buffer.from('Ed'), keyId, raw]).toString('base64')
  const text = `untrusted comment: minisign public key: ${Buffer.from(keyId).reverse().toString('hex').toUpperCase()}\n${line}\n`
  return { pubkey: Buffer.from(text).toString('base64'), privateKey, keyId }
}

/** A signature the way `tauri signer sign` makes one (prehashed, "ED"), base64 again. */
function signFile(kp, data, trusted) {
  const sig = sign(null, createHash('blake2b512').update(data).digest(), kp.privateKey)
  const global = sign(null, Buffer.concat([sig, Buffer.from(trusted)]), kp.privateKey)
  const text = [
    'untrusted comment: signature from tauri secret key',
    Buffer.concat([Buffer.from('ED'), kp.keyId, sig]).toString('base64'),
    `trusted comment: ${trusted}`,
    global.toString('base64'),
    '',
  ].join('\n')
  return Buffer.from(text).toString('base64')
}

const kp = keyPair()
const installer = Buffer.concat([Buffer.from('MZ'), randomBytes(4096)])
const comment = (v, file = setupName(v)) => `timestamp:1792365701\tfile:${file}\tversion:${v}`
const release = (v = '0.3.1', data = installer, key = kp, trusted = comment(v)) =>
  makeManifest({ version: v, notes: 'Notes', pubDate: '2026-10-22T00:00:00.000Z', signature: signFile(key, data, trusted) })
const client = (version, extra = {}) => ({ tag: `v${version}`, version, endpoints: [ENDPOINT], pubkey: kp.pubkey, installMode: 'passive', ...extra })
const clients = ['0.1.0', '0.1.1', '0.2.0', '0.2.1', '0.2.2', '0.2.3', '0.3.0'].map((v) => client(v))
const assets = (v = '0.3.1') => [setupName(v), `${setupName(v)}.sha256`, 'latest.json']

test('the manifest names both Windows platforms, at the release download address', () => {
  const m = release()
  assert.deepEqual(Object.keys(m.platforms).sort(), [...PLATFORM_KEYS].sort())
  for (const k of PLATFORM_KEYS) assert.equal(m.platforms[k].url, downloadUrl('0.3.1'))
  assert.equal(m.platforms['windows-x86_64'].url, 'https://github.com/venuta-lab/venuta/releases/download/v0.3.1/Venuta_0.3.1_x64-setup.exe')
})

test('a good release passes for every old version, the installer bytes checked too', () => {
  const r = checkManifest(release(), { clients, installer, assets: assets(), strict: true })
  assert.equal(r.ok, true, JSON.stringify(r, null, 1))
  assert.equal(r.rows.length, clients.length)
  assert.ok(r.rows.every((row) => row.ok && row.platform === 'windows-x86_64-nsis'))
})

test('a release without latest.json among its files fails every version that reads releases/latest', () => {
  const r = checkManifest(release(), { clients, installer, assets: [setupName('0.3.1')] })
  assert.equal(r.ok, false)
  assert.ok(r.rows.every((row) => row.why.some((w) => /latest\.json/.test(w))))
})

test('a pre-release never reaches the versions that read the stable address', () => {
  const r = checkManifest(release('0.3.1'), { clients, installer, assets: assets(), prerelease: true })
  assert.equal(r.ok, true)
  assert.ok(r.rows.every((row) => row.skipped))
  // But a stable release must not carry a pre-release version.
  const pre = checkManifest(release('0.4.0-beta.1'), { clients, assets: assets('0.4.0-beta.1') })
  assert.equal(pre.ok, false)
})

test('a manifest without a Windows platform fails, even for versions with nothing newer', () => {
  const m = release()
  delete m.platforms['windows-x86_64']
  delete m.platforms['windows-x86_64-nsis']
  m.platforms['darwin-aarch64'] = { url: 'https://x/y', signature: 'z' }
  const r = checkManifest(m, { clients, assets: assets() })
  assert.equal(r.ok, false)
  assert.ok(r.rows.every((row) => !row.ok))
})

test('old manifests with only windows-x86_64 still serve old versions; strict asks for both', () => {
  const m = release()
  delete m.platforms['windows-x86_64-nsis']
  assert.equal(checkManifest(m, { clients, installer, assets: assets() }).ok, true)
  const strict = checkManifest(m, { clients, installer, assets: assets(), strict: true })
  assert.equal(strict.ok, false)
  assert.ok(strict.problems.some((p) => /windows-x86_64-nsis/.test(p)))
})

test('a signature by another key is refused for every version', () => {
  const other = keyPair()
  const r = checkManifest(release('0.3.1', installer, other), { clients, installer, assets: assets() })
  assert.equal(r.ok, false)
  assert.ok(r.rows.every((row) => row.why.some((w) => /key/.test(w))))
})

test('changed installer bytes fail the signature', () => {
  const m = release()
  const tampered = Buffer.from(installer)
  tampered[100] ^= 1
  const r = checkManifest(m, { clients, installer: tampered, assets: assets() })
  assert.equal(r.ok, false)
  assert.ok(r.problems.some((p) => /bytes/.test(p)))
})

test('a signature made for another version or file is refused', () => {
  const wrongVersion = checkManifest(release('0.3.1', installer, kp, comment('0.3.0')), { clients, installer, assets: assets() })
  assert.equal(wrongVersion.ok, false)
  const wrongFile = checkManifest(release('0.3.1', installer, kp, comment('0.3.1', 'other.exe')), { clients, installer, assets: assets() })
  assert.equal(wrongFile.ok, false)
})

test('a signature written again after Authenticode signing reaches both platforms', () => {
  const m = release()
  const signed = Buffer.concat([installer, Buffer.from('authenticode')])
  setSignature(m, signFile(kp, signed, comment('0.3.1')))
  assert.equal(m.platforms['windows-x86_64'].signature, m.platforms['windows-x86_64-nsis'].signature)
  assert.equal(checkManifest(m, { clients, installer: signed, assets: assets(), strict: true }).ok, true)
})

test('a release older than an installed version is no update for it', () => {
  const r = checkManifest(release('0.2.2'), { clients, installer, assets: assets('0.2.2') })
  assert.equal(r.ok, false)
  const late = r.rows.filter((row) => !row.ok).map((row) => row.client)
  assert.deepEqual(late, ['v0.2.3', 'v0.3.0'])
})

test('the address must be the release download of this version', () => {
  const m = release()
  for (const k of PLATFORM_KEYS) m.platforms[k].url = 'http://example.com/Venuta_0.3.1_x64-setup.exe'
  assert.equal(checkManifest(m, { clients, installer, assets: assets() }).ok, false)
})

test('an endpoint the check does not know is a failure, never a pass', () => {
  const r = checkManifest(release(), { clients: [client('0.2.0', { endpoints: ['https://example.com/u.json'] })], installer, assets: assets() })
  assert.equal(r.ok, false)
})

test('versions compare as semver, pre-releases before their release', () => {
  assert.equal(compareSemver('0.3.1', '0.3.0'), 1)
  assert.equal(compareSemver('0.10.0', '0.9.9'), 1)
  assert.equal(compareSemver('0.4.0-beta.1', '0.4.0'), -1)
  assert.equal(compareSemver('0.4.0-beta.2', '0.4.0-beta.10'), -1)
  assert.equal(compareSemver('0.3.0', '0.3.0'), 0)
})

test("today's published manifest (0.3.0) serves every version before it, under the real key", () => {
  const live = JSON.parse(readFileSync(new URL('./fixtures/latest-0.3.0.json', import.meta.url), 'utf8'))
  const conf = JSON.parse(readFileSync(new URL('../../app/src-tauri/tauri.conf.json', import.meta.url), 'utf8'))
  const real = ['0.1.0', '0.1.1', '0.2.0', '0.2.1', '0.2.2', '0.2.3'].map((v) => client(v, { pubkey: conf.plugins.updater.pubkey }))
  const r = checkManifest(live, { clients: real, assets: ['Venuta_0.3.0_x64-setup.exe', 'latest.json'] })
  assert.equal(r.ok, true, JSON.stringify(r, null, 1))
  assert.ok(r.rows.every((row) => row.platform === 'windows-x86_64'))
  assert.equal(decodeKey(conf.plugins.updater.pubkey).keyId, 'E3A9CF88E79B3655')
})
