// A community game being installed (Transfers → A, or its page in Community):
// the four steps — get the parts, extract, run the install script, add to
// Steam — with each part's progress. Parts that aren't public wait for the
// user's own file; any downloadable part can also take a file the user
// already has (a finished one, or a partial download to continue).

import { type ReactNode, useEffect, useRef, useState } from 'react'
import { type CommunityInstall, type CommunityInstallPart, api, formatBytes, formatEta, imgUrl } from '../api'
import { partKind, stageLabel } from '../community'
import { tr, trb, trn } from '../i18n'
import { focusFirst, input, keepFocus } from '../input'
import { homePath } from '../storage'
import { Icon, Progress, Spinner, toast, useHints } from '../ui'
import FilePicker from './FilePicker'
import { CommunityRemoveDialog } from './TransferTiles'

const dirOf = (p: string) => p.replace(/\/[^/]+$/, '') || '/'
const ORDER = ['downloading', 'extracting', 'preparing', 'shortcut', 'installed'] as const
const pct = (f: number | null | undefined) => Math.floor(Math.max(0, Math.min(1, f ?? 0)) * 100)

type StepState = 'todo' | 'now' | 'done' | 'failed'

/** Where the user's files usually are: Downloads (three levels above the parts). */
export const pickStart = (downloads: string) => dirOf(dirOf(dirOf(downloads)))

export default function CommunityInstallPage({
  install,
  onClose,
  onExplore,
  onOpenGame,
}: {
  install: CommunityInstall
  onClose: () => void
  onExplore?: (path: string) => void
  onOpenGame?: (appid: number) => void
}) {
  const i = install
  const [busy, setBusy] = useState(false)
  const [pick, setPick] = useState<CommunityInstallPart | null>(null)
  const [remove, setRemove] = useState(false)
  const restore = useRef<(() => void) | null>(null)

  const run = async (f: () => Promise<unknown>, ok?: string) => {
    setBusy(true)
    try {
      await f()
      if (ok) toast(ok, i.name, 'ok')
    } catch (e) {
      toast(tr('Something went wrong'), (e as Error).message, 'error')
    } finally {
      setBusy(false)
    }
  }

  const modal = !!pick || remove
  const close = useRef(onClose)
  close.current = onClose
  useEffect(() => {
    if (modal) return
    return input.pushModal(() => close.current(), { tabs: true })
  }, [modal])
  const modalOpen = useRef(false)
  modalOpen.current = modal
  useEffect(() => {
    if (!modalOpen.current) requestAnimationFrame(focusFirst)
  }, [i.stage])

  const downloading = i.stage === 'downloading'
  const ctl = useRef({ i, downloading, run, modal })
  ctl.current = { i, downloading, run, modal }
  useEffect(
    () =>
      input.pushHandler(a => {
        const c = ctl.current
        if (c.modal) return false
        if (a === 'x' && c.downloading) {
          void c.run(() => api.communityAction(c.i.id, c.i.paused ? 'resume' : 'pause'))
          return true
        }
        if (a === 'y') {
          restore.current = keepFocus()
          setRemove(true)
          return true
        }
        return false
      }),
    [],
  )
  useHints(
    modal
      ? null
      : [
          { glyph: 'A', label: tr('Select') },
          ...(downloading ? [{ glyph: 'X' as const, label: i.paused ? tr('Resume') : tr('Pause') }] : []),
          { glyph: 'Y', label: i.stage === 'installed' ? tr('Remove') : tr('Cancel installation') },
          { glyph: 'B', label: tr('Back') },
        ],
  )

  const at = i.stage === 'failed' ? (i.failed_at ?? 'downloading') : i.stage
  const step = (s: (typeof ORDER)[number]): StepState => {
    const here = ORDER.indexOf(at as (typeof ORDER)[number])
    const mine = ORDER.indexOf(s)
    if (i.stage === 'failed' && s === at) return 'failed'
    if (i.stage === 'installed') return 'done'
    return mine < here ? 'done' : mine === here ? 'now' : 'todo'
  }
  const hasExtract = i.parts.some(p => p.extract != null)
  const g = i.game

  return (
    <div className="inst cmi" data-nav-scope>
      {g?.hero && <div className="inst-bg" style={{ backgroundImage: `url(${imgUrl(g.hero)})` }} />}
      <div className="inst-shade" />
      <div className="inst-content">
        <aside className="inst-side">
          <div className="inst-cover">{g?.cover ? <img src={imgUrl(g.cover)} alt="" draggable={false} /> : <Icon name="pad" size={48} />}</div>
          {onExplore && (
            <button data-nav className="btn" onClick={() => onExplore(i.downloads)}>
              <Icon name="download" /> {tr('Show the downloads')}
            </button>
          )}
          {onExplore && i.stage !== 'downloading' && (
            <button data-nav className="btn" onClick={() => onExplore(i.game_dir)}>
              <Icon name="folder" /> {tr('Show the game’s folder')}
            </button>
          )}
        </aside>
        <div className="inst-main">
          <span className="hero-kicker">
            {[tr('Community'), i.platform, i.wrapper ? tr('via {name}', { name: i.wrapper.name }) : null].filter(Boolean).join(' · ')}
          </span>
          <h1>{i.name}</h1>

          {i.stage === 'failed' && (
            <section className="inst-card cm-failed">
              <h3>
                <Icon name="info" /> {tr('The installation stopped')}
              </h3>
              <p className="inst-error">{i.error}</p>
              <div className="row">
                <button data-nav data-nav-default className="btn primary big" disabled={busy} onClick={() => void run(() => api.communityAction(i.id, 'retry'))}>
                  <Icon name="refresh" /> {tr('Try again')}
                </button>
              </div>
              <p className="muted small">{tr('Trying again picks up at the step that failed; finished downloads aren’t fetched again.')}</p>
            </section>
          )}

          {i.stage === 'installed' && (
            <section className="inst-card ok">
              <h3>
                <Icon name="check" /> {tr('Installed')}
              </h3>
              <p className="muted">
                {trb('**{name}** is in your Steam library and in Games, where its Proton, patches, files and artwork are managed.', { name: i.name })}
              </p>
              <div className="row">
                {i.appid && onOpenGame && (
                  <button data-nav data-nav-default className="btn primary big" onClick={() => onOpenGame(i.appid!)}>
                    <Icon name="pad" /> {tr('Open in Games')}
                  </button>
                )}
                <button data-nav className="btn danger" onClick={() => ((restore.current = keepFocus()), setRemove(true))}>
                  <Icon name="trash" /> {tr('Delete downloaded files…')}
                </button>
              </div>
            </section>
          )}

          <section className="inst-card cm-steps">
            <Step state={step('downloading')} title={tr('Get the parts')} detail={downloading ? stageLabel(i) : null}>
              <div className="inst-list">
                {i.parts.map((p, n) => (
                  <PartRow
                    key={p.id}
                    p={p}
                    first={n === 0 && i.stage === 'downloading'}
                    canChange={i.stage === 'downloading' || (i.stage === 'failed' && i.failed_at === 'downloading')}
                    onPick={() => {
                      restore.current = keepFocus()
                      setPick(p)
                    }}
                  />
                ))}
              </div>
              {downloading && (
                <div className="row">
                  <button data-nav className="btn" disabled={busy} onClick={() => void run(() => api.communityAction(i.id, i.paused ? 'resume' : 'pause'))}>
                    {i.paused ? tr('Resume') : tr('Pause')}
                  </button>
                  <span className="muted small">{homePath(i.downloads)}</span>
                </div>
              )}
            </Step>
            {hasExtract && (
              <Step state={step('extracting')} title={tr('Extract')} detail={i.stage === 'extracting' ? `${pct(i.progress)}%` : null}>
                {i.stage === 'extracting' && <Progress value={i.progress ?? 0} />}
              </Step>
            )}
            <Step state={step('preparing')} title={tr('Prepare the game')} detail={i.stage === 'preparing' ? (i.step ?? tr('Install script')) : null}>
              {i.stage === 'preparing' && i.progress != null && <Progress value={i.progress} />}
              {(i.stage === 'preparing' || (i.stage === 'failed' && i.failed_at === 'preparing')) && i.log.length > 0 && (
                <pre className="inst-log cm-log">{i.log.slice(-14).join('\n')}</pre>
              )}
              {i.stage !== 'preparing' && <p className="muted small">{trb('Runs the game’s install script in a sandbox: no network, only **{dir}** writable.', { dir: homePath(i.game_dir) })}</p>}
            </Step>
            <Step state={step('shortcut')} title={tr('Add to Steam')} detail={i.runtime === 'native' ? tr('Native Linux game') : i.tool ? i.tool.replace(/^proton_/, 'Proton ').replace(/_/g, ' ') : null} />
          </section>
        </div>
      </div>

      {pick && (
        <FilePicker
          title={pick.kind === 'user' ? tr('Pick {name}', { name: pick.name }) : tr('Your copy of {name}', { name: pick.name })}
          accept={pick.extensions.length ? pick.extensions : ['']}
          start={pickStart(i.downloads)}
          onClose={() => {
            setPick(null)
            restore.current?.()
          }}
          onPick={path => {
            const p = pick
            setPick(null)
            restore.current?.()
            void run(() => api.communityProvide(i.id, p.id, path), tr('File added'))
          }}
          onPickSource={src => {
            const p = pick
            setPick(null)
            restore.current?.()
            void run(() => api.communityProvideSource(i.id, p.id, src), tr('Copying to this device'))
          }}
        />
      )}
      {remove && (
        <CommunityRemoveDialog
          i={i}
          onClose={() => {
            setRemove(false)
            restore.current?.()
          }}
        />
      )}
    </div>
  )
}

function Step({ state, title, detail, children }: { state: StepState; title: string; detail?: string | null; children?: ReactNode }) {
  return (
    <div className={`cm-step ${state}`}>
      <span className="cm-step-dot">
        {state === 'done' ? <Icon name="check" size={16} /> : state === 'now' ? <Spinner /> : state === 'failed' ? '!' : null}
      </span>
      <div className="cm-step-body">
        <div className="cm-step-head">
          <b>{title}</b>
          {detail && <span className="muted">{detail}</span>}
        </div>
        {children}
      </div>
    </div>
  )
}

function PartRow({ p, first, canChange, onPick }: { p: CommunityInstallPart; first: boolean; canChange: boolean; onPick: () => void }) {
  const frac = p.total ? p.done / p.total : 0
  const status = (() => {
    switch (p.state) {
      case 'waiting':
        return p.hint || tr('Pick your own copy')
      case 'copying': {
        const of = p.total ? tr('{done} of {total}', { done: formatBytes(p.done), total: formatBytes(p.total) }) : formatBytes(p.done)
        return `${tr('Copying from {source}', { source: p.copying_from ?? '' })} · ${of}${p.speed > 0 ? ` · ${formatBytes(p.speed)}/s` : ''}`
      }
      case 'verifying':
        return tr('Checking… {pct}%', { pct: pct(p.verify) })
      case 'ready':
        return p.extract != null ? (p.extracted ? tr('Extracted') : tr('Ready to extract')) : tr('Ready')
      case 'failed':
        return p.error ?? tr('Download error')
      case 'downloading': {
        if (!p.total && !p.done) return tr('Starting…')
        const of = tr('{done} of {total}', { done: formatBytes(p.done), total: formatBytes(p.total) })
        if (p.paused) return `${tr('Paused')} · ${of}`
        const eta = p.speed > 0 && p.total ? formatEta((p.total - p.done) / p.speed) : ''
        const conns = p.connections > 1 ? ` · ${trn(p.connections, '{n} connection', '{n} connections')}` : ''
        return `${of} · ${formatBytes(p.speed)}/s${eta ? ` · ${eta}` : ''}${conns}`
      }
    }
  })()
  const action =
    p.state === 'waiting'
      ? tr('Pick the file…')
      : canChange && (p.state === 'downloading' || p.state === 'failed') && p.kind !== 'torrent'
        ? tr('Use a file I already have…')
        : null
  return (
    <div className={`cm-part ${p.state}`}>
      <Icon name={p.state === 'copying' ? 'network' : p.kind === 'user' || p.provided ? 'sd' : p.kind === 'torrent' ? 'torrent' : 'download'} />
      <span className="inst-row-main">
        <b>
          {p.name} <small className="cm-kind">{partKind(p)}</small>
        </b>
        <small className={p.state === 'failed' || (p.state === 'waiting' && p.error) ? 'error' : ''}>
          {p.state === 'waiting' && p.error ? p.error : status}
        </small>
        {p.provided && p.path && <small className="mono">{homePath(p.path)}</small>}
        {(p.state === 'downloading' || p.state === 'verifying' || p.state === 'copying') && (
          <Progress value={p.state === 'verifying' ? p.verify : frac} state={p.paused && p.state === 'downloading' ? 'paused' : undefined} />
        )}
      </span>
      {action ? (
        <button data-nav data-nav-default={first || p.state === 'waiting' ? '' : undefined} className={`btn small ${p.state === 'waiting' ? 'primary' : ''}`} onClick={onPick}>
          {action}
        </button>
      ) : (
        <span className="inst-row-side">{p.state === 'ready' ? <Icon name="check" size={18} /> : p.total ? formatBytes(p.total) : ''}</span>
      )}
    </div>
  )
}
