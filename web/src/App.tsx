import { useCallback, useEffect, useRef, useState } from 'react'
import { type Job, type Place, type Source, api, formatBytes } from './api'
import { focusFirst, input } from './input'
import Explorer from './screens/Explorer'
import Jobs from './screens/Jobs'
import Settings from './screens/Settings'
import Discover from './screens/Discover'
import Store, { type StoreRequest } from './screens/Store'
import { useEngineState } from './screens/Torrents'
import { setTorrentApi } from './torrent'
import { Footer, Glyph, Spinner, Toasts } from './ui'

export type Tab = 'discover' | 'store' | 'explorer' | 'jobs' | 'settings'

const TABS: Array<[Tab, string]> = [
  ['discover', 'Descobrir'],
  ['store', 'Loja'],
  ['explorer', 'Explorar'],
  ['jobs', 'Transferências'],
  ['settings', 'Configurações'],
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
  return now.toLocaleTimeString('pt-BR', { hour: '2-digit', minute: '2-digit' })
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
  const [tab, setTab] = useState<Tab>('discover')
  const [storeRequest, setStoreRequest] = useState<StoreRequest | null>(null)
  const [sources, setSources] = useState<Source[]>([])
  const [places, setPlaces] = useState<Place[]>([])
  const [jobs, setJobs] = useState<Job[]>([])
  const [info, setInfo] = useState<ServerInfo | null>(null)
  const [quitting, setQuitting] = useState(false)
  const clock = useClock()
  const quitProgress = useQuitProgress()

  const loadSources = useCallback(() => void api.sources().then(setSources).catch(() => {}), [])
  const loadPlaces = useCallback(() => void api.places().then(setPlaces).catch(() => {}), [])
  const loadJobs = useCallback(() => void api.jobs().then(setJobs).catch(() => {}), [])

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
  const go = useCallback(
    (t: Tab) => {
      setTab(t)
      if (t === 'explorer' || t === 'store') loadPlaces()
    },
    [loadPlaces],
  )

  useEffect(() => {
    input.start()
    return input.onAction(a => {
      const i = TABS.findIndex(([t]) => t === tabRef.current)
      if (a === 'lb') go(TABS[(i - 1 + TABS.length) % TABS.length][0])
      else if (a === 'rb') go(TABS[(i + 1) % TABS.length][0])
      else if (a === 'back' && tabRef.current !== 'discover') go('discover')
      else if (a === 'quit') {
        setQuitting(true)
        void api.quit().catch(() => {})
      }
    })
  }, [go])

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
              {label}
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
          />
        )}
        {tab === 'store' && <Store places={places} request={storeRequest} onGoSettings={() => go('settings')} />}
        {tab === 'explorer' && <Explorer sources={sources} places={places} onGoSettings={() => go('settings')} />}
        {tab === 'jobs' && <Jobs jobs={jobs} refresh={loadJobs} downloadDir={info?.download_dir} />}
        {tab === 'settings' && <Settings sources={sources} reloadSources={loadSources} info={info} />}
      </main>

      <Footer quitProgress={quitProgress} />
      <Toasts />

      {quitting && (
        <div className="quitting">
          <Spinner />
          <p>Saindo…</p>
        </div>
      )}
    </div>
  )
}
