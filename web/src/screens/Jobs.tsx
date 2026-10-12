// Transfers, styled after Steam's Downloads page: shelves of game covers (the
// focused one detailed on top) — community installs (each game with its
// parts), torrents, direct downloads added by hand — then network copies.

import { useEffect, useMemo, useRef, useState } from 'react'
import { type CommunityInstall, type DirectDownload, type Job, api, formatBytes, formatEta } from '../api'
import { ownedTorrents, useTransfersExtra } from '../community'
import { tr, trn } from '../i18n'
import { focusFirst, input, keepFocus } from '../input'
import { type TorrentInfo, addMagnet, pause, resume } from '../torrent'
import { Icon, Progress, TextPrompt, toast, useHints } from '../ui'
import CommunityInstallPage from './CommunityInstall'
import InstallPage from './Install'
import {
  DeleteDialog, DownloadHero, DownloadTile, sortDownloads, toggleLabel, useEngineState, useLibrary,
} from './Torrents'
import { CommunityHero, CommunityRemoveDialog, CommunityTile, DirectDialog, DirectHero, DirectTile } from './TransferTiles'

const ACTIVE = ['running', 'scanning'] as const

type Kind = 'job' | 'torrent' | 'community' | 'direct'
type Focus = { kind: 'job' | 'torrent' | 'direct'; id: number } | { kind: 'community'; id: string } | null
type DialogState =
  | { kind: 'details' | 'delete'; id: number }
  | { kind: 'community' | 'community-remove'; id: string }
  | { kind: 'direct'; id: number }
  | null

/** X on a community install: pause or resume its downloads. */
function toggleInstall(i: CommunityInstall) {
  if (i.stage !== 'downloading') return
  api.communityAction(i.id, i.paused ? 'resume' : 'pause').catch(e => toast(tr('Action failed'), String((e as Error).message), 'error'))
}

/** X on a direct download: pause, or resume (also after an error). */
function toggleDirect(d: DirectDownload) {
  if (d.state === 'done') return
  api.directAction(d.id, d.state === 'running' ? 'pause' : 'resume').catch(e => toast(tr('Action failed'), String((e as Error).message), 'error'))
}

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
  onOpenGame,
}: {
  jobs: Job[]
  refresh: () => void
  downloadDir?: string
  /** Opens a local folder in Explore (left pane). */
  onExplore?: (path: string) => void
  /** Games, on an installed game's page. */
  onOpenGame?: (appid: number) => void
}) {
  const current = jobs.find(j => (ACTIVE as readonly string[]).includes(j.status))
  const queued = jobs.filter(j => j.status === 'queued')
  const history = jobs
    .filter(j => ['done', 'failed', 'canceled'].includes(j.status))
    .sort((a, b) => (b.finished ?? 0) - (a.finished ?? 0))

  const engine = useEngineState()
  const extra = useTransfersExtra(1000)
  const owned = useMemo(() => ownedTorrents(extra.installs), [extra.installs])
  // A community install's torrents show inside its game, not on their own.
  const torrents = useMemo(() => engine.torrents.filter(t => !owned.has(t.infoHash)), [engine.torrents, owned])
  const lib = useLibrary(torrents)
  const downloads = useMemo(() => sortDownloads(torrents, lib), [torrents, lib])
  const installs = extra.installs
  const directs = useMemo(() => extra.directs.filter(d => !d.owner).sort((a, b) => b.added - a.added), [extra.directs])
  const [dialog, setDialog] = useState<DialogState>(null)
  const [prompt, setPrompt] = useState(false)
  const [linkBusy, setLinkBusy] = useState(false)
  const [linkError, setLinkError] = useState<string | null>(null)
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
  const [hero, setHero] = useState<{ kind: Exclude<Kind, 'job'>; id: number | string } | null>(null)
  const focusedJob = focus?.kind === 'job' ? (jobs.find(j => j.id === focus.id) ?? null) : null
  const focusedTorrent = focus?.kind === 'torrent' ? (downloads.find(t => t.id === focus.id) ?? null) : null
  const focusedInstall = focus?.kind === 'community' ? (installs.find(i => i.id === focus.id) ?? null) : null
  const focusedDirect = focus?.kind === 'direct' ? (directs.find(d => d.id === focus.id) ?? null) : null
  // The panel on top: the focused transfer, else the first one there is.
  const heroInstall = hero?.kind === 'community' ? installs.find(i => i.id === hero.id) : undefined
  const heroT = hero?.kind === 'torrent' ? downloads.find(t => t.id === hero.id) : undefined
  const heroDirect = hero?.kind === 'direct' ? directs.find(d => d.id === hero.id) : undefined
  const top = heroInstall
    ? ({ kind: 'community', i: heroInstall } as const)
    : heroT
      ? ({ kind: 'torrent', t: heroT } as const)
      : heroDirect
        ? ({ kind: 'direct', d: heroDirect } as const)
        : installs[0]
          ? ({ kind: 'community', i: installs[0] } as const)
          : downloads[0]
            ? ({ kind: 'torrent', t: downloads[0] } as const)
            : directs[0]
              ? ({ kind: 'direct', d: directs[0] } as const)
              : null
  const dialogT = dialog && (dialog.kind === 'details' || dialog.kind === 'delete') ? (downloads.find(t => t.id === dialog.id) ?? null) : null
  const dialogInstall = dialog && (dialog.kind === 'community' || dialog.kind === 'community-remove') ? (installs.find(i => i.id === dialog.id) ?? null) : null
  const dialogDirect = dialog?.kind === 'direct' ? (directs.find(d => d.id === dialog.id) ?? null) : null
  const count = installs.length + downloads.length + directs.length

  // The lists arrive just after the tab opens: land on the first game then,
  // unless the controller already moved somewhere else.
  const landed = useRef(false)
  useEffect(() => {
    if (landed.current || !count) return
    landed.current = true
    const el = document.activeElement as HTMLElement | null
    if (!el || el === document.body || el.closest('.dl-head')) requestAnimationFrame(focusFirst)
  }, [count])

  const openDialog = (d: NonNullable<DialogState>) => {
    restoreFocus.current = keepFocus()
    setDialog(d)
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

  const ref = useRef({ focusedJob, focusedTorrent, focusedInstall, focusedDirect, act, history, openDialog })
  ref.current = { focusedJob, focusedTorrent, focusedInstall, focusedDirect, act, history, openDialog }
  useEffect(
    () =>
      input.pushHandler(a => {
        const { focusedJob: j, focusedTorrent: t, focusedInstall: ci, focusedDirect: d, act, history, openDialog } = ref.current
        if (t && a === 'x') {
          toggle(t)
          return true
        }
        if (t && a === 'y') {
          openDialog({ kind: 'delete', id: t.id })
          return true
        }
        if (ci && a === 'x') {
          toggleInstall(ci)
          return true
        }
        if (ci && a === 'y') {
          openDialog({ kind: 'community-remove', id: ci.id })
          return true
        }
        if (d && a === 'x') {
          toggleDirect(d)
          return true
        }
        if (d && a === 'y') {
          openDialog({ kind: 'direct', id: d.id })
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
          ...(focusedInstall
            ? [
                { glyph: 'A' as const, label: tr('View details') },
                ...(focusedInstall.stage === 'downloading' ? [{ glyph: 'X' as const, label: focusedInstall.paused ? tr('Resume') : tr('Pause') }] : []),
                { glyph: 'Y' as const, label: focusedInstall.stage === 'installed' ? tr('Remove') : tr('Cancel installation') },
              ]
            : []),
          ...(focusedDirect
            ? [
                { glyph: 'A' as const, label: tr('Options') },
                ...(focusedDirect.state !== 'done' ? [{ glyph: 'X' as const, label: focusedDirect.state === 'running' ? tr('Pause') : tr('Resume') }] : []),
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
        if (d.torrentId) setHero({ kind: 'torrent', id: Number(d.torrentId) })
        if (d.communityId) setHero({ kind: 'community', id: d.communityId })
        if (d.directId) setHero({ kind: 'direct', id: Number(d.directId) })
        setFocus(
          d.jobId
            ? { kind: 'job', id: Number(d.jobId) }
            : d.torrentId
              ? { kind: 'torrent', id: Number(d.torrentId) }
              : d.communityId
                ? { kind: 'community', id: d.communityId }
                : d.directId
                  ? { kind: 'direct', id: Number(d.directId) }
                  : null,
        )
      }}
    >
      {top?.kind === 'torrent' && <DownloadHero t={top.t} entry={lib[top.t.infoHash]} />}
      {top?.kind === 'community' && <CommunityHero i={top.i} />}
      {top?.kind === 'direct' && <DirectHero d={top.d} />}
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
            <button data-nav data-nav-default={count ? undefined : ''} className="btn small" onClick={() => setPrompt(true)}>
              <Icon name="plus" size={18} /> {tr('Add link')}
            </button>
          </div>
          {engine.online && !count && (
            <div className="jobs-empty">
              <Icon name="download" size={28} />
              <span>{tr('No downloads. Find games in Discover, the Store or Community.')}</span>
            </div>
          )}
          {installs.length > 0 && (
            <>
              <h4 className="dl-kind">
                <Icon name="globe" size={16} /> {tr('Community')}
              </h4>
              <div className="dl-grid">
                {installs.map((i, n) => (
                  <CommunityTile key={i.id} i={i} first={n === 0} onOpen={() => openDialog({ kind: 'community', id: i.id })} />
                ))}
              </div>
            </>
          )}
          {downloads.length > 0 && (
            <>
              {(installs.length > 0 || directs.length > 0) && (
                <h4 className="dl-kind">
                  <Icon name="torrent" size={16} /> {tr('Torrents')}
                </h4>
              )}
              <div className="dl-grid">
                {downloads.map((t, i) => (
                  <DownloadTile
                    key={t.id}
                    t={t}
                    entry={lib[t.infoHash]}
                    first={i === 0 && !installs.length}
                    onOpen={() => openDialog({ kind: 'details', id: t.id })}
                  />
                ))}
              </div>
            </>
          )}
          {directs.length > 0 && (
            <>
              <h4 className="dl-kind">
                <Icon name="download" size={16} /> {tr('Direct downloads')}
              </h4>
              <div className="dl-grid">
                {directs.map((d, i) => (
                  <DirectTile
                    key={d.id}
                    d={d}
                    first={i === 0 && !installs.length && !downloads.length}
                    onOpen={() => openDialog({ kind: 'direct', id: d.id })}
                  />
                ))}
              </div>
            </>
          )}
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

      {dialog?.kind === 'community' && dialogInstall && (
        <CommunityInstallPage install={dialogInstall} onClose={closeDialog} onExplore={onExplore} onOpenGame={onOpenGame} />
      )}
      {dialog?.kind === 'community-remove' && dialogInstall && <CommunityRemoveDialog i={dialogInstall} onClose={closeDialog} />}
      {dialog?.kind === 'direct' && dialogDirect && <DirectDialog d={dialogDirect} onClose={closeDialog} onExplore={onExplore} />}
      {dialog?.kind === 'details' && dialogT && (
        <InstallPage t={dialogT} entry={lib[dialogT.infoHash]} onClose={closeDialog} onExplore={onExplore} onOpenGame={onOpenGame} />
      )}
      {dialog?.kind === 'delete' && dialogT && <DeleteDialog t={dialogT} entry={lib[dialogT.infoHash]} onClose={closeDialog} />}
      {prompt && (
        <TextPrompt
          title={tr('Add link')}
          placeholder="magnet:?xt=urn:btih:…  ·  https://…"
          hint={tr('A magnet link downloads as a torrent; a web link downloads the file directly.')}
          submitLabel={tr('Add')}
          busy={linkBusy}
          error={linkError}
          validate={v => v.startsWith('magnet:') || /^https?:\/\/\S+$/.test(v)}
          onCancel={() => {
            setPrompt(false)
            setLinkError(null)
          }}
          onSubmit={link => {
            setLinkBusy(true)
            setLinkError(null)
            const add = link.startsWith('magnet:') ? addMagnet(link) : api.addDirect(link)
            add
              .then(() => {
                setPrompt(false)
                toast(link.startsWith('magnet:') ? tr('Torrent added') : tr('Download added'), undefined, 'ok')
              })
              .catch(e => setLinkError(String((e as Error).message)))
              .finally(() => setLinkBusy(false))
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
