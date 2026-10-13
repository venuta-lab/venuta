// Make a release: node scripts/release.mjs <version> [--notes file.md] [--publish]
//
//   1. sets the version (tauri.conf.json, Cargo workspace, app/package.json);
//   2. builds the Windows installer, signed with the updater key (never stored in the repo:
//      VENUTA_SIGNING_KEY, or %USERPROFILE%\.venuta-signing\venuta-updater.key);
//   3. writes latest.json, which installed copies read to find the new version;
//   4. tags the commit and creates a GitHub release with both files, as a draft unless
//      --publish (a draft is seen by nobody until you publish it on GitHub).
import { execFileSync } from 'node:child_process'
import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { join } from 'node:path'

const REPO = 'theSelfSa/venuta'
const root = new URL('..', import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1')
const args = process.argv.slice(2)
const version = args.find((a) => /^\d+\.\d+\.\d+$/.test(a))
if (!version) {
  console.error('Usage: node scripts/release.mjs <x.y.z> [--notes notes.md] [--publish]')
  process.exit(1)
}
const notesAt = args.indexOf('--notes')
const notes = notesAt >= 0 ? readFileSync(args[notesAt + 1], 'utf8').trim() : `Venuta ${version}`
const publish = args.includes('--publish')
const run = (cmd, a, opts = {}) => execFileSync(cmd, a, { stdio: 'inherit', cwd: root, shell: process.platform === 'win32', ...opts })

// 1. Version everywhere.
const conf = join(root, 'app/src-tauri/tauri.conf.json')
const c = JSON.parse(readFileSync(conf, 'utf8'))
c.version = version
writeFileSync(conf, JSON.stringify(c, null, 2) + '\n')
const cargo = join(root, 'Cargo.toml')
writeFileSync(cargo, readFileSync(cargo, 'utf8').replace(/(\[workspace\.package\][^[]*?version\s*=\s*")[^"]+(")/, `$1${version}$2`))
const pkg = join(root, 'app/package.json')
const p = JSON.parse(readFileSync(pkg, 'utf8'))
p.version = version
writeFileSync(pkg, JSON.stringify(p, null, 2) + '\n')

// 2. The signed installer.
const keyFile = process.env.VENUTA_SIGNING_KEY ?? join(homedir(), '.venuta-signing', 'venuta-updater.key')
if (!existsSync(keyFile)) throw new Error(`No updater key at ${keyFile}`)
const env = { ...process.env, TAURI_SIGNING_PRIVATE_KEY: readFileSync(keyFile, 'utf8'), TAURI_SIGNING_PRIVATE_KEY_PASSWORD: '' }
run('pnpm', ['tauri', 'build', '--bundles', 'nsis'], { cwd: join(root, 'app'), env })
const dir = join(root, 'target/release/bundle/nsis')
const setup = `Venuta_${version}_x64-setup.exe`
const signature = readFileSync(join(dir, `${setup}.sig`), 'utf8').trim()

// 3. What installed copies read.
const latest = {
  version,
  notes,
  pub_date: new Date().toISOString(),
  platforms: {
    'windows-x86_64': { signature, url: `https://github.com/${REPO}/releases/download/v${version}/${setup}` },
  },
}
writeFileSync(join(dir, 'latest.json'), JSON.stringify(latest, null, 2) + '\n')

// 4. Tag and release.
run('git', ['add', 'app/src-tauri/tauri.conf.json', 'Cargo.toml', 'Cargo.lock', 'app/package.json'])
run('git', ['commit', '-m', `chore(release): ${version}`])
run('git', ['tag', `v${version}`])
run('git', ['push', 'origin', 'HEAD', `v${version}`])
const notesFile = join(dir, 'notes.md')
writeFileSync(notesFile, notes + '\n')
run('gh', ['release', 'create', `v${version}`, '--repo', REPO, '--title', `Venuta ${version}`, '--notes-file', notesFile, ...(publish ? [] : ['--draft']), join(dir, setup), join(dir, 'latest.json')])
console.log(publish ? `Published v${version}.` : `Draft v${version} created: publish it on GitHub when ready.`)
