// Captures the landing-page screenshots by driving a running piShop instance
// (keyboard = controller) in headless Chrome at handheld resolutions.
//
//   PISHOP_PORT=47950 ./pishop --no-browser &        # an instance with data configured
//   node scripts/screenshots.mjs http://127.0.0.1:47950/ docs/assets/screens
//
// Optional env: SHOTS_JOBS (see Transfers below), SHOTS_REPLACE (text rewrites),
// CHROME (path to a Chrome/Chromium binary).
//
// Output: <out>/<device>/<screen>.webp for ROG Ally (1920×1080) and Steam Deck (1280×800).

import { spawn } from 'node:child_process'
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

const [, , base = 'http://127.0.0.1:47950/', out = 'docs/assets/screens'] = process.argv
const CHROME = process.env.CHROME ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'
const DEVICES = [
  { id: 'ally', width: 1920, height: 1080 },
  { id: 'deck', width: 1280, height: 800 },
]
const sleep = ms => new Promise(r => setTimeout(r, ms))

async function session(port) {
  const profile = mkdtempSync(join(tmpdir(), 'pishop-shots-'))
  const chrome = spawn(CHROME, ['--headless=new', `--remote-debugging-port=${port}`, `--user-data-dir=${profile}`, '--hide-scrollbars', 'about:blank'], {
    stdio: 'ignore',
  })
  let targets
  for (let i = 0; i < 60 && !targets; i++) {
    await sleep(200)
    targets = await fetch(`http://127.0.0.1:${port}/json`).then(r => r.json()).catch(() => null)
  }
  const ws = new WebSocket(targets.find(t => t.type === 'page').webSocketDebuggerUrl)
  await new Promise(r => (ws.onopen = r))
  let id = 0
  const pending = new Map()
  ws.onmessage = e => {
    const m = JSON.parse(e.data)
    pending.get(m.id)?.(m)
    pending.delete(m.id)
  }
  const send = (method, params = {}) =>
    new Promise(r => {
      const i = ++id
      pending.set(i, r)
      ws.send(JSON.stringify({ id: i, method, params }))
    })
  const close = async () => {
    ws.close()
    chrome.kill()
    await sleep(1500) // Chrome keeps writing its profile for a moment
    try {
      rmSync(profile, { recursive: true, force: true })
    } catch {
      // a leftover temp profile is harmless
    }
  }
  return { send, close }
}

async function capture(device, port) {
  const { send, close } = await session(port)
  const val = async e => (await send('Runtime.evaluate', { expression: e, awaitPromise: true, returnByValue: true })).result.result?.value
  const key = async (k, n = 1) => {
    for (let i = 0; i < n; i++) {
      await val(`window.dispatchEvent(new KeyboardEvent('keydown', { key: ${JSON.stringify(k)} }))`)
      await sleep(120)
    }
  }
  const type = async text => {
    await send('Input.insertText', { text })
    await sleep(100)
    await send('Input.dispatchKeyEvent', { type: 'keyDown', key: 'Enter', code: 'Enter', windowsVirtualKeyCode: 13 })
  }
  const dir = join(out, device.id)
  mkdirSync(dir, { recursive: true })
  const shot = async name => {
    // SHOTS_REPLACE = {"from": "to", …}: rewrites visible text before each shot,
    // e.g. a stand-in HOME back to /home/deck or a LAN address to a hostname.
    if (process.env.SHOTS_REPLACE) {
      await val(`(() => {
        const pairs = Object.entries(${process.env.SHOTS_REPLACE})
        const w = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT)
        for (let n; (n = w.nextNode()); )
          for (const [from, to] of pairs) if (n.nodeValue.includes(from)) n.nodeValue = n.nodeValue.split(from).join(to)
      })()`)
    }
    const r = await send('Page.captureScreenshot', { format: 'webp', quality: 86 })
    writeFileSync(join(dir, `${name}.webp`), Buffer.from(r.result.data, 'base64'))
    console.log(`  ${device.id}/${name}.webp`)
  }

  await send('Emulation.setFocusEmulationEnabled', { enabled: true })
  await send('Emulation.setDeviceMetricsOverride', { width: device.width, height: device.height, deviceScaleFactor: 1, mobile: false })
  await send('Page.navigate', { url: base })
  await sleep(9000)
  await shot('discover')

  await key('ArrowDown', 2)
  await sleep(1800)
  await shot('discover-rows')
  // Back to the featured hero's "Ver jogo" (wherever the rows left focus).
  await val(`document.querySelector('.dsc-scroll').scrollTop = 0; document.querySelector('.dsc-hero .btn.primary')?.focus()`)
  await sleep(800)

  await key('Enter') // "Ver jogo" on the featured hero
  await sleep(8000)
  await shot('game')
  await key('ArrowUp') // slide up to the trailer
  await sleep(5000)
  await shot('trailer')
  await key('ArrowDown')
  await sleep(1200)
  await key('Enter') // "Buscar torrents"
  await sleep(9000)
  await shot('store')

  await key('e') // R1 → Explorar
  await sleep(2500)
  await key('m') // search in the source pane
  await sleep(500)
  await type('Super Nintendo')
  await sleep(600)
  await key('Enter')
  await sleep(2500)
  await key('ArrowDown', 3)
  await key('x', 3)
  await key('ArrowDown')
  await sleep(600)
  await shot('explorer')

  // Optional real activity for the Transfers screen: SHOTS_JOBS is the body
  // of POST /api/jobs ({ source_id, dest, items }); cancelled right after.
  if (process.env.SHOTS_JOBS) {
    await val(`fetch('/api/jobs', { method: 'POST', headers: { 'content-type': 'application/json' }, body: ${JSON.stringify(process.env.SHOTS_JOBS)} })`)
  }
  await key('e') // R1 → Transferências
  await sleep(7000)
  await shot('transfers')
  if (process.env.SHOTS_JOBS) {
    await val(`fetch('/api/jobs').then(r => r.json()).then(js => Promise.all(js.filter(j => ['queued', 'scanning', 'running'].includes(j.status)).map(j => fetch('/api/jobs/' + j.id + '/cancel', { method: 'POST' }))))`)
    await sleep(1500)
    await val(`fetch('/api/jobs/clear', { method: 'POST' })`)
  }

  await key('q', 3) // L1 ×3 → Início
  await sleep(2500)
  await shot('home')

  await key('e', 4) // → Configurações
  await sleep(2000)
  await shot('settings')
  await close()
}

let port = 9450
for (const d of DEVICES) {
  console.log(`${d.id} ${d.width}×${d.height}`)
  await capture(d, port++)
}
