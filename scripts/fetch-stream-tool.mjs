// Download the official standalone yt-dlp binary for Venuta.
//
// Usage (from repo root):
//   node scripts/fetch-stream-tool.mjs              # managed user path (dev/test)
//   node scripts/fetch-stream-tool.mjs --bundle     # also app/src-tauri/resources (installer)
//   node scripts/fetch-stream-tool.mjs --bundle-only
//
// Same binary the app auto-downloads on first play / Settings → Install stream tool.
import { createWriteStream, existsSync, mkdirSync, renameSync, statSync, unlinkSync } from 'node:fs'
import { homedir, platform } from 'node:os'
import { dirname, join } from 'node:path'
import { pipeline } from 'node:stream/promises'

const args = new Set(process.argv.slice(2))
const wantManaged = !args.has('--bundle-only')
const wantBundle = args.has('--bundle') || args.has('--bundle-only')

const isWin = platform() === 'win32'
const name = isWin ? 'yt-dlp.exe' : 'yt-dlp'
const url = `https://github.com/yt-dlp/yt-dlp/releases/latest/download/${name}`

const root = new URL('..', import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, '$1')

function managedDest() {
  if (isWin) {
    const base = process.env.LOCALAPPDATA || process.env.APPDATA || join(homedir(), 'AppData', 'Local')
    return join(base, 'Venuta', 'bin', name)
  }
  if (platform() === 'darwin') {
    return join(homedir(), 'Library', 'Application Support', 'Venuta', 'bin', name)
  }
  const xdg = process.env.XDG_DATA_HOME || join(homedir(), '.local', 'share')
  return join(xdg, 'Venuta', 'bin', name)
}

function bundleDest() {
  return join(root, 'app', 'src-tauri', 'resources', name)
}

async function download(dest) {
  mkdirSync(dirname(dest), { recursive: true })
  if (existsSync(dest) && statSync(dest).size > 1_000_000) {
    console.log(`Already present (${statSync(dest).size} bytes): ${dest}`)
    return dest
  }
  console.log(`Downloading ${url}`)
  console.log(`         → ${dest}`)
  const res = await fetch(url, { redirect: 'follow' })
  if (!res.ok) throw new Error(`Download failed: HTTP ${res.status}`)
  const tmp = `${dest}.download`
  try {
    await pipeline(res.body, createWriteStream(tmp))
    const size = statSync(tmp).size
    if (size < 1_000_000) throw new Error(`Download looked incomplete (${size} bytes)`)
    renameSync(tmp, dest)
  } catch (e) {
    try {
      unlinkSync(tmp)
    } catch {
      /* ignore */
    }
    throw e
  }
  console.log(`OK ${statSync(dest).size} bytes`)
  return dest
}

const targets = []
if (wantManaged) targets.push(managedDest())
if (wantBundle) {
  if (!isWin && name === 'yt-dlp.exe') {
    console.warn('Bundle path uses the Windows filename; skip --bundle on non-Windows hosts.')
  } else {
    targets.push(bundleDest())
  }
}

for (const dest of targets) {
  await download(dest)
}
console.log('Stream tool ready for test and/or release packaging.')
