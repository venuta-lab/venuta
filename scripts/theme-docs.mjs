// Writes the list of every theme value into docs/THEMES.md (between its VALUES markers), from
// the list the app itself checks themes against (app/src/lib/packFormat.ts), so the guide is
// never out of step with the code.
//
//   node scripts/theme-docs.mjs          write it
//   node scripts/theme-docs.mjs --check  fail if the guide is out of date (CI)

import { readFileSync, writeFileSync } from 'node:fs'
import { BUILTIN_FONTS, GROUPS, SCENES, TOKENS } from '../app/src/lib/packFormat.ts'
import { STYLES } from '../app/src/lib/themes.ts'

const KIND = {
  color: 'colour',
  paint: 'colour or gradient',
  image: 'gradient, or none',
  filter: 'filter, or none',
  backdrop: 'blur, or none',
  shadow: 'shadow, or none',
  length: 'size',
  opacity: 'number 0 to 1',
  weight: '100 to 900',
  tracking: 'letter spacing',
  case: 'none, uppercase, lowercase',
  font: 'font list',
}

const lines = []
for (const g of GROUPS) {
  lines.push(`### ${g}`, '', '| Value | Kind | What it changes |', '|---|---|---|')
  for (const t of TOKENS.filter((x) => x.group === g)) lines.push(`| \`${t.name}\` | ${KIND[t.type]} | ${t.label} |`)
  lines.push('')
}
lines.push('### Built-in themes to build on', '', '| `extends` | Theme |', '|---|---|')
for (const s of STYLES) lines.push(`| \`${s.id}\` | ${s.name}: ${s.line} |`)
lines.push('', '### Fonts you can name', '', BUILTIN_FONTS.map((f) => `\`builtin:${f}\``).join(', '), '')
lines.push('### Scenes', '', `\`base\` (the scene of the theme it extends), ${SCENES.map((s) => `\`${s}\``).join(', ')}.`, '')

const file = new URL('../docs/THEMES.md', import.meta.url)
const doc = readFileSync(file, 'utf8')
const start = '<!-- VALUES: written by scripts/theme-docs.mjs -->'
const end = '<!-- /VALUES -->'
const a = doc.indexOf(start)
const b = doc.indexOf(end)
if (a < 0 || b < 0) throw new Error('docs/THEMES.md has no VALUES markers')
const next = doc.slice(0, a + start.length) + '\n\n' + lines.join('\n') + '\n' + doc.slice(b)
if (process.argv.includes('--check')) {
  if (next !== doc) {
    console.error('docs/THEMES.md is out of date: run node scripts/theme-docs.mjs')
    process.exit(1)
  }
  console.log('docs/THEMES.md is up to date.')
} else {
  writeFileSync(file, next)
  console.log('Wrote the values into docs/THEMES.md.')
}
