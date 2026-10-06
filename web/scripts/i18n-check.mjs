// Checks that every string passed to tr()/trn()/trb() has a Portuguese
// translation in src/locales/pt/*.json, and that areas don't disagree on one.
// Exit code 1 lists what's missing. Usage: node scripts/i18n-check.mjs [--unused]

import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join } from 'node:path'

const root = new URL('../src/', import.meta.url).pathname
const files = []
const walk = dir => {
  for (const f of readdirSync(dir)) {
    const p = join(dir, f)
    if (statSync(p).isDirectory()) walk(p)
    else if (/\.tsx?$/.test(f) && !p.includes('/locales/')) files.push(p)
  }
}
walk(root)

// A JS string literal ('…' or "…"), with escapes.
const LIT = String.raw`(['"])((?:\\.|(?!\1).)*)\1`
const unescape = s => s.replace(/\\(.)/g, (_, c) => ({ n: '\n', t: '\t' })[c] ?? c)
const used = new Map() // english → first file:line
const add = (s, file, src, index) => {
  if (!used.has(s)) used.set(s, `${file.replace(root, 'src/')}:${src.slice(0, index).split('\n').length}`)
}
const LIT2 = String.raw`(['"])((?:\\.|(?!\3).)*)\3`
// Examples in comments don't count.
const inComment = (src, index) => /^\s*(\/\/|\*|\/\*)/.test(src.slice(src.lastIndexOf('\n', index) + 1, index))
for (const file of files) {
  const src = readFileSync(file, 'utf8')
  for (const m of src.matchAll(new RegExp(String.raw`\btrb?\(\s*` + LIT, 'g'))) {
    if (!inComment(src, m.index)) add(unescape(m[2]), file, src, m.index)
  }
  const plural = new RegExp(String.raw`\btrn\(\s*[^,]+,\s*` + LIT + String.raw`\s*,\s*` + LIT2, 'g')
  for (const m of src.matchAll(plural)) {
    if (inComment(src, m.index)) continue
    add(unescape(m[2]), file, src, m.index)
    add(unescape(m[4]), file, src, m.index)
  }
}

const dir = join(root, 'locales/pt')
const pt = new Map() // english → [value, file]
const clashes = []
for (const f of readdirSync(dir).filter(f => f.endsWith('.json'))) {
  const dict = JSON.parse(readFileSync(join(dir, f), 'utf8'))
  for (const [k, v] of Object.entries(dict)) {
    const prev = pt.get(k)
    if (prev && prev[0] !== v) clashes.push(`  "${k}": "${prev[0]}" (${prev[1]}) ≠ "${v}" (${f})`)
    if (!prev) pt.set(k, [v, f])
  }
}

const missing = [...used].filter(([k]) => !pt.has(k))
let failed = false
if (missing.length) {
  failed = true
  console.error(`i18n: ${missing.length} string(s) without a pt translation:`)
  for (const [k, at] of missing) console.error(`  ${JSON.stringify(k)}  (${at})`)
}
if (clashes.length) {
  failed = true
  console.error('i18n: same English text translated differently (use distinct English strings):')
  for (const c of clashes) console.error(c)
}
if (process.argv.includes('--unused')) {
  const unused = [...pt.keys()].filter(k => !used.has(k))
  if (unused.length) console.log(`i18n: ${unused.length} unused pt entr(ies):\n` + unused.map(k => '  ' + JSON.stringify(k)).join('\n'))
}
if (failed) process.exit(1)
console.log(`i18n: ${used.size} strings, all translated.`)
