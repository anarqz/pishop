// An installed game's setup (Transfers → game): what its Steam shortcut runs,
// where the game and its Proton prefix are, which Proton it uses, moving the
// game's files into the prefix's Program Files, components for the prefix
// (Steam's redistributables, winetricks), and running an installer from the
// download inside the prefix.

import { useCallback, useEffect, useRef, useState } from 'react'
import { type ArtResult, type ComponentsStatus, type InstallInfo, api, formatBytes } from '../api'
import { tr, trb, trn } from '../i18n'
import { focusFirst, keepFocus } from '../input'
import { Dialog, Icon, Progress, Spinner, toast } from '../ui'
import FilePicker from './FilePicker'

const home = (p: string) => p.replace(/^\/home\/[^/]+/, '~')
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

export default function GameSetup({ hash, downloadDir, onChanged }: { hash: string; downloadDir: string; onChanged: () => void }) {
  const [info, setInfo] = useState<InstallInfo | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [comp, setComp] = useState<ComponentsStatus | null>(null)
  const [dialog, setDialog] = useState<null | 'tool' | 'components' | 'installer' | 'move'>(null)
  const [busy, setBusy] = useState(false)
  const restore = useRef<(() => void) | null>(null)
  const changed = useRef(onChanged)
  changed.current = onChanged

  const loadComponents = useCallback(async () => {
    try {
      setComp(await api.components(hash))
    } catch {
      // no prefix yet, or no shortcut: the dialog says so
    }
  }, [hash])
  const load = useCallback(async () => {
    void loadComponents()
    try {
      setInfo(await api.installInfo(hash))
      setError(null)
    } catch (e) {
      setError((e as Error).message)
    }
  }, [hash, loadComponents])
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
        .installMoveStatus(hash)
        .then(job => {
          if (!alive) return
          setInfo(i => i && { ...i, moving: job })
          if (job?.state === 'running') return
          if (job?.state === 'done') toast(tr('Moved into the prefix'), job.to, 'ok')
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
  }, [moving, hash, load])

  // Follow a components run.
  const compRunning = !!comp?.running
  useEffect(() => {
    if (!compRunning) return
    let alive = true
    const id = setInterval(() => {
      api
        .components(hash)
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
  }, [compRunning, hash, load])

  // While an installer from the download runs in the prefix.
  const borrowed = !!info?.borrowed
  useEffect(() => {
    if (!borrowed) return
    const id = setInterval(() => void load(), 3000)
    return () => clearInterval(id)
  }, [borrowed, load])

  const open = (d: 'tool' | 'components' | 'installer' | 'move') => {
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
  const where = here ? (here.kind === 'prefix' ? `${here.label} · ${info.prefix.disk}` : here.label) : (info.game_dir?.disk ?? '—')
  const toolName = info.tools.find(t => t.name === info.tool)?.display ?? (info.tool || '—')
  const installed = [
    ...(comp?.steam.filter(s => s.installed).map(s => s.title) ?? []),
    ...(comp?.verbs.filter(v => v.installed).map(v => verbInfo(v.verb).title) ?? []),
  ]

  return (
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
        {info.game_dir && (
          <Fact
            label={tr('Installed at')}
            value={info.game_dir.windows}
            detail={`${home(info.game_dir.path)} · ${formatBytes(info.game_dir.size)} · ${info.game_dir.disk}`}
          />
        )}
        <Fact
          label={tr('Prefix')}
          value={home(info.prefix.path)}
          mono
          detail={
            info.prefix.exists
              ? `${info.prefix.disk} · ${tr('{free} free', { free: formatBytes(info.prefix.free ?? 0) })}`
              : tr('Not created yet: Proton creates it the first time the game runs.')
          }
        />
      </dl>
      <div className="inst-list">
        <Choice label={tr('Compatibility')} value={toolName} disabled={!info.steam_api || info.busy} onOpen={() => open('tool')} />
        <Choice
          label={tr('Components')}
          value={comp?.running ? tr('Installing…') : installed.length ? installed.join(', ') : tr('None installed yet')}
          onOpen={() => open('components')}
        />
      </div>
      <div className="row">
        <button data-nav className="btn" disabled={busy || info.busy || !info.prefix.exists} onClick={() => open('installer')}>
          <Icon name="file" /> {tr('Run an installer in this prefix…')}
        </button>
        <button
          data-nav
          className="btn"
          disabled={busy || !info.steam_api}
          onClick={() => void run(() => api.installArtwork(hash), tr('Artwork updated'), artSummary)}
        >
          <Icon name="display" /> {tr('Refresh artwork')}
        </button>
      </div>

      {job?.state === 'running' ? (
        <div className="inst-move">
          <p>{trb('Moving to **{to}**…', { to: info.targets.find(t => t.to === job.to)?.windows ?? home(job.to) })}</p>
          <Progress value={job.total ? job.done / job.total : 0} />
          <p className="muted">{tr('{done} of {total}', { done: formatBytes(job.done), total: formatBytes(job.total) })}</p>
          <div className="row">
            <button data-nav className="btn danger" onClick={() => void run(() => api.installMoveCancel(hash))}>
              {tr('Cancel')}
            </button>
          </div>
        </div>
      ) : (
        info.targets.length > 0 && (
          <div className="inst-list">
            <Choice label={tr('Location')} value={where} disabled={busy || info.busy} onOpen={() => open('move')} />
            {job?.state === 'failed' && <p className="inst-error">{job.error}</p>}
          </div>
        )
      )}

      {dialog === 'tool' && (
        <ChooseDialog
          title={tr('Compatibility')}
          options={info.tools.map(x => ({ value: x.name, label: x.display, detail: x.installed ? tr('Installed') : tr('Downloads on first use') }))}
          value={info.tool}
          onPick={v => {
            close()
            if (v !== info.tool) {
              void run(async () => {
                await api.installTool(hash, v)
                await load()
              }, tr('Compatibility tool changed'))
            }
          }}
          onClose={close}
        />
      )}
      {dialog === 'move' && (
        <ChooseDialog
          title={tr('Move the game')}
          note={tr('The game’s files move; the Steam shortcut’s target and start folder follow them.')}
          options={info.targets
            .filter(t => !t.here)
            .map(t => ({
              value: t.id,
              label: t.label,
              disabled: !!t.blocked,
              detail:
                t.blocked ??
                `${t.windows} · ${
                  t.same_disk
                    ? tr('Same disk: it takes a moment')
                    : tr('Copies {size} · {free} free there', { size: formatBytes(info.game_dir?.size ?? 0), free: formatBytes(t.free ?? 0) })
                }`,
            }))}
          value=""
          onPick={to => {
            close()
            void run(async () => {
              await api.installMove(hash, to)
              await load()
            })
          }}
          onClose={close}
        />
      )}
      {dialog === 'components' && (
        <ComponentsDialog
          hash={hash}
          status={comp}
          onRefresh={loadComponents}
          onRunInstaller={() => setDialog('installer')}
          onClose={close}
        />
      )}
      {dialog === 'installer' && (
        <FilePicker
          title={tr('Pick an installer to run in this prefix')}
          start={downloadDir}
          accept={INSTALLERS}
          onClose={close}
          onPick={path => {
            close()
            void run(
              async () => {
                await api.installRun(hash, path)
                await load()
              },
              tr('Running {name}', { name: baseName(path) }),
            )
          }}
        />
      )}
    </section>
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
