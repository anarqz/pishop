// A game's setup (Games → game): what its Steam shortcut runs, where the game
// and its Proton prefix are (with their disks' free space), which Proton it
// uses, moving the game (into the prefix's Program Files or another Steam
// library), components for the prefix (Steam's redistributables,
// winetricks), running an installer inside the prefix, the artwork (refresh,
// or pick it by hand) and removing the shortcut, the prefix or the files.

import { useCallback, useEffect, useRef, useState } from 'react'
import { type ArtResult, type ArtSearch, type ComponentsStatus, type InstallInfo, api, formatBytes, imgUrl } from '../api'
import { tr, trb, trn } from '../i18n'
import { focusFirst, keepFocus } from '../input'
import { StorageCard, homePath } from '../storage'
import { Dialog, Icon, Progress, Spinner, TextPrompt, toast } from '../ui'
import DirPicker from './DirPicker'
import FilePicker from './FilePicker'
import PatchesPanel from './Patches'

const baseName = (p: string) => p.split('/').pop() ?? p

/** How piShop names the winetricks verbs it offers. */
const verbInfo = (verb: string): { title: string; note: string } =>
  ({
    vcrun2022: { title: 'Visual C++ 2015–2022', note: tr('Most games from 2015 on') },
    d3dx9: { title: tr('DirectX 9 extras (d3dx9)'), note: tr('Older games') },
    xact: { title: 'XACT / XAudio', note: tr('Sound in older games') },
    xinput: { title: 'XInput', note: tr('Controllers in older games') },
    d3dx11_43: { title: tr('DirectX 11 extras (d3dx11)'), note: '' },
    d3dcompiler_47: { title: tr('Shader compiler (d3dcompiler_47)'), note: '' },
    vcrun2013: { title: 'Visual C++ 2013', note: '' },
    vcrun2012: { title: 'Visual C++ 2012', note: '' },
    vcrun2010: { title: 'Visual C++ 2010', note: '' },
    vcrun2008: { title: 'Visual C++ 2008', note: '' },
    vcrun2005: { title: 'Visual C++ 2005', note: '' },
    dotnet48: { title: '.NET Framework 4.8', note: tr('Slow: 10 minutes or more') },
    dotnetdesktop8: { title: '.NET 8 Desktop Runtime', note: '' },
    xna40: { title: 'XNA Framework 4.0', note: tr('Indie games') },
    physx: { title: 'NVIDIA PhysX', note: '' },
    openal: { title: 'OpenAL', note: '' },
    corefonts: { title: tr('Microsoft core fonts'), note: '' },
  })[verb] ?? { title: verb, note: '' }

/** "Steam: cover, banner… · SteamGridDB: icon · Not found: logo" */
export function artSummary(r: ArtResult): string {
  const names: Record<keyof ArtResult, string> = {
    cover: tr('Cover'),
    wide: tr('Banner'),
    hero: tr('Hero'),
    logo: tr('Logo'),
    icon: tr('Icon'),
  }
  const keys = Object.keys(names) as Array<keyof ArtResult>
  const of = (src: ArtResult[keyof ArtResult]) => keys.filter(k => r[k] === src).map(k => names[k])
  return [
    [tr('Steam'), of('steam')],
    ['SteamGridDB', of('steamgriddb')],
    ['piShop', of('piShop')],
    [tr('Not found'), of(null)],
  ]
    .filter(([, list]) => list.length)
    .map(([from, list]) => `${from}: ${(list as string[]).join(', ')}`)
    .join(' · ')
}

/** Installers piShop can run in a prefix. */
const INSTALLERS = ['.exe', '.msi', '.bat', '.cmd']

export default function GameSetup({
  gameKey,
  name,
  startDir,
  onChanged,
  onExplore,
  onRemoved,
}: {
  gameKey: string
  name: string
  /** Where the installer picker starts (the download's folder, while there is one). */
  startDir?: string
  onChanged: () => void
  onExplore?: (path: string) => void
  /** The shortcut was removed from Steam: the page closes. */
  onRemoved?: () => void
}) {
  const [info, setInfo] = useState<InstallInfo | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [comp, setComp] = useState<ComponentsStatus | null>(null)
  const [dialog, setDialog] = useState<null | 'tool' | 'components' | 'installer' | 'move' | 'exe' | 'folder' | 'art' | 'remove' | 'patches'>(null)
  const [busy, setBusy] = useState(false)
  const restore = useRef<(() => void) | null>(null)
  const changed = useRef(onChanged)
  changed.current = onChanged

  const loadComponents = useCallback(async () => {
    try {
      setComp(await api.components(gameKey))
    } catch {
      // no prefix yet, or no shortcut: the dialog says so
    }
  }, [gameKey])
  const load = useCallback(async () => {
    void loadComponents()
    try {
      setInfo(await api.installInfo(gameKey))
      setError(null)
    } catch (e) {
      setError((e as Error).message)
    }
  }, [gameKey, loadComponents])
  useEffect(() => {
    void load()
  }, [load])

  // Follow a move while it runs.
  const moving = info?.moving?.state === 'running'
  useEffect(() => {
    if (!moving) return
    let alive = true
    const id = setInterval(() => {
      api
        .installMoveStatus(gameKey)
        .then(job => {
          if (!alive) return
          setInfo(i => i && { ...i, moving: job })
          if (job?.state === 'running') return
          if (job?.state === 'done') toast(tr('Game moved'), homePath(job.to), 'ok')
          if (job?.state === 'failed') toast(tr('Couldn’t move the game'), job.error ?? '', 'error')
          void load()
          changed.current()
        })
        .catch(() => {})
    }, 1000)
    return () => {
      alive = false
      clearInterval(id)
    }
  }, [moving, gameKey, load])

  // Follow a components run.
  const compRunning = !!comp?.running
  useEffect(() => {
    if (!compRunning) return
    let alive = true
    const id = setInterval(() => {
      api
        .components(gameKey)
        .then(s => {
          if (!alive) return
          setComp(s)
          if (s.running) return
          if (s.exit === 0) toast(tr('Components installed'), s.current, 'ok')
          else toast(tr('Some components failed'), s.log.slice(-1)[0] ?? '', 'error')
          void load()
        })
        .catch(() => {})
    }, 1500)
    return () => {
      alive = false
      clearInterval(id)
    }
  }, [compRunning, gameKey, load])

  // While an installer runs in the prefix.
  const borrowed = !!info?.borrowed
  useEffect(() => {
    if (!borrowed) return
    const id = setInterval(() => void load(), 3000)
    return () => clearInterval(id)
  }, [borrowed, load])

  const open = (d: NonNullable<typeof dialog>) => {
    restore.current = keepFocus()
    setDialog(d)
  }
  const close = () => {
    setDialog(null)
    restore.current?.()
  }
  const run = async <T,>(f: () => Promise<T>, okTitle?: string, okText?: (v: T) => string) => {
    setBusy(true)
    try {
      const v = await f()
      if (okTitle) toast(okTitle, okText?.(v) ?? '', 'ok')
    } catch (e) {
      toast(tr('Something went wrong'), (e as Error).message, 'error')
    } finally {
      setBusy(false)
    }
  }

  if (error) {
    return (
      <section className="inst-card">
        <p className="inst-error">{error}</p>
      </section>
    )
  }
  if (!info) {
    return (
      <section className="inst-card">
        <p className="muted">
          <Spinner /> {tr('Reading the game’s setup…')}
        </p>
      </section>
    )
  }
  const sc = info.shortcut
  const job = info.moving
  const here = info.targets.find(t => t.here)
  const toolName = info.tools.find(t => t.name === info.tool)?.display ?? (info.tool || tr('None (runs natively)'))
  const proton = !!info.tool || info.prefix.exists
  const installed = [
    ...(comp?.steam.filter(s => s.installed).map(s => s.title) ?? []),
    ...(comp?.verbs.filter(v => v.installed).map(v => verbInfo(v.verb).title) ?? []),
  ]
  const exePath = info.exe?.path ?? (sc?.exe ? sc.exe.replace(/^"|"$/g, '') : '')
  const exeDir = exePath.replace(/\/[^/]+$/, '')
  const gameIn = !!info.game_dir && info.prefix.exists && info.game_dir.path.startsWith(info.prefix.path)

  return (
    <>
      <section className="inst-card">
        <h3>{tr('Game setup')}</h3>
        {info.borrowed && (
          <p className="inst-note">
            <Spinner /> {trb('**{name}** is running in this prefix. piShop comes back when it closes.', { name: baseName(info.borrowed) })}
          </p>
        )}
        <dl className="inst-facts">
          <Fact label={tr('Target')} value={sc?.exe || (info.exe ? `"${info.exe.path}"` : '—')} mono />
          <Fact label={tr('Start in')} value={sc?.start_dir || '—'} mono />
          {sc?.launch_options && <Fact label={tr('Launch options')} value={sc.launch_options} mono />}
        </dl>
        <div className="inst-list">
          {info.game_dir ? (
            <StorageCard
              icon="folder"
              title={tr('Installed at {path}', { path: info.game_dir.windows })}
              path={info.game_dir.path}
              note={formatBytes(info.game_dir.size)}
              free={info.game_dir.free}
              total={info.game_dir.total}
              disk={info.game_dir.disk}
            />
          ) : (
            info.external &&
            info.exe && (
              <p className="muted small">
                {tr('piShop doesn’t know which folder is this game’s own, so it can’t move or delete its files. Pick it to do that.')}
              </p>
            )
          )}
          {proton && (
            <StorageCard
              icon="prefix"
              title={tr('Proton prefix')}
              path={info.prefix.path}
              note={info.prefix.exists ? (gameIn ? tr('The game is inside it') : null) : tr('Not created yet: Proton creates it the first time the game runs.')}
              free={info.prefix.free}
              total={info.prefix.total}
              disk={info.prefix.disk}
            />
          )}
        </div>
        <div className="inst-list">
          <Choice label={tr('Compatibility')} value={toolName} disabled={!info.steam_api || info.busy} onOpen={() => open('tool')} />
          {proton && (
            <Choice
              label={tr('Components')}
              value={comp?.running ? tr('Installing…') : installed.length ? installed.join(', ') : tr('None installed yet')}
              onOpen={() => open('components')}
            />
          )}
          {job?.state !== 'running' && info.targets.length > 0 && (
            <Choice
              label={tr('Location')}
              value={here ? (here.kind === 'prefix' ? `${here.label} · ${info.prefix.disk}` : here.label) : (info.game_dir?.disk ?? '—')}
              disabled={busy || info.busy}
              onOpen={() => open('move')}
            />
          )}
          {info.external && exePath && (
            <Choice label={tr('Game folder')} value={info.game_dir ? homePath(info.game_dir.path) : tr('Not set')} onOpen={() => open('folder')} />
          )}
        </div>
        {job?.state === 'running' && (
          <div className="inst-move">
            <p>{trb('Moving to **{to}**…', { to: info.targets.find(t => t.to === job.to)?.windows ?? homePath(job.to) })}</p>
            <Progress value={job.total ? job.done / job.total : 0} />
            <p className="muted">{tr('{done} of {total}', { done: formatBytes(job.done), total: formatBytes(job.total) })}</p>
            <div className="row">
              <button data-nav className="btn danger" onClick={() => void run(() => api.installMoveCancel(gameKey))}>
                {tr('Cancel')}
              </button>
            </div>
          </div>
        )}
        {job?.state === 'failed' && <p className="inst-error">{job.error}</p>}
        <div className="row">
          {proton && (
            <button data-nav className="btn" disabled={busy || info.busy || !info.prefix.exists} onClick={() => open('installer')}>
              <Icon name="file" /> {tr('Run an installer in this prefix…')}
            </button>
          )}
          <button data-nav className="btn" disabled={busy || info.busy} onClick={() => open('exe')}>
            <Icon name="pad" /> {tr('Change executable…')}
          </button>
          {onExplore && info.game_dir && (
            <button data-nav className="btn" onClick={() => onExplore(info.game_dir!.path)}>
              <Icon name="folder" /> {tr('Show on Explore')}
            </button>
          )}
          {onExplore && info.prefix.exists && (
            <button data-nav className="btn" onClick={() => onExplore(`${info.prefix.path}/drive_c`)}>
              <Icon name="prefix" /> {tr('Show the prefix on Explore')}
            </button>
          )}
        </div>
      </section>

      <section className="inst-card">
        <h3>{tr('Patches')}</h3>
        <p className="muted">{tr('Quick workarounds to try when a game won’t run on Linux, as launch options you can turn on and off.')}</p>
        <div className="row">
          <button data-nav className="btn" disabled={!info.steam_api} onClick={() => open('patches')}>
            <Icon name="gear" /> {tr('Patches…')}
          </button>
        </div>
      </section>

      <section className="inst-card">
        <h3>{tr('Artwork')}</h3>
        <p className="muted">{tr('Cover, banner, hero, logo and icon in your Steam library.')}</p>
        <div className="row">
          <button
            data-nav
            className="btn"
            disabled={busy || !info.steam_api}
            onClick={() =>
              void run(
                () =>
                  api.installArtwork(gameKey).then(r => {
                    changed.current()
                    return r
                  }),
                tr('Artwork updated'),
                artSummary,
              )
            }
          >
            <Icon name="refresh" /> {tr('Refresh artwork')}
          </button>
          <button data-nav className="btn" disabled={busy || !info.steam_api} onClick={() => open('art')}>
            <Icon name="image" /> {tr('Find artwork…')}
          </button>
        </div>
      </section>

      <section className="inst-card">
        <h3>{tr('Remove')}</h3>
        <p className="muted">{tr('The Steam shortcut, the Proton prefix or the game’s files: pick what goes.')}</p>
        <div className="row">
          <button data-nav className="btn danger" disabled={busy || info.busy} onClick={() => open('remove')}>
            <Icon name="trash" /> {tr('Remove…')}
          </button>
        </div>
      </section>

      {dialog === 'tool' && (
        <ChooseDialog
          title={tr('Compatibility')}
          options={info.tools.map(x => ({ value: x.name, label: x.display, detail: x.installed ? tr('Installed') : tr('Downloads on first use') }))}
          value={info.tool}
          onPick={v => {
            close()
            if (v !== info.tool) {
              void run(async () => {
                await api.installTool(gameKey, v)
                await load()
                changed.current()
              }, tr('Compatibility tool changed'))
            }
          }}
          onClose={close}
        />
      )}
      {dialog === 'move' && (
        <MoveDialog
          info={info}
          onClose={close}
          onPick={to => {
            close()
            void run(async () => {
              await api.installMove(gameKey, to)
              await load()
            })
          }}
        />
      )}
      {dialog === 'components' && (
        <ComponentsDialog hash={gameKey} status={comp} onRefresh={loadComponents} onRunInstaller={() => setDialog('installer')} onClose={close} />
      )}
      {dialog === 'installer' && (
        <FilePicker
          title={tr('Pick an installer to run in this prefix')}
          start={startDir || info.download_dir || info.game_dir?.path || exeDir || '/home'}
          accept={INSTALLERS}
          onClose={close}
          onPick={path => {
            close()
            void run(
              async () => {
                await api.installRun(gameKey, path)
                await load()
              },
              tr('Running {name}', { name: baseName(path) }),
            )
          }}
        />
      )}
      {dialog === 'exe' && (
        <FilePicker
          title={tr('Pick the game’s executable')}
          start={info.game_dir?.path || exeDir || '/home'}
          accept={proton ? ['.exe', '.bat', '.lnk'] : ['']}
          onClose={close}
          onPick={path => {
            close()
            void run(
              async () => {
                await api.installFinish(gameKey, path)
                await load()
                changed.current()
              },
              tr('Executable changed'),
              () => baseName(path),
            )
          }}
        />
      )}
      {dialog === 'folder' && (
        <DirPicker
          title={tr('Which folder is the game?')}
          start={info.game_dir?.path || exeDir || '/home'}
          confirmLabel={tr('This is the game’s folder')}
          onClose={close}
          onPick={path => {
            close()
            void run(
              async () => {
                await api.gameFolder(gameKey, path)
                await load()
              },
              tr('Game folder set'),
              () => homePath(path),
            )
          }}
        />
      )}
      {dialog === 'art' && (
        <ArtworkDialog
          gameKey={gameKey}
          name={name}
          onClose={close}
          onApplied={() => {
            close()
            changed.current()
          }}
        />
      )}
      {dialog === 'patches' && (
        <PatchesPanel
          appid={info.appid}
          name={name}
          onClose={() => {
            close()
            void load()
          }}
        />
      )}
      {dialog === 'remove' && (
        <RemoveDialog
          gameKey={gameKey}
          name={name}
          info={info}
          onClose={close}
          onDone={r => {
            close()
            void load()
            changed.current()
            if (r.removed.includes('shortcut')) onRemoved?.()
          }}
        />
      )}
    </>
  )
}

/** Where the game can move, each place with its disk's free space. */
function MoveDialog({ info, onPick, onClose }: { info: InstallInfo; onPick: (to: string) => void; onClose: () => void }) {
  const size = info.game_dir?.size ?? 0
  const first = info.targets.findIndex(x => !x.here && !x.blocked)
  return (
    <Dialog title={tr('Move the game')} onClose={onClose} wide>
      <p className="muted">
        {info.is_download
          ? tr('The game runs from its download’s files: they move, and the transfer leaves Transfers.')
          : tr('The game’s files move; the Steam shortcut’s target and start folder follow them.')}
      </p>
      <div className="inst-list scroll">
        {info.targets.map((t, i) => (
          <StorageCard
            key={t.id}
            icon={t.kind === 'prefix' ? 'prefix' : 'library'}
            title={t.here ? tr('{place} (it’s here)', { place: t.label }) : t.label}
            path={t.to}
            note={t.here ? null : t.same_disk ? tr('Same disk: it takes a moment') : tr('Copies {size}', { size: formatBytes(size) })}
            warn={t.here ? null : t.blocked}
            free={t.free}
            total={t.total}
            disk={t.disk}
            selected={t.here}
            disabled={t.here || !!t.blocked}
            navDefault={i === first}
            onClick={() => onPick(t.id)}
          />
        ))}
      </div>
    </Dialog>
  )
}

/** Find artwork: Steam's official art or a SteamGridDB game, picked by hand. */
function ArtworkDialog({ gameKey, name, onApplied, onClose }: { gameKey: string; name: string; onApplied: () => void; onClose: () => void }) {
  const [q, setQ] = useState(name.replace(/\s*\([^)]*\)\s*$/, ''))
  const [res, setRes] = useState<ArtSearch | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [typing, setTyping] = useState(false)
  const [busy, setBusy] = useState<string | null>(null)
  useEffect(() => {
    let alive = true
    setRes(null)
    setError(null)
    api
      .artSearch(q)
      .then(r => alive && setRes(r))
      .catch(e => alive && setError((e as Error).message))
    return () => {
      alive = false
    }
  }, [q])
  useEffect(() => {
    if (res) requestAnimationFrame(focusFirst)
  }, [res])
  const apply = (source: 'steam' | 'sgdb', id: string, label: string) => {
    setBusy(`${source}:${id}`)
    api
      .artApply(gameKey, source, id, label)
      .then(r => {
        toast(tr('Artwork updated'), artSummary(r), 'ok')
        onApplied()
      })
      .catch(e => {
        setBusy(null)
        toast(tr('Something went wrong'), (e as Error).message, 'error')
      })
  }
  if (typing) {
    return (
      <TextPrompt
        title={tr('Search artwork')}
        placeholder={tr('Game name…')}
        initial={q}
        submitLabel={tr('Search')}
        onCancel={() => setTyping(false)}
        onSubmit={v => {
          setTyping(false)
          if (v.trim()) setQ(v.trim())
        }}
      />
    )
  }
  return (
    <Dialog title={tr('Find artwork')} onClose={onClose} wide>
      <div className="row">
        <button data-nav className="btn" onClick={() => setTyping(true)}>
          <Icon name="search" size={18} /> {q}
        </button>
        <span className="muted small">{tr('Steam’s official art first; SteamGridDB has the rest.')}</span>
      </div>
      {error && <p className="inst-error">{error}</p>}
      {!res && !error && (
        <p className="muted">
          <Spinner /> {tr('Searching…')}
        </p>
      )}
      {res && (
        <div className="inst-list scroll art-results">
          {res.steam.length > 0 && <h4 className="inst-sub">Steam</h4>}
          <div className="art-grid">
            {res.steam.map((s, i) => (
              <button
                key={`s${s.appid}`}
                data-nav
                data-nav-default={i === 0 ? '' : undefined}
                className="art-pick wide"
                disabled={!!busy}
                onClick={() => apply('steam', s.appid, s.name)}
              >
                {s.image ? (
                  <img src={imgUrl(s.image)} alt="" />
                ) : (
                  <span className="art-ph">
                    <Icon name="image" />
                  </span>
                )}
                <b>{s.name}</b>
                <small>{busy === `steam:${s.appid}` ? tr('Applying…') : tr('Official art · app {id}', { id: s.appid })}</small>
              </button>
            ))}
          </div>
          {res.sgdb.length > 0 && <h4 className="inst-sub">SteamGridDB</h4>}
          {res.sgdb_error && <p className="inst-error">{res.sgdb_error}</p>}
          <div className="art-grid">
            {res.sgdb.map((g, i) => (
              <button
                key={`g${g.id}`}
                data-nav
                data-nav-default={!res.steam.length && i === 0 ? '' : undefined}
                className="art-pick"
                disabled={!!busy}
                onClick={() => apply('sgdb', String(g.id), g.name)}
              >
                {g.cover ? (
                  <img src={imgUrl(g.cover)} alt="" />
                ) : (
                  <span className="art-ph">
                    <Icon name="image" />
                  </span>
                )}
                <b>{g.name}</b>
                <small>{busy === `sgdb:${g.id}` ? tr('Applying…') : [g.year, g.verified ? tr('verified') : null].filter(Boolean).join(' · ')}</small>
              </button>
            ))}
          </div>
          {!res.steam.length && !res.sgdb.length && <p className="muted">{tr('Nothing found. Try another name.')}</p>}
        </div>
      )}
    </Dialog>
  )
}

/** Remove: the shortcut, the prefix and/or the game's own files. */
function RemoveDialog({
  gameKey,
  name,
  info,
  onDone,
  onClose,
}: {
  gameKey: string
  name: string
  info: InstallInfo
  onDone: (r: { removed: string[]; freed: number }) => void
  onClose: () => void
}) {
  const [shortcut, setShortcut] = useState(true)
  const [prefix, setPrefix] = useState(false)
  const [files, setFiles] = useState(false)
  const [busy, setBusy] = useState(false)
  const gameIn = !!info.game_dir && info.prefix.exists && info.game_dir.path.startsWith(info.prefix.path)
  const go = async () => {
    setBusy(true)
    try {
      const r = await api.gameRemove(gameKey, { shortcut, prefix, files })
      toast(tr('Removed'), r.freed ? tr('{size} freed', { size: formatBytes(r.freed) }) : name, 'ok')
      onDone(r)
    } catch (e) {
      setBusy(false)
      toast(tr('Something went wrong'), (e as Error).message, 'error')
    }
  }
  return (
    <Dialog title={tr('Remove {name}?', { name })} onClose={onClose} wide>
      <div className="inst-list">
        <Check on={shortcut} title={tr('Steam shortcut')} detail={tr('Off your Steam library, with its artwork')} onToggle={() => setShortcut(v => !v)} />
        {info.prefix.exists && (
          <Check
            on={prefix}
            title={tr('Proton prefix')}
            detail={[homePath(info.prefix.path), tr('Saves and settings kept in the prefix go with it'), gameIn ? tr('The game is inside it') : null]
              .filter(Boolean)
              .join(' · ')}
            onToggle={() => setPrefix(v => !v)}
          />
        )}
        {info.game_dir && (
          <Check
            on={files}
            title={tr('Game files')}
            detail={`${homePath(info.game_dir.path)} · ${formatBytes(info.game_dir.size)}`}
            onToggle={() => setFiles(v => !v)}
          />
        )}
      </div>
      {(prefix || files) && <p className="inst-error">{tr('Deleted files can’t be brought back.')}</p>}
      <div className="dialog-actions">
        <button data-nav className="btn danger" disabled={busy || !(shortcut || prefix || files)} onClick={() => void go()}>
          {busy ? <Spinner /> : <Icon name="trash" size={18} />} {tr('Remove')}
        </button>
        <button data-nav data-nav-default className="btn" onClick={onClose}>
          {tr('Cancel')}
        </button>
      </div>
    </Dialog>
  )
}

function Fact({ label, value, detail, mono }: { label: string; value: string; detail?: string; mono?: boolean }) {
  return (
    <div>
      <dt>{label}</dt>
      <dd>
        <b className={mono ? 'mono' : undefined}>{value}</b>
        {detail && <small>{detail}</small>}
      </dd>
    </div>
  )
}

function Check({ on, title, detail, onToggle }: { on: boolean; title: string; detail: string; onToggle: () => void }) {
  return (
    <button data-nav className={`inst-check ${on ? 'on' : ''}`} onClick={onToggle}>
      <span className="box">{on && <Icon name="check" size={16} />}</span>
      <span className="inst-row-main">
        <b>{title}</b>
        {detail && <small>{detail}</small>}
      </span>
    </button>
  )
}

function ComponentsDialog({
  hash,
  status,
  onRefresh,
  onRunInstaller,
  onClose,
}: {
  hash: string
  status: ComponentsStatus | null
  onRefresh: () => Promise<void>
  onRunInstaller: () => void
  onClose: () => void
}) {
  // Visual C++ 2015–2022 is what most games miss: ticked unless it's there.
  const [steam, setSteam] = useState<Set<string>>(() => {
    const vc = status?.steam.find(s => s.id.toLowerCase() === 'vcredist/2022')
    return new Set(vc && !vc.installed ? [vc.id] : [])
  })
  const [verbs, setVerbs] = useState<Set<string>>(() => {
    const steamVc = status?.steam.some(s => s.id.toLowerCase() === 'vcredist/2022')
    const vc = status?.verbs.find(v => v.verb === 'vcrun2022')
    return new Set(!steamVc && vc && !vc.installed ? ['vcrun2022'] : [])
  })
  const [busy, setBusy] = useState(false)
  const running = !!status?.running
  useEffect(() => {
    requestAnimationFrame(focusFirst)
  }, [running, !!status])
  const toggle = (set: (f: (s: Set<string>) => Set<string>) => void, v: string) =>
    set(s => {
      const n = new Set(s)
      if (n.has(v)) n.delete(v)
      else n.add(v)
      return n
    })
  const act = async (f: () => Promise<unknown>) => {
    setBusy(true)
    try {
      await f()
      await onRefresh()
    } catch (e) {
      toast(tr('Something went wrong'), (e as Error).message, 'error')
    } finally {
      setBusy(false)
    }
  }
  const count = steam.size + verbs.size

  return (
    <Dialog title={tr('Components')} onClose={onClose} wide>
      {!status ? (
        <p className="muted">
          <Spinner /> {tr('Loading…')}
        </p>
      ) : !status.prefix ? (
        <p className="inst-error">{tr('This game has no Proton prefix yet: play it once first.')}</p>
      ) : running ? (
        <>
          <p>
            <Spinner /> {tr('Installing {verbs}…', { verbs: status.current })}
          </p>
          <pre className="inst-log">{status.log.join('\n')}</pre>
          <div className="row">
            <button data-nav className="btn danger" disabled={busy} onClick={() => void act(() => api.componentsCancel(hash))}>
              {tr('Cancel')}
            </button>
          </div>
        </>
      ) : (
        <>
          <p className="muted">{tr('Runtimes and libraries for this game’s prefix only: every game has its own.')}</p>
          {status.exit != null && status.exit !== 0 && (
            <>
              <p className="inst-error">{tr('The last run failed (code {code}).', { code: status.exit })}</p>
              <pre className="inst-log">{status.log.join('\n')}</pre>
            </>
          )}
          <div className="inst-list scroll">
            {status.steam.length > 0 && (
              <>
                <h4 className="inst-sub">{tr('From Steam (offline)')}</h4>
                <p className="muted small">{tr('Steam’s own installers, run the way Steam runs them for its games.')}</p>
                {status.steam.map(s => (
                  <Check
                    key={s.id}
                    on={steam.has(s.id)}
                    title={s.title}
                    detail={[
                      s.installed ? tr('Installed') : '',
                      s.id.toLowerCase().startsWith('dotnet') ? tr('Proton normally uses Wine Mono instead') : '',
                    ]
                      .filter(Boolean)
                      .join(' · ')}
                    onToggle={() => toggle(setSteam, s.id)}
                  />
                ))}
              </>
            )}
            {status.available && (
              <>
                <h4 className="inst-sub">Winetricks</h4>
                <p className="muted small">{tr('Also sets Wine to use them. Offline when Steam has the files; the rest is downloaded.')}</p>
                {status.verbs.map(v => {
                  const { title, note } = verbInfo(v.verb)
                  return (
                    <Check
                      key={v.verb}
                      on={verbs.has(v.verb)}
                      title={title}
                      detail={[v.verb, v.installed ? tr('Installed') : note, v.offline ? tr('Offline') : tr('Needs internet')].filter(Boolean).join(' · ')}
                      onToggle={() => toggle(setVerbs, v.verb)}
                    />
                  )
                })}
              </>
            )}
          </div>
          <div className="row">
            <button
              data-nav
              className="btn primary"
              disabled={busy || !count}
              onClick={() => void act(() => api.componentsRun(hash, { steam: [...steam], verbs: [...verbs] }))}
            >
              {trn(count, 'Install {n} component', 'Install {n} components')}
            </button>
            <button data-nav className="btn" onClick={onRunInstaller}>
              <Icon name="file" /> {tr('Run an installer in this prefix…')}
            </button>
          </div>
        </>
      )}
    </Dialog>
  )
}

export function Choice({ label, value, disabled, onOpen }: { label: string; value: string; disabled?: boolean; onOpen: () => void }) {
  return (
    <button data-nav className="inst-row" disabled={disabled} onClick={onOpen}>
      <span className="inst-row-main">
        <small>{label}</small>
        <b>{value}</b>
      </span>
      <span className="inst-row-side">{tr('Change')}</span>
    </button>
  )
}

export function ChooseDialog({
  title,
  note,
  options,
  value,
  onPick,
  onClose,
}: {
  title: string
  note?: string
  options: Array<{ value: string; label: string; detail?: string; disabled?: boolean }>
  value: string
  onPick: (v: string) => void
  onClose: () => void
}) {
  return (
    <Dialog title={title} onClose={onClose} wide>
      {note && <p className="muted">{note}</p>}
      <div className="inst-list scroll">
        {options.map(o => (
          <button
            key={o.value}
            data-nav
            data-nav-default={o.value === value ? '' : undefined}
            className="inst-row"
            disabled={o.disabled}
            onClick={() => onPick(o.value)}
          >
            <Icon name={o.value === value ? 'check' : 'file'} />
            <span className="inst-row-main">
              <b>{o.label}</b>
              {o.detail && <small>{o.detail}</small>}
            </span>
          </button>
        ))}
      </div>
    </Dialog>
  )
}
