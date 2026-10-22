// Will every installed Venuta update to this release? Run before a release is drafted and after
// it is published. Exits 1 when any released version would not find, accept or verify it.
//
//   node scripts/update-feed.mjs check --manifest <latest.json> [--installer <setup.exe>]
//        [--assets "a.exe,latest.json"] [--prerelease]
//     The release about to be made: its manifest against every released tag's updater settings
//     (read from git, so the tags must be fetched), the installer's bytes against the signature.
//
//   node scripts/update-feed.mjs live [--installer <setup.exe>]
//     What GitHub serves now, read-only: the release GitHub calls latest, the manifest at each
//     address a released version asks, and the installer it names (its headers only).
import { readFileSync } from 'node:fs'
import { checkManifest, clientsFromGit, compareSemver, isSemver, REPO, report } from './lib/update-feed.mjs'

const root = new URL('..', import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1')
const [cmd, ...rest] = process.argv.slice(2)
const opt = (name) => {
  const i = rest.indexOf(name)
  return i >= 0 ? rest[i + 1] : undefined
}
const UA = { 'User-Agent': 'Venuta-release-check (https://github.com/venuta-lab/venuta)' }

function released() {
  const clients = clientsFromGit(root)
  if (!clients.length) {
    console.error('No released tags (v*) in this checkout. Fetch them first (actions/checkout with fetch-depth: 0).')
    process.exit(1)
  }
  return clients
}

if (cmd === 'check') {
  const file = opt('--manifest')
  if (!file) throw new Error('--manifest <latest.json> is required')
  const manifest = JSON.parse(readFileSync(file, 'utf8'))
  const installer = opt('--installer') ? readFileSync(opt('--installer')) : null
  const assets = opt('--assets')?.split(',').map((s) => s.trim()).filter(Boolean) ?? null
  const r = checkManifest(manifest, { clients: released(), installer, assets, prerelease: rest.includes('--prerelease'), strict: true })
  console.log(report(r))
  process.exit(r.ok ? 0 : 1)
} else if (cmd === 'live') {
  const clients = released()
  const res = await fetch(`https://api.github.com/repos/${REPO}/releases?per_page=30`, { headers: { ...UA, Accept: 'application/vnd.github+json' } })
  if (!res.ok) throw new Error(`GitHub's releases list: HTTP ${res.status}`)
  const list = (await res.json()).filter((r) => !r.draft)
  const stable = list.filter((r) => !r.prerelease && isSemver(r.tag_name.replace(/^v/, '')))
  stable.sort((a, b) => compareSemver(b.tag_name.slice(1), a.tag_name.slice(1)))
  const latest = await fetch(`https://api.github.com/repos/${REPO}/releases/latest`, { headers: { ...UA, Accept: 'application/vnd.github+json' } }).then((r) => (r.ok ? r.json() : null))
  let ok = true
  const say = (good, line) => {
    ok &&= good
    console.log(`${good ? 'ok  ' : 'FAIL'} ${line}`)
  }
  say(!!latest, `GitHub's latest release: ${latest?.tag_name ?? 'none'}`)
  say(!!latest && latest.tag_name === stable[0]?.tag_name, `the newest stable release is ${stable[0]?.tag_name ?? 'none'}`)
  for (const r of list.filter((r) => /-/.test(r.tag_name) && !r.prerelease)) say(false, `${r.tag_name} has a pre-release version but is not marked pre-release`)
  const assets = latest?.assets.map((a) => a.name) ?? []
  // Each address a released version asks, as that version asks it.
  for (const url of [...new Set(clients.flatMap((c) => c.endpoints))]) {
    const m = await fetch(url, { headers: { ...UA, Accept: 'application/json' } })
    say(m.ok, `${url}: HTTP ${m.status}`)
    if (!m.ok) continue
    const manifest = JSON.parse(await m.text())
    const installer = opt('--installer') ? readFileSync(opt('--installer')) : null
    const r = checkManifest(manifest, { clients: clients.filter((c) => c.endpoints.includes(url)), installer, assets })
    console.log(report(r))
    ok &&= r.ok
    const entry = manifest.platforms?.['windows-x86_64-nsis'] ?? manifest.platforms?.['windows-x86_64']
    if (entry) {
      const head = await fetch(entry.url, { method: 'HEAD', headers: UA, redirect: 'follow' })
      say(head.ok, `${entry.url}: HTTP ${head.status}, ${head.headers.get('content-length') ?? '?'} bytes`)
    }
  }
  process.exit(ok ? 0 : 1)
} else {
  console.error('Usage: node scripts/update-feed.mjs check --manifest <latest.json> [--installer <exe>] [--assets a,b] [--prerelease] | live [--installer <exe>]')
  process.exit(1)
}
