import { useCallback, useEffect, useRef, useState } from 'react'
import { type Job, type Place, type Source, api, formatBytes } from './api'
import { locale, setLang, tr, useLang } from './i18n'
import { focusFirst, input } from './input'
import Explorer, { type ExplorerRequest } from './screens/Explorer'
import Games, { type GamesRequest } from './screens/Games'
import Jobs from './screens/Jobs'
import Settings, { type SettingsSection } from './screens/Settings'
import Discover from './screens/Discover'
import Store, { type StoreRequest } from './screens/Store'
import { useEngineState } from './screens/Torrents'
import { setTorrentApi } from './torrent'
import { Dialog, Footer, Glyph, Spinner, Toasts, toast } from './ui'

export type Tab = 'discover' | 'store' | 'jobs' | 'games' | 'explorer' | 'settings'

// Labels are functions so they follow the current language. In the order a
// game goes: found, downloaded, installed; then files and settings.
const TABS: Array<[Tab, () => string]> = [
  ['discover', () => tr('Discover')],
  ['store', () => tr('Store')],
  ['jobs', () => tr('Transfers')],
  ['games', () => tr('Games')],
  ['explorer', () => tr('Explore')],
  ['settings', () => tr('Settings')],
]

interface ServerInfo {
  version: string
  addr: string
  torrent_api: string
  download_dir: string
}

function useClock() {
  const [now, setNow] = useState(() => new Date())
  useEffect(() => {
    const id = setInterval(() => setNow(new Date()), 10_000)
    return () => clearInterval(id)
  }, [])
  return now.toLocaleTimeString(locale(), { hour: '2-digit', minute: '2-digit' })
}

function useQuitProgress() {
  const [p, setP] = useState(0)
  useEffect(() => {
    let raf = 0
    const loop = () => {
      setP(input.quitProgress())
      raf = requestAnimationFrame(loop)
    }
    raf = requestAnimationFrame(loop)
    return () => cancelAnimationFrame(raf)
  }, [])
  return p
}

export default function App() {
  // Re-renders the whole tree when the language changes.
  useLang()
  const [tab, setTab] = useState<Tab>('discover')
  const [storeRequest, setStoreRequest] = useState<StoreRequest | null>(null)
  const [explorerRequest, setExplorerRequest] = useState<ExplorerRequest | null>(null)
  const [gamesRequest, setGamesRequest] = useState<GamesRequest | null>(null)
  const [sources, setSources] = useState<Source[]>([])
  const [places, setPlaces] = useState<Place[]>([])
  const [jobs, setJobs] = useState<Job[]>([])
  const [info, setInfo] = useState<ServerInfo | null>(null)
  const [quitting, setQuitting] = useState(false)
  const [askQuit, setAskQuit] = useState(false)
  const clock = useClock()
  const quitProgress = useQuitProgress()

  const loadSources = useCallback(() => void api.sources().then(setSources).catch(() => {}), [])
  const loadPlaces = useCallback(() => void api.places().then(setPlaces).catch(() => {}), [])
  const loadJobs = useCallback(() => void api.jobs().then(setJobs).catch(() => {}), [])

  // The language saved in the launcher (Settings → Language) wins over the cached one.
  useEffect(() => {
    api
      .settings()
      .then(s => setLang(s.lang))
      .catch(() => {})
  }, [])

  useEffect(() => {
    fetch('/api/info')
      .then(r => r.json())
      .then((j: ServerInfo) => {
        setInfo(j)
        if (j.torrent_api) setTorrentApi(j.torrent_api)
      })
      .catch(() => {})
    loadSources()
    loadPlaces()
  }, [loadSources, loadPlaces])

  // Jobs: poll fast while something is moving, slowly otherwise.
  const busy = jobs.some(j => ['running', 'scanning', 'queued'].includes(j.status))
  useEffect(() => {
    loadJobs()
    const id = setInterval(loadJobs, busy ? 700 : 3000)
    return () => clearInterval(id)
  }, [busy, loadJobs])

  // Free space changes as copies land; refresh places when the queue settles.
  const wasBusy = useRef(busy)
  useEffect(() => {
    if (wasBusy.current && !busy) loadPlaces()
    wasBusy.current = busy
  }, [busy, loadPlaces])

  const tabRef = useRef(tab)
  tabRef.current = tab
  const [settingsAt, setSettingsAt] = useState<SettingsSection | undefined>()
  const go = useCallback(
    (t: Tab) => {
      setTab(t)
      setSettingsAt(undefined)
      if (t === 'explorer' || t === 'store') loadPlaces()
    },
    [loadPlaces],
  )
  /** Settings, opened on the section that sets something up. */
  const goSettings = (s: SettingsSection) => {
    go('settings')
    setSettingsAt(s)
  }
  /** Explore, with a folder open on the left. */
  const explore = (path: string) => {
    setExplorerRequest({ path, n: Date.now() })
    go('explorer')
  }
  /** Games, on a game's page. */
  const openGame = (appid: number) => {
    setGamesRequest({ appid, n: Date.now() })
    go('games')
  }

  useEffect(() => {
    input.start()
    return input.onAction(a => {
      const i = TABS.findIndex(([t]) => t === tabRef.current)
      if (a === 'lb') go(TABS[(i - 1 + TABS.length) % TABS.length][0])
      else if (a === 'rb') go(TABS[(i + 1) % TABS.length][0])
      // B walks out: a tab's root goes back to Discover, Discover asks to quit.
      else if (a === 'back' && tabRef.current !== 'discover') go('discover')
      else if (a === 'back') setAskQuit(true)
      else if (a === 'quit') {
        setQuitting(true)
        void api.quit().catch(() => {})
      }
    })
  }, [go])

  // A new version finished downloading: say so once (it installs on the next start).
  useEffect(() => {
    let told = ''
    const tick = () =>
      api
        .update()
        .then(u => {
          if (u.state === 'ready' && u.latest && told !== u.latest) {
            told = u.latest
            toast(
              tr('piShop {v} is ready', { v: u.latest }),
              tr('It installs the next time piShop starts. To update now: Settings → About → Restart now.'),
              'ok',
            )
          }
        })
        .catch(() => {})
    const first = setTimeout(tick, 45_000)
    const id = setInterval(tick, 10 * 60_000)
    return () => {
      clearTimeout(first)
      clearInterval(id)
    }
  }, [])

  // Each tab change lands focus on that screen's default element.
  useEffect(() => {
    requestAnimationFrame(focusFirst)
  }, [tab])

  const current = jobs.find(j => j.status === 'running' || j.status === 'scanning')
  const engine = useEngineState(3000)
  const torrentsActive = engine.torrents.filter(t => !t.finished && t.state !== 'paused' && !t.error).length
  const active = jobs.filter(j => ['running', 'scanning', 'queued'].includes(j.status)).length + torrentsActive

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          <img src="/icon.svg" alt="" />
        </div>
        <nav className="tabs">
          <Glyph name="L1" />
          {TABS.map(([t, label]) => (
            <button key={t} className={`tab ${tab === t ? 'active' : ''}`} onClick={() => go(t)} tabIndex={-1}>
              {label()}
              {t === 'jobs' && active > 0 && <span className="badge">{active}</span>}
            </button>
          ))}
          <Glyph name="R1" />
        </nav>
        <div className="status">
          {current && (
            <button className="mini-job" onClick={() => go('jobs')} tabIndex={-1}>
              <Spinner />
              <span>{formatBytes(current.speed)}/s</span>
              <div className="mini-progress">
                <div style={{ width: `${current.total_bytes ? (current.done_bytes / current.total_bytes) * 100 : 0}%` }} />
              </div>
            </button>
          )}
          <span className="clock">{clock}</span>
        </div>
      </header>

      <main className="content" key={tab}>
        {tab === 'discover' && (
          <Discover
            onPick={game => {
              setStoreRequest({ q: game.title, kind: 'pc', n: Date.now(), game })
              go('store')
            }}
            onGoStore={() => go('store')}
            onGoSettings={() => goSettings('indexers')}
          />
        )}
        {tab === 'store' && <Store places={places} request={storeRequest} onGoSettings={() => goSettings('indexers')} />}
        {tab === 'explorer' && (
          <Explorer sources={sources} places={places} request={explorerRequest} onGoSettings={() => go('settings')} />
        )}
        {tab === 'jobs' && (
          <Jobs jobs={jobs} refresh={loadJobs} downloadDir={info?.download_dir} onExplore={explore} onOpenGame={openGame} />
        )}
        {tab === 'games' && <Games request={gamesRequest} onExplore={explore} />}
        {tab === 'settings' && <Settings sources={sources} reloadSources={loadSources} info={info} initialSection={settingsAt} />}
      </main>

      <Footer quitProgress={quitProgress} />
      <Toasts />

      {askQuit && !quitting && (
        <Dialog title={tr('Quit piShop?')} onClose={() => setAskQuit(false)}>
          {active > 0 && <p className="muted">{tr('Downloads stop until you open piShop again.')}</p>}
          <div className="row">
            <button
              data-nav
              data-nav-default
              className="btn primary"
              onClick={() => {
                setAskQuit(false)
                setQuitting(true)
                void api.quit().catch(() => {})
              }}
            >
              {tr('Quit')}
            </button>
            <button data-nav className="btn" onClick={() => setAskQuit(false)}>
              {tr('Cancel')}
            </button>
          </div>
        </Dialog>
      )}
      {quitting && (
        <div className="quitting">
          <Spinner />
          <p>{tr('Quitting…')}</p>
        </div>
      )}
    </div>
  )
}
