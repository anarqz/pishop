#!/usr/bin/env node
// Builds community/index.json — the catalog piShop's Community tab reads —
// from community/<game>/manifest.json and each game's install script, and
// checks every manifest the way the launcher does (launcher/src/community.rs)
// plus the script policy in community/README.md.
//
//   node scripts/community.mjs           validate and write community/index.json
//   node scripts/community.mjs --check   validate; fail if index.json is out of date (CI)
//
// No dependencies: Node 18+.

import { createHash } from 'node:crypto'
import { existsSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'

const root = new URL('../community/', import.meta.url).pathname
const out = join(root, 'index.json')
const check = process.argv.includes('--check')

const errors = []
const warnings = []
const fail = (id, msg) => errors.push(`${id}: ${msg}`)
const warn = (id, msg) => warnings.push(`${id}: ${msg}`)

const SLUG = /^[a-z0-9_-]+$/
const sha256 = s => createHash('sha256').update(s).digest('hex')
/** A path inside the game's folder: relative, no `..`. */
const inside = p => typeof p === 'string' && !p.startsWith('/') && !p.split(/[\\/]/).includes('..')
const plainName = f => typeof f === 'string' && f.length > 0 && !/[\\/]/.test(f) && !f.startsWith('.')
const https = u => typeof u === 'string' && u.startsWith('https://')
const hex = (v, len) => (Array.isArray(v) ? v : [v]).every(x => typeof x === 'string' && new RegExp(`^[0-9a-fA-F]{${len}}$`).test(x))

// Things a community script never does (see community/README.md → Scripts).
// The sandbox already blocks most of them; a script that tries is rejected
// in review rather than failing on someone's device.
const POLICY = [
  [/\bsudo\b|\bpkexec\b|\bdoas\b/, 'asks for root (sudo/pkexec): scripts run as the user, in a sandbox'],
  [/\b(curl|wget|aria2c|nc|ncat|socat|ssh|scp|rsync)\b/, 'uses the network: declare every download as a part, piShop fetches it'],
  [/\/dev\/(tcp|udp)\//, 'opens a network socket'],
  [/\beval\b/, 'uses eval'],
  [/base64\s+(-d|--decode)|\bxxd\s+-r\b/, 'decodes embedded data: keep scripts readable'],
  [/\b(steam|steamos-session-select|systemctl|flatpak|pacman|rpm-ostree)\b/, 'touches Steam or the system: piShop makes the shortcut'],
  [/\brm\s+-[a-zA-Z]*r[a-zA-Z]*f?\s+(\/|~|\$HOME)(\s|$|\/)/, 'deletes outside the game folder'],
  [/~\/|\$HOME\b|\$\{HOME\}/, 'reaches into the home folder: only the game folder exists for the script'],
  [/\bchmod\s+(-R\s+)?777\b/, 'makes files world-writable'],
]

function lintScript(id, text) {
  if (!text.startsWith('#!/usr/bin/env bash') && !text.startsWith('#!/bin/bash')) fail(id, 'install script must start with #!/usr/bin/env bash')
  if (!/^set -euo pipefail$/m.test(text)) fail(id, 'install script must `set -euo pipefail`')
  text.split('\n').forEach((line, i) => {
    const code = line.replace(/(^|\s)#.*$/, '')
    for (const [re, why] of POLICY) if (re.test(code)) fail(id, `install.sh:${i + 1}: ${why}\n      ${line.trim()}`)
  })
  if (!/::step /.test(text)) warn(id, 'the script never says what it is doing (echo "::step …")')
}

function checkPart(id, p, seen) {
  const at = `part ${JSON.stringify(p?.id)}`
  if (!p || typeof p !== 'object') return fail(id, 'a part must be an object')
  if (!SLUG.test(p.id ?? '') || p.id.length > 32) fail(id, `${at}: id is lowercase letters, digits, - and _`)
  if (seen.has(p.id)) fail(id, `${at}: repeated id`)
  seen.add(p.id)
  if (!p.name) fail(id, `${at}: no name`)
  if (p.role && !['launcher', 'game', 'data', 'other'].includes(p.role)) fail(id, `${at}: role is launcher, game, data or other`)
  if (p.extract !== undefined && !inside(p.extract)) fail(id, `${at}: extract must stay inside the game folder`)
  if (p.updatable !== undefined && typeof p.updatable !== 'boolean') fail(id, `${at}: updatable is true or false`)
  if (p.updatable && !['github', 'itch'].includes(p.source?.type)) fail(id, `${at}: only GitHub and itch.io parts can be updatable (piShop reads their versions)`)
  if (p.size !== undefined && !(Number.isInteger(p.size) && p.size > 0)) fail(id, `${at}: size is a byte count`)
  const c = p.checksums ?? {}
  if (c.sha1 && !hex(c.sha1, 40)) fail(id, `${at}: sha1 must be 40 hex digits`)
  if (c.sha256 && !hex(c.sha256, 64)) fail(id, `${at}: sha256 must be 64 hex digits`)
  if (c.md5 && !hex(c.md5, 32)) fail(id, `${at}: md5 must be 32 hex digits`)
  const s = p.source ?? {}
  switch (s.type) {
    case 'http':
      if (!Array.isArray(s.urls) || !s.urls.length || !s.urls.every(https)) fail(id, `${at}: http needs https urls`)
      if (!plainName(s.file)) fail(id, `${at}: http needs a plain file name`)
      if (!p.size) warn(id, `${at}: no size (resuming and the space check work better with one)`)
      if (!c.sha1 && !c.sha256 && !c.md5) warn(id, `${at}: no checksum`)
      break
    case 'itch':
      if (!https(s.page) || !/\.itch\.io\//.test(s.page)) fail(id, `${at}: itch needs the game's https://<creator>.itch.io/<game> page`)
      if (!plainName(s.file)) fail(id, `${at}: itch needs the upload's file name`)
      break
    case 'github':
      if (!/^[\w.-]+\/[\w.-]+$/.test(s.repo ?? '')) fail(id, `${at}: github needs repo "owner/name"`)
      try {
        new RegExp(s.asset)
      } catch {
        fail(id, `${at}: github asset must be a regular expression`)
      }
      break
    case 'torrent':
      if (!(Array.isArray(s.urls) && s.urls.length) && !s.magnet) fail(id, `${at}: torrent needs .torrent urls or a magnet`)
      if (s.urls && !s.urls.every(https)) fail(id, `${at}: torrent urls must be https`)
      if (s.files) {
        try {
          new RegExp(s.files)
        } catch {
          fail(id, `${at}: torrent files must be a regular expression`)
        }
      }
      break
    case 'user':
      if (!s.hint) fail(id, `${at}: tell the user what to pick (hint)`)
      if (!c.sha1 && !c.sha256 && !c.md5) fail(id, `${at}: a file the user picks needs checksums of the accepted dumps`)
      if (s.extensions && !s.extensions.every(x => /^\.[a-z0-9]+$/.test(x))) fail(id, `${at}: extensions look like ".z64"`)
      break
    default:
      fail(id, `${at}: source.type is http, itch, github, torrent or user`)
  }
}

const games = []
const dirs = readdirSync(root).filter(d => statSync(join(root, d)).isDirectory()).sort()
for (const dir of dirs) {
  const file = join(root, dir, 'manifest.json')
  if (!existsSync(file)) {
    fail(dir, 'no manifest.json')
    continue
  }
  let m
  try {
    m = JSON.parse(readFileSync(file, 'utf8'))
  } catch (e) {
    fail(dir, `manifest.json isn't valid JSON: ${e.message}`)
    continue
  }
  const id = m.id
  if (id !== dir) fail(dir, `id ${JSON.stringify(id)} must match the folder name`)
  if (!SLUG.test(id ?? '') || id.length > 64) fail(dir, 'id is lowercase letters, digits, - and _')
  for (const k of ['name', 'platform', 'summary', 'description']) if (!m[k]) fail(id, `no ${k}`)
  if (!Number.isInteger(m.revision) || m.revision < 1) fail(id, 'revision is a whole number from 1 (bump it on every change)')
  if (!Array.isArray(m.authors) || !m.authors.length) fail(id, 'authors: who maintains it (GitHub users)')
  if (!Number.isInteger(m.art?.sgdb_id)) fail(id, 'art.sgdb_id: the game on SteamGridDB (artwork and its troubleshooting issues)')
  if (!Array.isArray(m.parts) || !m.parts.length) fail(id, 'no parts')
  const seen = new Set()
  for (const p of m.parts ?? []) checkPart(id, p, seen)
  if (!inside(m.launch?.exe) || !m.launch?.exe) fail(id, 'launch.exe must be a path inside the game folder')
  if (m.launch?.runtime && !['proton', 'native'].includes(m.launch.runtime)) fail(id, 'launch.runtime is proton or native')
  for (const p of m.patches ?? []) {
    if (!SLUG.test(p.id ?? '')) fail(id, `patch ${JSON.stringify(p.id)}: bad id`)
    if (!p.title) fail(id, `patch ${p.id}: no title`)
    if (!p.env || !Object.keys(p.env).length || !Object.keys(p.env).every(k => /^[A-Za-z_][A-Za-z0-9_]*$/.test(k)))
      fail(id, `patch ${p.id}: env is { "NAME": "value" }`)
  }
  const scriptName = m.install?.script
  if (!plainName(scriptName)) {
    fail(id, 'install.script: the script file next to manifest.json')
    continue
  }
  const scriptFile = join(root, dir, scriptName)
  if (!existsSync(scriptFile)) {
    fail(id, `${scriptName} not found`)
    continue
  }
  const source = readFileSync(scriptFile, 'utf8')
  lintScript(id, source)
  delete m.$schema
  games.push({ ...m, install: { ...m.install, source, sha256: sha256(source) } })
}

games.sort((a, b) => String(a.name).localeCompare(String(b.name)))
const index = { format: 1, games }
const text = `${JSON.stringify(index, null, 2)}\n`

for (const w of warnings) console.warn(`warning  ${w}`)
if (errors.length) {
  for (const e of errors) console.error(`error    ${e}`)
  console.error(`\n${errors.length} problem(s); see community/README.md`)
  process.exit(1)
}
if (check) {
  const current = existsSync(out) ? readFileSync(out, 'utf8') : ''
  if (current !== text) {
    console.error('community/index.json is out of date: run `node scripts/community.mjs` and commit it')
    process.exit(1)
  }
  console.log(`community: ${games.length} game(s), index up to date`)
} else {
  writeFileSync(out, text)
  console.log(`community: ${games.length} game(s) → community/index.json`)
}
