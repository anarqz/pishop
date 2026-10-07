// A download's install page (Transfers → A). Transfers only takes a download
// as far as its first install: compressed downloads are extracted, then the
// game's installer runs under Proton through a new Steam shortcut (which
// creates the game's prefix) and, once it closes, that shortcut is pointed at
// the game's executable. From then on the game lives in Games (Proton,
// components, moving it, artwork…); here the download's files can be deleted
// or shown in Explore. Games that need no installer go straight to Steam.

import { useCallback, useEffect, useRef, useState } from 'react'
import {
  type ArchiveJob, type ArchiveSet, type ExeCandidate, type InstallOptions, type InstallStatus, type LibraryEntry, api,
  formatBytes, imgUrl,
} from '../api'
import { tr, trb } from '../i18n'
import { focusFirst, input, keepFocus } from '../input'
import { StorageCard, homePath, useSpace } from '../storage'
import { type TorrentInfo, pause } from '../torrent'
import { Dialog, Icon, Progress, Spinner, toast, useHints } from '../ui'
import { CompatPanel, ProtonBadge } from './Compat'
import FilePicker from './FilePicker'
import { ChooseDialog, Choice } from './GameSetup'

const dirOf = (p: string) => p.replace(/\/[^/]+$/, '') || '/'
const baseName = (p: string) => p.split('/').pop() ?? p

export default function InstallPage({
  t,
  entry,
  onClose,
  onExplore,
  onOpenGame,
}: {
  t: TorrentInfo
  entry?: LibraryEntry
  onClose: () => void
  onExplore?: (path: string) => void
  /** Games, on this game's page. */
  onOpenGame?: (appid: number) => void
}) {
  const hash = t.infoHash
  const game = entry?.game
  const name = game?.name ?? t.name
  const [opts, setOpts] = useState<InstallOptions | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [archives, setArchives] = useState<ArchiveSet[]>([])
  const [jobs, setJobs] = useState<ArchiveJob[]>([])
  const [deleteAfter, setDeleteAfter] = useState(true)
  const [installer, setInstaller] = useState('')
  const [library, setLibrary] = useState('')
  const [tool, setTool] = useState('')
  const [status, setStatus] = useState<InstallStatus | null>(null)
  const [busy, setBusy] = useState(false)
  const [choose, setChoose] = useState<null | 'installer' | 'library' | 'tool'>(null)
  const [picker, setPicker] = useState<null | { start: string; then: 'finish' | 'portable' }>(null)
  const [compat, setCompat] = useState(false)
  const [wipe, setWipe] = useState(false)
  const restore = useRef<(() => void) | null>(null)

  const load = useCallback(async () => {
    try {
      const o = await api.installOptions(hash)
      setOpts(o)
      setInstaller(i => i || o.installers[0]?.path || '')
      setLibrary(l => l || o.state?.target.replace(/\/piShop\/[^/]+$/, '') || o.default_library || '')
      setTool(x => x || o.state?.tool || o.default_tool || '')
      setStatus(s => s ?? { state: o.state })
      const found = await Promise.all(o.content.map(p => api.archiveInspect(p).catch(() => null)))
      setArchives(found.flatMap(f => f?.archives ?? []))
      setError(null)
    } catch (e) {
      setError((e as Error).message)
    }
  }, [hash])
  useEffect(() => {
    void load()
  }, [load])

  // Archive jobs for this download, while any runs.
  const archivePaths = new Set(archives.map(a => a.path))
  const myJobs = jobs.filter(j => archivePaths.has(j.archive))
  const extracting = myJobs.find(j => j.state === 'running')
  useEffect(() => {
    if (!archives.length) return
    let alive = true
    const tick = () =>
      api
        .archiveJobs()
        .then(js => alive && setJobs(js))
        .catch(() => {})
    void tick()
    const id = setInterval(tick, 1000)
    return () => {
      alive = false
      clearInterval(id)
    }
  }, [archives.length])
  // A finished extraction changes what's on disk: look again.
  const doneCount = myJobs.filter(j => j.state === 'done').length
  useEffect(() => {
    if (doneCount) void load()
  }, [doneCount, load])

  // While installing, follow the installer and offer executables once it's gone.
  const stage = status?.state?.stage ?? 'ready'
  useEffect(() => {
    if (stage !== 'installing') return
    let alive = true
    const tick = () =>
      api
        .installStatus(hash)
        .then(s => alive && setStatus(s))
        .catch(() => {})
    void tick()
    const id = setInterval(tick, 2000)
    return () => {
      alive = false
      clearInterval(id)
    }
  }, [stage, hash])

  const modal = choose || picker || compat || wipe
  const close = useRef(onClose)
  close.current = onClose
  useEffect(() => {
    if (modal) return
    return input.pushModal(() => close.current(), { tabs: true })
  }, [modal])
  // Land on the step's main action when the step changes (closing a chooser
  // puts focus back on the row that opened it instead).
  const modalOpen = useRef(false)
  modalOpen.current = !!modal
  useEffect(() => {
    if (opts && !modalOpen.current) requestAnimationFrame(focusFirst)
  }, [opts, stage, archives.length, !!extracting])
  useHints(modal ? null : [{ glyph: 'A', label: tr('Select') }, { glyph: 'B', label: tr('Back') }])

  const run = async (f: () => Promise<unknown>, okTitle?: string) => {
    setBusy(true)
    try {
      await f()
      if (okTitle) toast(okTitle, name, 'ok')
    } catch (e) {
      toast(tr('Something went wrong'), (e as Error).message, 'error')
    } finally {
      setBusy(false)
    }
  }
  const open = (kind: 'installer' | 'library' | 'tool') => {
    restore.current = keepFocus()
    setChoose(kind)
  }
  const closeChooser = () => {
    setChoose(null)
    restore.current?.()
  }
  // Installing takes the download's files over: the transfer stops.
  const stopTransfer = () => {
    if (t.state !== 'paused') pause(t.id).catch(() => {})
  }

  const lib = opts?.libraries.find(l => l.path === library)
  const toolName = opts?.tools.find(x => x.name === tool)
  const candidates: ExeCandidate[] = status?.candidates ?? []
  const contentRoot = opts?.download_dir ?? ''
  const needsApi = opts && !opts.steam_api
  const appid = status?.state?.appid ?? null
  const installerRunning = stage === 'installing' && status?.running !== false

  // Once the shortcut exists the download can go (not while its installer runs).
  const downloadCard = appid && (
    <section className="inst-card">
      <h3>{tr('The download')}</h3>
      <p className="muted">
        {stage === 'installed'
          ? tr('The game is installed: the files it was installed from can go. Keep them if you may need to install it again.')
          : tr('If the installation went wrong you may want to keep them and try again.')}
      </p>
      <div className="row">
        <button data-nav className="btn danger" disabled={busy || installerRunning} onClick={() => setWipe(true)}>
          <Icon name="trash" /> {tr('Delete downloaded files…')}
        </button>
        {contentRoot && onExplore && (
          <button data-nav className="btn" onClick={() => onExplore(contentRoot)}>
            <Icon name="folder" /> {tr('Show on Explore')}
          </button>
        )}
      </div>
      {installerRunning && <p className="muted small">{tr('The installer is still running from these files.')}</p>}
    </section>
  )

  const body = (() => {
    if (error) return <p className="inst-error">{error}</p>
    if (!opts) {
      return (
        <p className="muted">
          <Spinner /> {tr('Looking at the files…')}
        </p>
      )
    }
    if (!t.finished && !appid) {
      return (
        <section className="inst-card">
          <h3>{tr('Waiting for the download')}</h3>
          <Progress value={t.progress} />
          <p className="muted">{tr('{pct}% downloaded. You can install as soon as it finishes.', { pct: Math.floor(t.progress * 100) })}</p>
        </section>
      )
    }
    if (stage === 'installed') {
      return (
        <>
          <section className="inst-card ok">
            <h3>
              <Icon name="check" /> {tr('Installed')}
            </h3>
            <p className="muted">
              {trb('**{name}** is in your Steam library and in Games, where its Proton, components, files and artwork are managed.', { name })}
            </p>
            <div className="row">
              {appid && onOpenGame && (
                <button data-nav data-nav-default className="btn primary big" onClick={() => onOpenGame(appid)}>
                  <Icon name="pad" /> {tr('Open in Games')}
                </button>
              )}
            </div>
          </section>
          {downloadCard}
        </>
      )
    }
    if (stage === 'installing') {
      if (status?.running !== false) {
        return (
          <>
            <section className="inst-card">
              <h3>
                <Spinner /> {tr('The installer is running')}
              </h3>
              <p>{tr('Follow it on screen. When it closes, piShop comes back here to pick the game’s executable.')}</p>
              <p className="muted">{trb('Install folder: **{dir}**', { dir: homePath(status?.state?.target ?? '') })}</p>
              <div className="row">
                <button data-nav data-nav-default className="btn big" onClick={() => void run(() => api.installStatus(hash).then(setStatus))}>
                  {tr('Check again')}
                </button>
              </div>
            </section>
            {downloadCard}
          </>
        )
      }
      return (
        <>
          <section className="inst-card">
            <h3>{tr('Which file starts the game?')}</h3>
            <p className="muted">{tr('The installer has closed. Pick the game’s executable; the Steam shortcut will open it from now on.')}</p>
            <div className="inst-list">
              {candidates.map((c, i) => (
                <button
                  key={c.path}
                  data-nav
                  data-nav-default={i === 0 ? '' : undefined}
                  className="inst-row"
                  disabled={busy}
                  onClick={() => void run(() => api.installFinish(hash, c.path).then(s => setStatus({ state: s })), tr('Installed'))}
                >
                  <Icon name="file" />
                  <span className="inst-row-main">
                    <b>{baseName(c.path)}</b>
                    <small>{homePath(dirOf(c.path))}</small>
                  </span>
                  <span className="inst-row-side">
                    {c.registered ? tr('Registered by the installer') : i === 0 ? tr('Best guess') : formatBytes(c.size)}
                  </span>
                </button>
              ))}
              {!candidates.length && <p className="muted">{tr('No executable found where the installer usually puts games.')}</p>}
            </div>
            <div className="row">
              <button data-nav className="btn" onClick={() => setPicker({ start: status?.state?.target ?? contentRoot, then: 'finish' })}>
                {tr('Browse…')}
              </button>
              <button
                data-nav
                className="btn"
                disabled={busy}
                onClick={() => void run(() => api.installStart(hash, { installer, library, tool }).then(s => setStatus({ state: s, running: true })))}
              >
                {tr('Run the installer again')}
              </button>
            </div>
          </section>
          {downloadCard}
        </>
      )
    }
    if (opts.installers.length) {
      return (
        <section className="inst-card">
          <h3>{tr('Install')}</h3>
          <p className="muted">
            {tr('The installer runs under Proton from a new Steam shortcut, which creates the game’s prefix; the game then shows up in Games. The transfer stops while it installs.')}
          </p>
          <div className="inst-list">
            {opts.installers.length > 1 && <Choice label={tr('Installer')} value={baseName(installer)} onOpen={() => open('installer')} />}
            <Choice label={tr('Compatibility')} value={toolName?.display ?? '—'} onOpen={() => open('tool')} />
          </div>
          {lib && (
            <StorageCard
              icon="library"
              title={tr('Install to {place}', { place: lib.label })}
              path={`${lib.path}/piShop`}
              free={lib.free}
              total={lib.total}
              onClick={() => open('library')}
            />
          )}
          <div className="row">
            <button
              data-nav
              data-nav-default
              className="btn primary big"
              disabled={busy || !installer || !library || !tool || !!needsApi}
              onClick={() =>
                void run(() => {
                  stopTransfer()
                  return api.installStart(hash, { installer, library, tool }).then(s => setStatus({ state: s, running: true }))
                })
              }
            >
              <Icon name="download" /> {tr('Start installation')}
            </button>
          </div>
        </section>
      )
    }
    if (opts.portable.length) {
      return (
        <section className="inst-card">
          <h3>{tr('Add to Steam')}</h3>
          <p className="muted">{tr('No installer needed. Pick the file that starts the game.')}</p>
          <div className="inst-list">
            {opts.portable.slice(0, 5).map((c, i) => (
              <button
                key={c.path}
                data-nav
                data-nav-default={i === 0 ? '' : undefined}
                className="inst-row"
                disabled={busy || !!needsApi}
                onClick={() =>
                  void run(() => {
                    stopTransfer()
                    return api.installPortable(hash, c.path, tool).then(s => setStatus({ state: s }))
                  }, tr('Added to Steam'))
                }
              >
                <Icon name="file" />
                <span className="inst-row-main">
                  <b>{baseName(c.path)}</b>
                  <small>{homePath(dirOf(c.path))}</small>
                </span>
                <span className="inst-row-side">{i === 0 ? tr('Best guess') : formatBytes(c.size)}</span>
              </button>
            ))}
            <Choice label={tr('Compatibility')} value={toolName?.display ?? '—'} onOpen={() => open('tool')} />
          </div>
          <div className="row">
            <button data-nav className="btn" onClick={() => setPicker({ start: contentRoot, then: 'portable' })}>
              {tr('Browse…')}
            </button>
          </div>
        </section>
      )
    }
    // Only when nothing extracted is ready to install yet.
    if (archives.length) {
      return (
        <section className="inst-card">
          <h3>{tr('Extract first')}</h3>
          <p className="muted">{tr('This download is compressed. Extract it next to the archive, then install. The transfer stops while it extracts.')}</p>
          <div className="inst-list">
            {archives.map(a => {
              const j = myJobs.find(x => x.archive === a.path)
              return (
                <div key={a.path} className="inst-archive">
                  <Icon name="file" />
                  <span className="inst-row-main">
                    <b>{a.name}</b>
                    <small>
                      {a.kind.toUpperCase()} · {formatBytes(a.size)}
                      {a.parts > 1 ? ` · ${tr('{n} parts', { n: a.parts })}` : ''}
                    </small>
                  </span>
                  {j?.state === 'running' && <Progress value={j.progress} />}
                  {j?.state === 'failed' && <span className="error">{j.error}</span>}
                </div>
              )
            })}
          </div>
          {archives.some(a => a.complete === false) && (
            <p className="inst-error">{tr('A part of this archive is missing. Wait for the download to finish or check the files.')}</p>
          )}
          <ExtractSpace dir={contentRoot} need={archives.reduce((s, a) => s + a.size, 0)} />
          <button data-nav className={`inst-check ${deleteAfter ? 'on' : ''}`} onClick={() => setDeleteAfter(d => !d)}>
            <span className="box">{deleteAfter && <Icon name="check" size={16} />}</span>
            {tr('Delete the archive after extracting')}
          </button>
          <div className="row">
            {extracting ? (
              <button data-nav data-nav-default className="btn big danger" onClick={() => void run(() => api.archiveCancel(extracting.id))}>
                {tr('Cancel extraction')}
              </button>
            ) : (
              <button
                data-nav
                data-nav-default
                className="btn primary big"
                disabled={busy || archives.some(a => a.complete === false)}
                onClick={() =>
                  void run(() => {
                    stopTransfer()
                    return Promise.all(archives.map(a => api.archiveExtract(a.path, deleteAfter)))
                  })
                }
              >
                {tr('Extract')}
              </button>
            )}
          </div>
        </section>
      )
    }
    return (
      <section className="inst-card">
        <h3>{tr('Nothing to install here')}</h3>
        <p className="muted">{tr('No installer, archive or Windows executable was found in this download.')}</p>
      </section>
    )
  })()

  return (
    <div className="inst" data-nav-scope>
      {game?.hero && <div className="inst-bg" style={{ backgroundImage: `url(${imgUrl(game.hero)})` }} />}
      <div className="inst-shade" />
      <div className="inst-content">
        <aside className="inst-side">
          <div className="inst-cover">{game?.cover ? <img src={imgUrl(game.cover)} alt="" draggable={false} /> : <Icon name="pad" size={48} />}</div>
          <ProtonBadge appid={game?.steam_appid ? Number(game.steam_appid) : null} />
          <button data-nav className="btn" onClick={() => setCompat(true)}>
            {tr('Compatibility reports')}
          </button>
          {contentRoot && onExplore && (
            <button data-nav className="btn" onClick={() => onExplore(contentRoot)}>
              <Icon name="folder" /> {tr('Show on Explore')}
            </button>
          )}
        </aside>
        <div className="inst-main">
          <span className="hero-kicker">{[game?.year, game?.platform, game?.developers?.[0]].filter(Boolean).join(' · ')}</span>
          <h1>{name}</h1>
          {needsApi && (
            <div className="inst-banner">
              <p>
                {opts?.debugging_enabled
                  ? tr('Steam’s client API is switched on but Steam hasn’t picked it up yet. Restart Steam to use it.')
                  : tr('To add games to your library, piShop uses Steam’s client API (the same one Decky Loader uses). Turning it on restarts Steam; open piShop again afterwards.')}
              </p>
              <button data-nav className="btn" onClick={() => void run(() => api.enableSteamApi(), tr('Restarting Steam…'))}>
                {tr('Turn on and restart Steam')}
              </button>
            </div>
          )}
          {body}
        </div>
      </div>

      {choose === 'installer' && opts && (
        <ChooseDialog
          title={tr('Installer')}
          options={opts.installers.map(i => ({ value: i.path, label: i.name, detail: i.kind ?? '' }))}
          value={installer}
          onPick={v => {
            setInstaller(v)
            closeChooser()
          }}
          onClose={closeChooser}
        />
      )}
      {choose === 'library' && opts && (
        <Dialog title={tr('Install to')} onClose={closeChooser} wide>
          <p className="muted">{tr('The game goes into a piShop folder in the Steam library you pick.')}</p>
          <div className="inst-list scroll">
            {opts.libraries.map(l => (
              <StorageCard
                key={l.path}
                icon="library"
                title={l.label}
                path={`${l.path}/piShop`}
                free={l.free}
                total={l.total}
                selected={l.path === library}
                navDefault={l.path === library}
                onClick={() => {
                  setLibrary(l.path)
                  closeChooser()
                }}
              />
            ))}
          </div>
        </Dialog>
      )}
      {choose === 'tool' && opts && (
        <ChooseDialog
          title={tr('Compatibility')}
          options={opts.tools.map(x => ({ value: x.name, label: x.display, detail: x.installed ? tr('Installed') : tr('Downloads on first use') }))}
          value={tool}
          onPick={v => {
            setTool(v)
            closeChooser()
          }}
          onClose={closeChooser}
        />
      )}
      {picker && (
        <FilePicker
          title={tr('Pick the game’s executable')}
          accept={['.exe']}
          start={picker.start}
          onClose={() => setPicker(null)}
          onPick={path => {
            const then = picker.then
            setPicker(null)
            void run(
              () =>
                (then === 'finish'
                  ? api.installFinish(hash, path)
                  : (stopTransfer(), api.installPortable(hash, path, tool))
                ).then(s => setStatus({ state: s })),
              then === 'finish' ? tr('Installed') : tr('Added to Steam'),
            )
          }}
        />
      )}
      {compat && <CompatPanel appid={game?.steam_appid ? Number(game.steam_appid) : null} name={name} onClose={() => setCompat(false)} />}
      {wipe && (
        <DeleteDownloadDialog
          hash={hash}
          name={name}
          dir={contentRoot}
          size={t.totalBytes}
          onClose={() => setWipe(false)}
          onDone={() => {
            setWipe(false)
            onClose()
          }}
        />
      )}
    </div>
  )
}

/** Where the archive extracts to, and whether it fits there. */
function ExtractSpace({ dir, need }: { dir: string; need: number }) {
  const s = useSpace(dir || null)
  if (!dir) return null
  return (
    <StorageCard
      icon="folder"
      title={tr('Extracts next to the archive')}
      path={dir}
      free={s?.free}
      total={s?.total}
      disk={s?.disk}
      warn={s?.free != null && s.free < need ? tr('Not enough free space there.') : null}
    />
  )
}

/** Transfers → Delete downloaded files: the torrent's files and what was extracted from them. */
export function DeleteDownloadDialog({
  hash,
  name,
  dir,
  size,
  onDone,
  onClose,
}: {
  hash: string
  name: string
  dir: string
  size: number
  onDone: () => void
  onClose: () => void
}) {
  const [busy, setBusy] = useState(false)
  const s = useSpace(dir || null)
  const go = async () => {
    setBusy(true)
    try {
      const r = await api.installDeleteDownload(hash)
      toast(tr('Downloaded files deleted'), tr('{size} freed', { size: formatBytes(r.freed) }), 'ok')
      onDone()
    } catch (e) {
      setBusy(false)
      toast(tr("Couldn't delete the download"), (e as Error).message, 'error')
    }
  }
  return (
    <Dialog title={tr('Delete the download of {name}?', { name })} onClose={onClose} wide>
      <p className="muted">
        {tr('The torrent’s files and what was extracted from them go; the installed game stays in Games. The transfer leaves Transfers.')}
      </p>
      {dir && (
        <StorageCard icon="download" title={tr('About {size} to free', { size: formatBytes(size) })} path={dir} free={s?.free} total={s?.total} disk={s?.disk} />
      )}
      <div className="dialog-actions">
        <button data-nav className="btn danger" disabled={busy} onClick={() => void go()}>
          {busy ? <Spinner /> : <Icon name="trash" size={18} />} {tr('Delete')}
        </button>
        <button data-nav data-nav-default className="btn" onClick={onClose}>
          {tr('Cancel')}
        </button>
      </div>
    </Dialog>
  )
}
