// Helpers for the release workflow (.github/workflows/release.yml).
//
//   node scripts/ci-release.mjs notes <x.y.z>                # that version's CHANGELOG section
//   node scripts/ci-release.mjs resign <x.y.z> <signed dir>  # after Authenticode signing
//
// Signing the installer changes its bytes, so the updater's signature (and latest.json, which
// installed copies read) is made again from the signed file. Without that, every installed
// copy would refuse the update.
import { execFileSync } from 'node:child_process'
import { copyFileSync, existsSync, readFileSync, readdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { setSignature } from './lib/update-feed.mjs'

const root = new URL('..', import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1')
const [cmd, version, signedDir] = process.argv.slice(2)
if (!cmd || !/^\d+\.\d+\.\d+$/.test(version ?? '')) {
  console.error('Usage: node scripts/ci-release.mjs notes|resign <x.y.z> [signed dir]')
  process.exit(1)
}

if (cmd === 'notes') {
  const log = readFileSync(join(root, 'CHANGELOG.md'), 'utf8')
  const start = log.indexOf(`## ${version} `)
  if (start < 0) {
    console.log(`Venuta ${version}`)
  } else {
    const body = log.slice(log.indexOf('\n', start) + 1)
    const end = body.search(/^## /m)
    console.log((end < 0 ? body : body.slice(0, end)).trim())
  }
} else if (cmd === 'resign') {
  const dir = join(root, 'target/release/bundle/nsis')
  const setup = `Venuta_${version}_x64-setup.exe`
  const signed = readdirSync(signedDir).find((f) => f.toLowerCase().endsWith('.exe'))
  if (!signed) throw new Error(`No signed installer in ${signedDir}`)
  copyFileSync(join(signedDir, signed), join(dir, setup))
  // TAURI_SIGNING_PRIVATE_KEY and its password come from the workflow's environment.
  execFileSync('pnpm', ['tauri', 'signer', 'sign', join(dir, setup)], {
    cwd: join(root, 'app'),
    stdio: 'inherit',
    shell: process.platform === 'win32',
  })
  const sig = join(dir, `${setup}.sig`)
  if (!existsSync(sig)) throw new Error('The updater signature was not written')
  const latest = JSON.parse(readFileSync(join(dir, 'latest.json'), 'utf8'))
  // Every Windows platform in it (windows-x86_64-nsis and windows-x86_64) names this installer.
  setSignature(latest, readFileSync(sig, 'utf8').trim())
  writeFileSync(join(dir, 'latest.json'), JSON.stringify(latest, null, 2) + '\n')
  console.log(`Signed ${setup}; the updater signature and latest.json now match it.`)
} else {
  console.error(`Unknown command ${cmd}`)
  process.exit(1)
}
