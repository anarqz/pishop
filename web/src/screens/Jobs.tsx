// Transfers, styled after Steam's Downloads page: torrent downloads as a shelf
// of game covers (the focused one detailed on top), then network copies.

import { useEffect, useMemo, useRef, useState } from 'react'
import { type ArtJob, type Job, api, formatBytes, formatEta } from '../api'
import { tr, trn } from '../i18n'
import { focusFirst, input, keepFocus } from '../input'
import { type TorrentInfo, addMagnet, pause, resume } from '../torrent'
import { Icon, Progress, TextPrompt, toast, useHints } from '../ui'
import InstallPage from './Install'
import {
  DeleteDialog, DownloadHero, DownloadTile, sortDownloads, toggleLabel, useEngineState, useLibrary,
} from './Torrents'

const ACTIVE = ['running', 'scanning'] as const

type Focus = { kind: 'job' | 'torrent'; id: number } | null

/** X: pause/resume while downloading, stop/resume sharing once finished. */
function toggle(t: TorrentInfo) {
  const p = t.error || t.state === 'paused' ? resume(t.id) : pause(t.id)
  p.catch(e => toast(tr('Action failed'), String((e as Error).message), 'error'))
}

export default function Jobs({
  jobs,
  refresh,
  downloadDir,
  onExplore,
}: {
  jobs: Job[]
  refresh: () => void
  downloadDir?: string
  /** Opens a local folder in Explore (left pane), to copy it anywhere. */
  onExplore?: (path: string) => void
}) {
  const current = jobs.find(j => (ACTIVE as readonly string[]).includes(j.status))
  const queued = jobs.filter(j => j.status === 'queued')
  const history = jobs
    .filter(j => ['done', 'failed', 'canceled'].includes(j.status))
    .sort((a, b) => (b.finished ?? 0) - (a.finished ?? 0))

  const engine = useEngineState()
  const lib = useLibrary(engine.torrents)
  const downloads = useMemo(() => sortDownloads(engine.torrents, lib), [engine.torrents, lib])
  const [dialog, setDialog] = useState<{ kind: 'details' | 'delete'; id: number } | null>(null)
  const [prompt, setPrompt] = useState(false)
  // Refreshing the Steam artwork of every installed game.
  const installedCount = Object.values(lib).filter(e => e?.install?.stage === 'installed' && e.install.appid).length
  const [art, setArt] = useState<ArtJob | null>(null)
  useEffect(() => {
    if (!art?.running) return
    const id = setInterval(() => {
      api
        .libraryArtworkStatus()
        .then(j => {
          setArt(j)
          if (!j.running) toast(tr('Artwork updated'), trn(j.done, '{n} game', '{n} games'), 'ok')
        })
        .catch(() => {})
    }, 1500)
    return () => clearInterval(id)
  }, [art?.running])
  const restoreFocus = useRef<(() => void) | null>(null)
  const [cap, setCap] = useState<number | null>(null)
  useEffect(() => {
    api
      .torrentLimits()
      .then(l => setCap(l.download_bps))
      .catch(() => {})
  }, [])

  // What the controller is on: a download or a copy job (by data attribute).
  const [focus, setFocus] = useState<Focus>(null)
  const [heroId, setHeroId] = useState<number | null>(null)
  const focusedJob = focus?.kind === 'job' ? (jobs.find(j => j.id === focus.id) ?? null) : null
  const focusedTorrent = focus?.kind === 'torrent' ? (downloads.find(t => t.id === focus.id) ?? null) : null
  const heroT = downloads.find(t => t.id === heroId) ?? downloads[0] ?? null
  const dialogT = dialog ? (downloads.find(t => t.id === dialog.id) ?? null) : null

  // The torrent list arrives just after the tab opens: land on the first game
  // then, unless the controller already moved somewhere else.
  const landed = useRef(false)
  useEffect(() => {
    if (landed.current || !downloads.length) return
    landed.current = true
    const el = document.activeElement as HTMLElement | null
    if (!el || el === document.body || el.closest('.dl-head')) requestAnimationFrame(focusFirst)
  }, [downloads.length])

  const openDialog = (kind: 'details' | 'delete', id: number) => {
    restoreFocus.current = keepFocus()
    setDialog({ kind, id })
  }
  const closeDialog = () => {
    setDialog(null)
    restoreFocus.current?.()
  }

  const act = async (j: Job, action: 'cancel' | 'retry' | 'remove') => {
    try {
      await api.jobAction(j.id, action)
      refresh()
    } catch (e) {
      toast(tr('Action failed'), String((e as Error).message), 'error')
    }
  }

  const ref = useRef({ focusedJob, focusedTorrent, act, history, openDialog })
  ref.current = { focusedJob, focusedTorrent, act, history, openDialog }
  useEffect(
    () =>
      input.pushHandler(a => {
        const { focusedJob: j, focusedTorrent: t, act, history, openDialog } = ref.current
        if (t && a === 'x') {
          toggle(t)
          return true
        }
        if (t && a === 'y') {
          openDialog('delete', t.id)
          return true
        }
        if (j && a === 'x') {
          void act(j, ['queued', 'running', 'scanning'].includes(j.status) ? 'cancel' : 'remove')
          return true
        }
        if (j && a === 'y' && ['failed', 'canceled'].includes(j.status)) {
          void act(j, 'retry')
          return true
        }
        if (a === 'menu' && history.length) {
          void api.clearJobs().then(refresh)
          return true
        }
        return false
      }),
    [refresh],
  )

  const jobActive = focusedJob && ['queued', 'running', 'scanning'].includes(focusedJob.status)
  useHints(
    dialog || prompt
      ? null
      : [
          ...(focusedTorrent
            ? [
                { glyph: 'A' as const, label: tr('View details') },
                { glyph: 'X' as const, label: toggleLabel(focusedTorrent) },
                { glyph: 'Y' as const, label: tr('Delete') },
              ]
            : []),
          ...(focusedJob ? [{ glyph: 'X' as const, label: jobActive ? tr('Cancel') : tr('Remove') }] : []),
          ...(focusedJob && ['failed', 'canceled'].includes(focusedJob.status) ? [{ glyph: 'Y' as const, label: tr('Retry') }] : []),
          ...(history.length ? [{ glyph: 'MENU' as const, label: tr('Clear history') }] : []),
          { glyph: 'B', label: tr('Back') },
        ],
  )

  const row = (j: Job, big = false) => <JobRow key={j.id} job={j} big={big} />

  return (
    <div
      className="jobs transfers"
      data-nav-scope
      onFocusCapture={e => {
        const d = (e.target as HTMLElement).dataset
        if (d.torrentId) setHeroId(Number(d.torrentId))
        setFocus(d.jobId ? { kind: 'job', id: Number(d.jobId) } : d.torrentId ? { kind: 'torrent', id: Number(d.torrentId) } : null)
      }}
    >
      {heroT && <DownloadHero t={heroT} entry={lib[heroT.infoHash]} />}
      <div className="transfers-scroll">
        <section className="dl-section">
          <div className="dl-head">
            <h3 className="section-title">{tr('Downloads')}</h3>
            <span className="dl-head-stats">
              {engine.online ? (
                <>
                  ↓ {formatBytes(engine.downloadSpeed)}/s{cap ? ` (${tr('limit {speed}/s', { speed: formatBytes(cap) })})` : ''} · ↑{' '}
                  {formatBytes(engine.uploadSpeed)}/s · {trn(engine.peers, '{n} peer', '{n} peers')}
                  {downloadDir ? ` · ${downloadDir.replace(/^\/home\/[^/]+/, '~')}` : ''}
                </>
              ) : (
                tr('Starting the torrent engine…')
              )}
            </span>
            {installedCount > 0 && (
              <button
                data-nav
                className="btn small"
                disabled={!!art?.running}
                onClick={() =>
                  void api
                    .libraryArtwork()
                    .then(setArt)
                    .catch(e => toast(tr('Something went wrong'), (e as Error).message, 'error'))
                }
              >
                <Icon name="display" size={18} />{' '}
                {art?.running ? tr('Artwork {done}/{total}…', { done: art.done, total: art.total }) : tr('Refresh artwork')}
              </button>
            )}
            <button data-nav data-nav-default={downloads.length ? undefined : ''} className="btn small" onClick={() => setPrompt(true)}>
              <Icon name="plus" size={18} /> {tr('Magnet link')}
            </button>
          </div>
          {engine.online && !downloads.length && (
            <div className="jobs-empty">
              <Icon name="download" size={28} />
              <span>{tr('No downloads. Find games in Discover or the Store.')}</span>
            </div>
          )}
          <div className="dl-grid">
            {downloads.map((t, i) => (
              <DownloadTile key={t.id} t={t} entry={lib[t.infoHash]} first={i === 0} onOpen={() => openDialog('details', t.id)} />
            ))}
          </div>
        </section>

        {jobs.length > 0 && <h3 className="section-title jobs-title">{tr('Copies')}</h3>}
        {current && <section>{row(current, true)}</section>}
        {queued.length > 0 && (
          <section>
            <h3 className="section-title">{tr('Queued · {n}', { n: queued.length })}</h3>
            {queued.map(j => row(j))}
          </section>
        )}
        {history.length > 0 && (
          <section>
            <h3 className="section-title">{tr('Completed')}</h3>
            {history.map(j => row(j))}
          </section>
        )}
      </div>

      {dialog?.kind === 'details' && dialogT && (
        <InstallPage t={dialogT} entry={lib[dialogT.infoHash]} onClose={closeDialog} onExplore={onExplore} />
      )}
      {dialog?.kind === 'delete' && dialogT && <DeleteDialog t={dialogT} entry={lib[dialogT.infoHash]} onClose={closeDialog} />}
      {prompt && (
        <TextPrompt
          title={tr('Magnet link')}
          placeholder="magnet:?xt=urn:btih:…"
          submitLabel={tr('Add')}
          validate={v => v.startsWith('magnet:')}
          onCancel={() => setPrompt(false)}
          onSubmit={m => {
            setPrompt(false)
            addMagnet(m).catch(e => toast(tr("Couldn't add the magnet link"), String((e as Error).message), 'error'))
          }}
        />
      )}
    </div>
  )
}

function JobRow({ job: j, big }: { job: Job; big: boolean }) {
  const frac = j.total_bytes ? j.done_bytes / j.total_bytes : j.status === 'done' ? 1 : 0
  const eta = j.speed > 0 ? formatEta((j.total_bytes - j.done_bytes) / j.speed) : ''
  const status = (() => {
    switch (j.status) {
      case 'scanning':
        return trn(j.files_total, 'Scanning… {n} file, {size}', 'Scanning… {n} files, {size}', { size: formatBytes(j.total_bytes) })
      case 'running':
        return (
          tr('{done} of {total}', { done: formatBytes(j.done_bytes), total: formatBytes(j.total_bytes) }) +
          (eta ? ` · ${tr('{eta} left', { eta })}` : '')
        )
      case 'queued':
        return tr('Queued')
      case 'done':
        return `${tr('Complete')} · ${formatBytes(j.total_bytes)} · ${trn(j.files_total, '{n} file', '{n} files')}`
      case 'failed':
        return tr('Failed: {error}', { error: j.error ?? tr('unknown error') })
      case 'canceled':
        return tr('Canceled')
    }
  })()
  const dest = j.dest_dir.replace(/^\/home\/[^/]+/, '~')
  return (
    <div data-nav data-job-id={j.id} tabIndex={0} className={`job ${big ? 'big' : ''} job-${j.status}`}>
      <div className="job-icon">
        <Icon name={j.dir ? 'folder' : 'file'} size={big ? 34 : 24} />
      </div>
      <div className="job-main">
        <div className="job-line">
          <b className="job-name">{j.name}</b>
          {j.status === 'running' && <span className="job-speed">{formatBytes(j.speed)}/s</span>}
        </div>
        <div className="job-sub">
          {j.source_name} → {dest}
        </div>
        {['running', 'scanning', 'done', 'failed'].includes(j.status) && (
          <Progress value={frac} state={j.status === 'done' ? 'done' : j.status === 'failed' ? 'error' : undefined} />
        )}
        <div className={`job-status ${j.status === 'failed' ? 'error' : ''}`}>
          <span>{status}</span>
          {big && j.status === 'running' && (
            <span className="muted">
              {j.files_done}/{j.files_total} · {j.current}
            </span>
          )}
        </div>
      </div>
    </div>
  )
}
