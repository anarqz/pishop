// Captures the landing-page screenshots by driving a running piShop instance
// (keyboard = controller) in headless Chrome at handheld resolutions.
//
//   PISHOP_PORT=47950 ./pishop --no-browser &        # an instance with data configured
//   node scripts/screenshots.mjs http://127.0.0.1:47950/ docs/assets/screens
//
// Optional env:
//   SHOTS_REPLACE  {"from": "to", …} text rewrites before each shot (a stand-in
//                  HOME back to /home/deck, a LAN address to a hostname).
//   SHOTS_LANG     en | pt: the app's language for the captures (default: as set).
//   SHOTS_DEMO=1   Transfers shows representative downloads of free, open-source
//                  games (Steam art and data), two of them installed; Games
//                  shows those and other non-Steam shortcuts (free games), one
//                  game's page, its components and patches; Settings → VPN
//                  shows demo connections; disks report a handheld's numbers
//                  (not the capture machine's). All served to the capture
//                  browser only: nothing is downloaded, installed or changed.
//   CHROME         path to a Chrome/Chromium binary.
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
const LANG = process.env.SHOTS_LANG

async function session(port) {
  const profile = mkdtempSync(join(tmpdir(), 'pishop-shots-'))
  const chrome = spawn(
    CHROME,
    [
      '--headless=new',
      `--remote-debugging-port=${port}`,
      `--user-data-dir=${profile}`,
      '--hide-scrollbars',
      'about:blank',
    ],
    { stdio: 'ignore' },
  )
  let targets
  for (let i = 0; i < 60 && !targets; i++) {
    await sleep(200)
    targets = await fetch(`http://127.0.0.1:${port}/json`).then(r => r.json()).catch(() => null)
  }
  const ws = new WebSocket(targets.find(t => t.type === 'page').webSocketDebuggerUrl)
  await new Promise(r => (ws.onopen = r))
  let id = 0
  const pending = new Map()
  const listeners = new Map()
  ws.onmessage = e => {
    const m = JSON.parse(e.data)
    if (m.method) listeners.get(m.method)?.(m.params)
    pending.get(m.id)?.(m)
    pending.delete(m.id)
  }
  const send = (method, params = {}) =>
    new Promise(r => {
      const i = ++id
      pending.set(i, r)
      ws.send(JSON.stringify({ id: i, method, params }))
    })
  const on = (method, fn) => listeners.set(method, fn)
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
  return { send, on, close }
}

// ---------- Transfers demo data (SHOTS_DEMO=1) ----------

// Free on Steam and open source. progress/state/speeds (MiB/s) are a snapshot.
const DEMO = [
  { appid: 1241950, release: 'Warzone 2100 4.5.5 (Windows)', indexer: 'TPB nativo', size: 412e6, progress: 0.64, state: 'live', down: 6.1, up: 0.4, peers: 31, files: 3 },
  { appid: 599390, release: 'Battle for Wesnoth 1.18.5', indexer: '1337x', size: 540e6, progress: 0.23, state: 'live', down: 3.4, up: 0.2, peers: 18, files: 1 },
  { appid: 334920, release: 'Zero-K v1.12.4.0', indexer: 'TPB nativo', size: 980e6, progress: 0.47, state: 'paused', down: 0, up: 0, peers: 0, files: 12 },
  { appid: 404410, release: 'Endless Sky v0.10.12', indexer: '1337x', size: 340e6, progress: 0, state: 'initializing', down: 0, up: 0, peers: 4, files: 1 },
  { appid: 412220, release: 'DDNet 18.9 (Windows)', indexer: 'TPB nativo', size: 86e6, progress: 1, state: 'live', down: 0, up: 0.18, peers: 6, files: 214 },
  { appid: 380840, release: 'Teeworlds 0.7.5', indexer: '1337x', size: 32e6, progress: 1, state: 'paused', down: 0, up: 0, peers: 0, files: 87 },
]

const steamLang = () => (LANG === 'pt' ? 'brazilian' : 'english')

async function steamGame(appid) {
  const input = encodeURIComponent(
    JSON.stringify({ ids: [{ appid }], context: { language: steamLang(), country_code: 'BR' }, data_request: { include_assets: true } }),
  )
  const [info, item] = await Promise.all([
    fetch(`https://store.steampowered.com/api/appdetails?appids=${appid}&l=${steamLang()}&cc=br`)
      .then(r => r.json())
      .then(j => j[appid]?.data ?? {})
      .catch(() => ({})),
    fetch(`https://api.steampowered.com/IStoreBrowseService/GetItems/v1/?input_json=${input}`)
      .then(r => r.json())
      .then(j => j.response?.store_items?.[0] ?? {})
      .catch(() => ({})),
  ])
  const a = item.assets ?? {}
  const asset = f => (f && a.asset_url_format ? `https://shared.akamai.steamstatic.com/store_item_assets/${a.asset_url_format.replace('${FILENAME}', f)}` : null)
  const release_date = info.release_date?.date ?? null
  return {
    name: info.name ?? item.name ?? String(appid),
    cover: asset(a.library_capsule),
    hero: asset(a.library_hero),
    year: Number(/\d{4}/.exec(release_date ?? '')?.[0]) || null,
    platform: 'PC',
    steam_appid: String(appid),
    overview: info.short_description ?? null,
    genres: (info.genres ?? []).map(g => g.description),
    developers: info.developers ?? [],
    publishers: info.publishers ?? [],
    release_date,
    sources: ['Steam', 'SteamGridDB'],
  }
}

/** The demo download shown as installed (DDraceNetwork): its install page,
 * its page in Games, Components and Patches, as the launcher would answer on
 * a Steam Deck. Teeworlds is installed too (Games shows both). */
const INSTALLED = 412220
const ALSO_INSTALLED = 380840
/** Steam shortcut appids of the demo games (any 32-bit id with the top bit set). */
const SHORTCUT_IDS = { 412220: 3141592653, 380840: 2718281828 }

/** Other non-Steam shortcuts in Games: free games on Steam (for their art). */
const OTHERS = [
  { appid: 1118310, id: 2847193650, name: 'RetroArch', windows: false, played: 1 },
  { appid: 1536610, id: 3012456789, name: 'OpenTTD', windows: true, played: 4 },
  { appid: 967460, id: 2593817264, name: 'Red Eclipse 2', windows: false, played: 12 },
]

/** Steam CDN images through the instance's image proxy, as the app shows them. */
const proxied = u => (u ? `/api/catalog/img?u=${encodeURIComponent(u)}` : null)

function demoInstall(row, downloadDir) {
  const pt = LANG === 'pt'
  const appid = 3141592653
  const pfx = `/home/deck/.local/share/Steam/steamapps/compatdata/${appid}/pfx`
  const dir = `${pfx}/drive_c/Program Files/DDNet`
  const exe = `${dir}/DDNet.exe`
  const internal = pt ? 'Armazenamento interno' : 'Internal storage'
  const state = {
    stage: 'installed',
    appid,
    tool: 'proton_11',
    target: '/run/media/deck/SN01T/piShop/DDNet',
    installer: `${downloadDir}/${row.release}/setup.exe`,
    exe,
    started: 0,
    runner: 'steam',
    game_dir: dir,
  }
  const tools = [
    { name: 'proton_11', display: 'Proton 11.0-2', installed: true },
    { name: 'proton_experimental', display: 'Proton Experimental', installed: true },
    { name: 'proton_10', display: 'Proton 10.0-3', installed: true },
    { name: 'proton_9', display: 'Proton 9.0-4', installed: false },
  ]
  const libraries = [
    { path: '/home/deck/.local/share/Steam', label: internal, free: 261e9, total: 1.9e12 },
    { path: '/run/media/deck/SN01T', label: 'SN01T', free: 395e9, total: 1e12 },
  ]
  const steam = [
    ['vcredist/2022', 'Visual C++ 2022', false],
    ['vcredist/2019', 'Visual C++ 2019', false],
    ['vcredist/2017', 'Visual C++ 2017', false],
    ['vcredist/2015', 'Visual C++ 2015', false],
    ['vcredist/2013', 'Visual C++ 2013', true],
    ['vcredist/2012', 'Visual C++ 2012', false],
    ['vcredist/2010', 'Visual C++ 2010', true],
    ['vcredist/2008', 'Visual C++ 2008', false],
    ['DirectX/Jun2010', 'DirectX (June 2010)', true],
    ['DotNet/4.8', '.NET Framework 4.8', false],
    ['PhysX/9.12.1031', 'PhysX 9.12.1031', false],
  ]
  const offline = ['vcrun2022', 'd3dx9', 'xact', 'xinput', 'd3dx11_43', 'vcrun2013', 'vcrun2012', 'vcrun2010', 'vcrun2008']
  const verbs = ['vcrun2022', 'd3dx9', 'xact', 'xinput', 'd3dx11_43', 'd3dcompiler_47', 'vcrun2013', 'vcrun2012', 'vcrun2010', 'vcrun2008',
    'vcrun2005', 'dotnet48', 'dotnetdesktop8', 'xna40', 'physx', 'openal', 'corefonts']
  const win = p => p.replaceAll('/', '\\')
  return {
    state,
    options: {
      game: row.game,
      content: [`${downloadDir}/${row.release}`],
      download_dir: `${downloadDir}/${row.release}`,
      installers: [],
      portable: [],
      libraries,
      default_library: libraries[1].path,
      tools,
      default_tool: 'proton_11',
      steam_api: true,
      debugging_enabled: true,
      state,
    },
    info: {
      appid,
      steam_api: true,
      shortcut: { exe: `"${exe}"`, start_dir: `"${dir}"`, launch_options: 'WINEDLLOVERRIDES="winmm=n,b" %command%', tool: 'proton_11' },
      tool: 'proton_11',
      tools,
      exe: { path: exe, windows: win('C:/Program Files/DDNet/DDNet.exe') },
      game_dir: { path: dir, windows: win('C:/Program Files/DDNet'), size: 141e6, disk: internal, free: 261e9, total: 1.9e12 },
      prefix: { path: pfx, exists: true, disk: internal, free: 261e9, total: 1.9e12 },
      in_prefix: true,
      targets: [
        { id: 'prefix', kind: 'prefix', label: pt ? 'Dentro do prefixo' : 'Inside its prefix', to: dir, windows: win('C:/Program Files/DDNet'), here: true, same_disk: true, free: 261e9, total: 1.9e12, disk: internal, blocked: null },
        { id: libraries[0].path, kind: 'library', label: internal, to: `${libraries[0].path}/piShop/DDNet`, windows: win('Z:/home/deck/.local/share/Steam/piShop/DDNet'), here: false, same_disk: true, free: 261e9, total: 1.9e12, disk: internal, blocked: null },
        { id: libraries[1].path, kind: 'library', label: 'SN01T', to: '/run/media/deck/SN01T/piShop/DDNet', windows: win('D:/piShop/DDNet'), here: false, same_disk: false, free: 395e9, total: 1e12, disk: 'SN01T', blocked: null },
      ],
      moving: null,
      borrowed: null,
      running: false,
      busy: false,
      is_download: false,
      download_dir: null,
      external: false,
      stage: 'installed',
      name: row.game.name,
    },
    patches: {
      launch_options: 'WINEDLLOVERRIDES="winmm=n,b" %command%',
      patches: [
        {
          id: 'winmm',
          title: pt ? 'Sobrescrever o winmm' : 'Override winmm',
          about: pt
            ? 'Usado em alguns repacks da FitGirl: o winmm.dll do próprio jogo carrega antes do do Wine. Adiciona WINEDLLOVERRIDES="winmm=n,b" %command% às opções de inicialização.'
            : 'Used in some FitGirl repacks: the game\'s own winmm.dll loads before Wine\'s. Adds WINEDLLOVERRIDES="winmm=n,b" %command% to the launch options.',
          applied: true,
        },
      ],
      busy: false,
    },
    components: {
      available: true,
      prefix: true,
      steam: steam.map(([id, title, installed]) => ({ id, title, installed })),
      verbs: verbs.map(verb => ({ verb, offline: offline.includes(verb), installed: verb === 'd3dx9' })),
      running: false,
      current: '',
      log: [],
      exit: null,
    },
  }
}

/** Mocked engine + library responses for the Transfers screen. */
async function demoData(downloadDir) {
  const now = Math.floor(Date.now() / 1000)
  const rows = await Promise.all(
    DEMO.map(async (d, i) => ({ ...d, id: i + 1, hash: d.appid.toString(16).padStart(8, '0').repeat(5), game: await steamGame(d.appid) })),
  )
  const torrents = {
    torrents: rows.map(r => ({
      id: r.id,
      info_hash: r.hash,
      name: r.release,
      output_folder: downloadDir,
      stats: {
        state: r.state,
        total_bytes: r.size,
        progress_bytes: Math.round(r.size * r.progress),
        finished: r.progress >= 1,
        error: null,
        live:
          r.state === 'live'
            ? { download_speed: { mbps: r.down }, upload_speed: { mbps: r.up }, snapshot: { peer_stats: { live: r.peers } } }
            : null,
      },
    })),
  }
  const stats = {
    download_speed: { mbps: rows.reduce((s, r) => s + r.down, 0) },
    upload_speed: { mbps: rows.reduce((s, r) => s + r.up, 0) },
    peers: { live: rows.reduce((s, r) => s + r.peers, 0) },
  }
  const files = Object.fromEntries(
    rows.map(r => [r.id, { files: Array.from({ length: r.files }, (_, k) => ({ name: `file-${k}`, length: Math.round(r.size / r.files) })) }]),
  )
  const installed = rows.find(r => r.appid === INSTALLED)
  const also = rows.find(r => r.appid === ALSO_INSTALLED)
  const install = demoInstall(installed, downloadDir)
  const alsoState = { ...install.state, appid: SHORTCUT_IDS[ALSO_INSTALLED], tool: 'proton_11', exe: null, game_dir: null, started: -1 }
  const library = Object.fromEntries(
    rows.map((r, i) => [
      r.hash,
      {
        info_hash: r.hash,
        game: r.game,
        release: r.release,
        indexer: r.indexer,
        size: r.size,
        dest: null,
        added: now - i * 600,
        resolved: true,
        ...(r === installed ? { install: install.state } : r === also ? { install: alsoState } : {}),
      },
    ]),
  )
  // Games: the two installs, then other shortcuts (free games with Steam art).
  const others = await Promise.all(OTHERS.map(async o => ({ ...o, game: await steamGame(o.appid) })))
  const tools = install.options.tools.map(t => ({ name: t.name, display: t.display }))
  const item = (o, extra) => ({
    appid: o.id,
    key: null,
    name: o.name,
    origin: 'shortcut',
    stage: null,
    exe: o.windows ? `/home/deck/Games/${o.name}/${o.name}.exe` : `/home/deck/Applications/${o.name}.AppImage`,
    tool: o.windows ? 'proton_experimental' : '',
    windows: o.windows,
    prefix: o.windows,
    running: false,
    last_played: o.played ? now - o.played * 86400 : 0,
    missing: false,
    art: { cover: proxied(o.game.cover), wide: null, hero: proxied(o.game.hero), logo: null },
    game: null,
    ...extra,
  })
  const games = {
    steam_api: true,
    tools,
    games: [
      item(
        { id: SHORTCUT_IDS[INSTALLED], name: installed.game.name, windows: true, played: 0.2, game: installed.game },
        { key: installed.hash, origin: 'pishop', stage: 'installed', tool: 'proton_11', game: installed.game },
      ),
      item(
        { id: SHORTCUT_IDS[ALSO_INSTALLED], name: also.game.name, windows: true, played: 3, game: also.game },
        { key: also.hash, origin: 'pishop', stage: 'installed', tool: 'proton_11', game: also.game },
      ),
      ...others.map(o => item(o)),
    ],
  }
  const keys = { [SHORTCUT_IDS[INSTALLED]]: installed.hash, [SHORTCUT_IDS[ALSO_INSTALLED]]: also.hash }
  return { torrents, stats, files, library, install, installed: { id: installed.id, hash: installed.hash }, games, keys }
}

async function capture(device, port) {
  const { send, on, close } = await session(port)
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

  /** Waits until a page expression is truthy (true if it got there). */
  const waitFor = async (expr, ms = 10000) => {
    for (let t = 0; t < ms; t += 250) {
      if (await val(`!!(${expr})`)) return true
      await sleep(250)
    }
    return false
  }
  /** Opens a tab by clicking it (sturdier than counting R1 presses). */
  const tab = async re => {
    await val(`[...document.querySelectorAll('.tabs .tab')].find(t => ${re}.test(t.textContent.trim()))?.click()`)
    await sleep(600)
  }

  if (LANG) {
    await fetch(new URL('/api/settings', base), { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ lang: LANG }) })
  }
  // One interception handler for the whole capture: with SHOTS_DEMO, answers
  // about disks (/api/local/*) get a handheld's numbers instead of the capture
  // machine's; requests go to the current demo mock, if any.
  let mock = null
  const DISK = { urlPattern: '*/api/local/*', requestStage: 'Response' }
  const intercept = (...patterns) => send('Fetch.enable', { patterns: [...(process.env.SHOTS_DEMO ? [DISK] : []), ...patterns] })
  const internal = LANG === 'en' ? 'Internal storage' : 'Armazenamento interno'
  const asHandheld = o => {
    if (o && typeof o === 'object' && ('free' in o || 'total' in o)) {
      o.free = 261e9
      o.total = 1.9e12
      if ('disk' in o) o.disk = internal
    }
  }
  on('Fetch.requestPaused', async params => {
    const { requestId } = params
    if (params.responseStatusCode !== undefined) {
      const r = (await send('Fetch.getResponseBody', { requestId })).result ?? {}
      let body = r.base64Encoded ? Buffer.from(r.body, 'base64').toString() : (r.body ?? '')
      try {
        const j = JSON.parse(body)
        if (Array.isArray(j)) j.forEach(asHandheld)
        else asHandheld(j)
        body = JSON.stringify(j)
      } catch {
        // not JSON: as it came
      }
      void send('Fetch.fulfillRequest', {
        requestId,
        responseCode: params.responseStatusCode,
        responseHeaders: params.responseHeaders,
        body: Buffer.from(body).toString('base64'),
      })
      return
    }
    if (mock) mock(params)
    else void send('Fetch.continueRequest', { requestId })
  })
  if (process.env.SHOTS_DEMO) await intercept()
  await send('Emulation.setFocusEmulationEnabled', { enabled: true })
  await send('Emulation.setDeviceMetricsOverride', { width: device.width, height: device.height, deviceScaleFactor: 1, mobile: false })
  await send('Page.navigate', { url: base })
  await sleep(9000)
  await shot('discover')

  await key('ArrowDown', 2) // search bar → first game of "Cracks recentes"
  await sleep(2500)
  await shot('discover-grid')
  await val(`document.querySelector('.dsc-scroll').scrollTop = 0; document.querySelector('.dsc-hero .btn.primary')?.focus()`)
  await sleep(800)

  await key('Enter') // "Ver jogo" on the featured game
  await sleep(8000)
  await shot('game')
  await key('Enter') // "Search torrents"
  await sleep(10000)
  await shot('store')
  await key('Enter') // first release: its details, with the matched game's art
  await sleep(3500)
  await shot('release')
  // The download dialog: Downloads or a folder picked by hand, with their free space.
  const downloadButton = `[...document.querySelectorAll('button')].find(b => /^(Download|Baixar)$/.test(b.textContent.trim()))`
  if (await waitFor(downloadButton, 15000)) {
    await val(`${downloadButton}.click()`)
    if (await waitFor(`document.querySelector('.dialog .scard')`, 8000)) {
      await sleep(1500)
      await shot('download')
      await key('Escape')
      await waitFor(`!document.querySelector('.dialog .scard')`, 3000)
    }
  }
  await sleep(500)
  if (await val(`!!document.querySelector('.details-bg')`)) await key('Escape')
  await sleep(800)

  if (process.env.SHOTS_DEMO) {
    const info = await val(`fetch('/api/info').then(r => r.json())`)
    const demo = await demoData(info.download_dir)
    const h = demo.installed.hash
    mock = ({ requestId, request }) => {
      const p = new URL(request.url).pathname
      const body =
        p === '/api/games'
          ? demo.games
          : /^\/api\/games\/\d+\/manage$/.test(p)
            ? { key: demo.keys[p.split('/')[3]] ?? h }
            : /^\/api\/games\/\d+\/patches/.test(p)
              ? demo.install.patches
              : p === '/api/library'
                ? demo.library
                : p === `/api/install/${h}`
                  ? demo.install.options
                  : p === `/api/install/${h}/info`
                    ? demo.install.info
                    : p === `/api/install/${h}/components`
                      ? demo.install.components
                      : p.startsWith('/api/install/')
                        ? {}
                        : p === '/api/archive/inspect'
                          ? { archives: [], exe_count: 0, has_installer: false }
                          : p.startsWith('/api/archive/')
                            ? []
                            : p === '/torrents'
                              ? demo.torrents
                              : p === '/stats'
                                ? demo.stats
                                : (demo.files[p.split('/')[2]] ?? {})
      void send('Fetch.fulfillRequest', {
        requestId,
        responseCode: 200,
        responseHeaders: [
          { name: 'content-type', value: 'application/json' },
          { name: 'access-control-allow-origin', value: '*' },
        ],
        body: Buffer.from(JSON.stringify(body)).toString('base64'),
      })
    }
    await intercept(
      { urlPattern: `${info.torrent_api}/*` },
      { urlPattern: '*/api/library' },
      { urlPattern: '*/api/install/*' },
      { urlPattern: '*/api/archive/*' },
      { urlPattern: '*/api/games' },
      { urlPattern: '*/api/games/*' },
    )
    await val(`window.__demoInstalled = ${demo.installed.id}`)
  }
  await tab('/^(Transfers|Transferências)/')
  await waitFor(`document.querySelector('.dl-tile')`, 10000)
  await sleep(5000)
  await shot('transfers')
  if (process.env.SHOTS_DEMO) {
    // The installed demo game: A opens its page (installed: Games, the download).
    await val(`document.querySelector('[data-torrent-id="' + window.__demoInstalled + '"]')?.focus()`)
    await sleep(600)
    await key('Enter')
    await waitFor(`document.querySelector('.inst .inst-card')`, 10000)
    await sleep(3000)
    await shot('install')
    await key('Escape')
    await sleep(800)
  }

  await tab('/^(Games|Jogos)/')
  await waitFor(`document.querySelector('.game-tile')`, 10000)
  await sleep(4000)
  await shot('games')
  if (process.env.SHOTS_DEMO) {
    await val(`document.querySelector('.game-tile')?.focus()`)
    await key('y') // Patches of the first game
    await waitFor(`document.querySelector('.aside .patch')`, 8000)
    await sleep(1500)
    await shot('patches')
    await key('Escape')
    await sleep(800)
    await val(`document.querySelector('.game-tile')?.focus()`)
    await key('Enter') // its page
    await waitFor(`document.querySelector('.inst .scard')`, 10000)
    await sleep(2500)
    await shot('game-page')
    await val(`[...document.querySelectorAll('.inst .inst-row')].find(r => /Components|Componentes/.test(r.textContent))?.focus()`)
    await sleep(300)
    await key('Enter')
    await sleep(2500)
    await shot('components')
    await key('Escape')
    await sleep(500)
    await key('Escape')
    await sleep(800)
    mock = null
    await intercept()
  }

  // Explore: the NAS on the left, EmuDeck's ROMs on the right (Go to…).
  const menu = async item => {
    await key('m')
    await sleep(700)
    await val(`[...document.querySelectorAll('.menu-item')].find(b => ${item}.test(b.textContent))?.click()`)
    await sleep(900)
  }
  await tab('/^(Explore|Explorar)/')
  await sleep(2500)
  await menu('/Go to|Ir para/')
  await val(`[...document.querySelectorAll('.dest-option')].find(b => /NAS/.test(b.textContent))?.click()`)
  await sleep(3000)
  await key('ArrowRight')
  await menu('/Go to|Ir para/')
  await val(`[...document.querySelectorAll('.scard')].find(b => /ROMs/.test(b.textContent))?.click()`)
  await sleep(2000)
  await key('ArrowLeft')
  await menu('/Search|Buscar/')
  await type('Super Nintendo')
  await sleep(2500)
  await key('ArrowDown', 3)
  await key('x', 3)
  await key('ArrowDown')
  await sleep(600)
  await shot('explorer')

  await tab('/^(Settings|Configurações)/')
  await sleep(2000)
  await shot('settings')

  // Settings → VPN: demo connections (a NetworkManager answer, as on SteamOS).
  if (process.env.SHOTS_DEMO) {
    const conns = {
      available: true,
      connections: [
        { uuid: 'a1', name: 'br-sao-01', kind: 'wireguard', state: 'on', device: 'pswg0', ip: '10.66.12.34', needs_login: false },
        { uuid: 'b2', name: 'nl-ams-02', kind: 'openvpn', state: 'off', device: null, ip: null, needs_login: false },
        { uuid: 'c3', name: 'us-nyc-03', kind: 'wireguard', state: 'off', device: null, ip: null, needs_login: false },
      ],
    }
    mock = ({ requestId }) =>
      void send('Fetch.fulfillRequest', {
        requestId,
        responseCode: 200,
        responseHeaders: [{ name: 'content-type', value: 'application/json' }],
        body: Buffer.from(JSON.stringify(conns)).toString('base64'),
      })
    await intercept({ urlPattern: '*/api/vpn' })
  }
  await val(`document.querySelectorAll('.settings-tab')[3]?.focus()`)
  await sleep(3000)
  await shot('vpn')
  if (process.env.SHOTS_DEMO) {
    mock = null
    await send('Fetch.disable')
  }
  await close()
}

let port = 9450
for (const d of DEVICES) {
  console.log(`${d.id} ${d.width}×${d.height}`)
  await capture(d, port++)
}
