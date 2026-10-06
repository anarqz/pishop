// Transfers, styled after Steam's Downloads page: torrent downloads as a shelf
// of game covers (the focused one detailed on top), then network copies.

import { useEffect, useMemo, useRef, useState } from 'react'
import { type Job, api, formatBytes, formatEta } from '../api'
import { focusFirst, input, keepFocus } from '../input'
import { type TorrentInfo, addMagnet, pause, resume } from '../torrent'
import { Icon, Progress, TextPrompt, toast, useHints } from '../ui'
import {
  DeleteDialog, DetailsDialog, DownloadHero, DownloadTile, sortDownloads, toggleLabel, useEngineState, useLibrary,
} from './Torrents'

const ACTIVE = ['running', 'scanning'] as const

type Focus = { kind: 'job' | 'torrent'; id: number } | null

/** X: pause/resume while downloading, stop/resume sharing once finished. */
function toggle(t: TorrentInfo) {
  const p = t.error || t.state === 'paused' ? resume(t.id) : pause(t.id)
  p.catch(e => toast('Ação falhou', String((e as Error).message), 'error'))
}

export default function Jobs({ jobs, refresh, downloadDir }: { jobs: Job[]; refresh: () => void; downloadDir?: string }) {
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
      toast('Ação falhou', String((e as Error).message), 'error')
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
                { glyph: 'A' as const, label: 'Ver detalhes' },
                { glyph: 'X' as const, label: toggleLabel(focusedTorrent) },
                { glyph: 'Y' as const, label: 'Excluir' },
              ]
            : []),
          ...(focusedJob ? [{ glyph: 'X' as const, label: jobActive ? 'Cancelar' : 'Remover' }] : []),
          ...(focusedJob && ['failed', 'canceled'].includes(focusedJob.status) ? [{ glyph: 'Y' as const, label: 'Repetir' }] : []),
          ...(history.length ? [{ glyph: 'MENU' as const, label: 'Limpar histórico' }] : []),
          { glyph: 'B', label: 'Voltar' },
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
            <h3 className="section-title">Downloads</h3>
            <span className="dl-head-stats">
              {engine.online ? (
                <>
                  ↓ {formatBytes(engine.downloadSpeed)}/s{cap ? ` (limite ${formatBytes(cap)}/s)` : ''} · ↑{' '}
                  {formatBytes(engine.uploadSpeed)}/s · {engine.peers} peers
                  {downloadDir ? ` · ${downloadDir.replace(/^\/home\/[^/]+/, '~')}` : ''}
                </>
              ) : (
                'Iniciando o motor de torrents…'
              )}
            </span>
            <button data-nav data-nav-default={downloads.length ? undefined : ''} className="btn small" onClick={() => setPrompt(true)}>
              <Icon name="plus" size={18} /> Magnet link
            </button>
          </div>
          {engine.online && !downloads.length && (
            <div className="jobs-empty">
              <Icon name="download" size={28} />
              <span>Nenhum download. Encontre jogos em Descobrir ou na Loja.</span>
            </div>
          )}
          <div className="dl-grid">
            {downloads.map((t, i) => (
              <DownloadTile key={t.id} t={t} entry={lib[t.infoHash]} first={i === 0} onOpen={() => openDialog('details', t.id)} />
            ))}
          </div>
        </section>

        {jobs.length > 0 && <h3 className="section-title jobs-title">Cópias da rede</h3>}
        {current && <section>{row(current, true)}</section>}
        {queued.length > 0 && (
          <section>
            <h3 className="section-title">Na fila · {queued.length}</h3>
            {queued.map(j => row(j))}
          </section>
        )}
        {history.length > 0 && (
          <section>
            <h3 className="section-title">Concluídos</h3>
            {history.map(j => row(j))}
          </section>
        )}
      </div>

      {dialog?.kind === 'details' && dialogT && (
        <DetailsDialog t={dialogT} entry={lib[dialogT.infoHash]} onClose={closeDialog} />
      )}
      {dialog?.kind === 'delete' && dialogT && <DeleteDialog t={dialogT} entry={lib[dialogT.infoHash]} onClose={closeDialog} />}
      {prompt && (
        <TextPrompt
          title="Magnet link"
          placeholder="magnet:?xt=urn:btih:…"
          submitLabel="Adicionar"
          validate={v => v.startsWith('magnet:')}
          onCancel={() => setPrompt(false)}
          onSubmit={m => {
            setPrompt(false)
            addMagnet(m).catch(e => toast('Não foi possível adicionar', String((e as Error).message), 'error'))
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
        return `Analisando… ${j.files_total} arquivos, ${formatBytes(j.total_bytes)}`
      case 'running':
        return `${formatBytes(j.done_bytes)} de ${formatBytes(j.total_bytes)}${eta ? ` · ${eta} restantes` : ''}`
      case 'queued':
        return 'Na fila'
      case 'done':
        return `Concluído · ${formatBytes(j.total_bytes)} · ${j.files_total} ${j.files_total === 1 ? 'arquivo' : 'arquivos'}`
      case 'failed':
        return `Falhou: ${j.error ?? 'erro desconhecido'}`
      case 'canceled':
        return 'Cancelado'
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
