// Transfer queue, styled after Steam's Downloads page: the active job on top,
// then the queue, then history.

import { useEffect, useRef, useState } from 'react'
import { type Job, api, formatBytes, formatEta } from '../api'
import { input } from '../input'
import { Icon, Progress, toast, useHints } from '../ui'
import { TorrentSection } from './Torrents'

const ACTIVE = ['running', 'scanning'] as const

export default function Jobs({ jobs, refresh, downloadDir }: { jobs: Job[]; refresh: () => void; downloadDir?: string }) {
  const current = jobs.find(j => (ACTIVE as readonly string[]).includes(j.status))
  const queued = jobs.filter(j => j.status === 'queued')
  const history = jobs
    .filter(j => ['done', 'failed', 'canceled'].includes(j.status))
    .sort((a, b) => (b.finished ?? 0) - (a.finished ?? 0))

  const [focusId, setFocusId] = useState<number | null>(null)
  const focused = jobs.find(j => j.id === focusId) ?? null

  const act = async (j: Job, action: 'cancel' | 'retry' | 'remove') => {
    try {
      await api.jobAction(j.id, action)
      refresh()
    } catch (e) {
      toast('Ação falhou', String((e as Error).message), 'error')
    }
  }

  const ref = useRef({ focused, act, history })
  ref.current = { focused, act, history }
  useEffect(
    () =>
      input.pushHandler(a => {
        const { focused, act, history } = ref.current
        if (a === 'x' && focused) {
          void act(focused, ['queued', 'running', 'scanning'].includes(focused.status) ? 'cancel' : 'remove')
          return true
        }
        if (a === 'y' && focused && ['failed', 'canceled'].includes(focused.status)) {
          void act(focused, 'retry')
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

  const isActive = focused && ['queued', 'running', 'scanning'].includes(focused.status)
  useHints([
    ...(focused ? [{ glyph: 'X' as const, label: isActive ? 'Cancelar' : 'Remover' }] : []),
    ...(focused && ['failed', 'canceled'].includes(focused.status) ? [{ glyph: 'Y' as const, label: 'Repetir' }] : []),
    ...(history.length ? [{ glyph: 'MENU' as const, label: 'Limpar histórico' }] : []),
    { glyph: 'B', label: 'Voltar' },
  ])

  const row = (j: Job, big = false) => (
    <JobRow key={j.id} job={j} big={big} onFocus={() => setFocusId(j.id)} onActivate={() => setFocusId(j.id)} />
  )

  return (
    <div className="jobs" data-nav-scope>
      <TorrentSection downloadDir={downloadDir} />
      {!jobs.length && (
        <div className="jobs-empty">
          <Icon name="network" size={28} />
          <span>Cópias da rede aparecem aqui — copie jogos no Explorar (Y).</span>
        </div>
      )}
      {current && (
        <section>
          <h3 className="section-title">Em andamento</h3>
          {row(current, true)}
        </section>
      )}
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
  )
}

function JobRow({ job: j, big, onFocus, onActivate }: { job: Job; big: boolean; onFocus: () => void; onActivate: () => void }) {
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
    <div
      data-nav
      tabIndex={0}
      className={`job ${big ? 'big' : ''} job-${j.status}`}
      onFocus={onFocus}
      onClick={onActivate}
    >
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
