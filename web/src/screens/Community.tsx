// Community: games set up by installers the community writes in piShop's
// repository (a console game behind a wrapper, a decompilation that needs the
// player's own ROM). Discover's look: a featured game up top (L2/R2), the
// catalog as a grid, how it works, and a game's page with what it downloads,
// its SteamOS notes, its script and its troubleshooting reports.

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import {
  type CommunityGame, type CommunityInstall, type CommunityIssue, type CommunitySetup, type GameMeta, api, formatBytes, imgUrl,
} from '../api'
import { partKind } from '../community'
import { locale, tr, trn } from '../i18n'
import { focusFirst, input, keepFocus } from '../input'
import { StorageCard, homePath } from '../storage'
import { Dialog, Glyph, Icon, Spinner, toast, useHints } from '../ui'
import CommunityInstallPage, { pickStart } from './CommunityInstall'
import FilePicker, { type SourcePick } from './FilePicker'
import { ChooseDialog, Choice } from './GameSetup'

const ROTATE_MS = 9000

const memory: { games: CommunityGame[]; featured: number; open: string | null } = { games: [], featured: 0, open: null }

// SteamGridDB art by name (the manifest's own art wins), once per game.
const artCache = new Map<string, Promise<GameMeta | null>>()
function artFor(name: string) {
  let p = artCache.get(name)
  if (!p) {
    p = api.gameInfo(name, false).catch(() => null)
    artCache.set(name, p)
  }
  return p
}

function useArt(g: CommunityGame | null | undefined) {
  const [meta, setMeta] = useState<GameMeta | null>(null)
  useEffect(() => {
    if (!g) return
    let alive = true
    setMeta(null)
    void artFor(g.name).then(m => alive && setMeta(m))
    return () => {
      alive = false
    }
  }, [g?.name])
  return {
    cover: g?.art.cover ?? meta?.art?.cover ?? null,
    hero: g?.art.hero ?? meta?.hero ?? null,
  }
}

const downloadSize = (g: CommunityGame) => g.parts.filter(p => p.kind !== 'user').reduce((s, p) => s + (p.size ?? 0), 0)
const userParts = (g: CommunityGame) => g.parts.filter(p => p.kind === 'user')

function hue(s: string) {
  let h = 0
  for (const c of s) h = (h * 31 + c.charCodeAt(0)) % 360
  return h
}

export default function Community({
  onOpenGame,
  onExplore,
  onGoTransfers,
}: {
  /** Games, on the installed game's page. */
  onOpenGame: (appid: number) => void
  onExplore: (path: string) => void
  onGoTransfers: () => void
}) {
  const [games, setGames] = useState<CommunityGame[]>(memory.games)
  const [warning, setWarning] = useState<string | null>(null)
  const [contribute, setContribute] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [featured, setFeatured] = useState(memory.featured)
  const [openId, setOpenId] = useState<string | null>(memory.open)
  const open = games.find(g => g.id === openId) ?? null

  useEffect(() => {
    Object.assign(memory, { games, featured, open: openId })
  }, [games, featured, openId])

  const load = useCallback(async (fresh = false) => {
    setLoading(true)
    setError(null)
    try {
      const c = await api.community(fresh)
      setGames(c.games)
      setWarning(c.warning)
      setContribute(c.contribute)
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setLoading(false)
    }
  }, [])
  useEffect(() => {
    void load()
  }, [load])
  // Installs move on: their status on the cards follows.
  const installing = games.some(g => g.status.state === 'installing')
  useEffect(() => {
    if (!installing) return
    const id = setInterval(() => void load(), 3000)
    return () => clearInterval(id)
  }, [installing, load])
  const landed = useRef(false)
  useEffect(() => {
    if (!landed.current && games.length) {
      landed.current = true
      requestAnimationFrame(focusFirst)
    }
  }, [games.length])

  const hero = games.length ? games[featured % games.length] : null
  const heroArt = useArt(hero)
  useEffect(() => {
    if (openId || games.length < 2) return
    const id = setInterval(() => setFeatured(f => (f + 1) % games.length), ROTATE_MS)
    return () => clearInterval(id)
  }, [openId, games.length, featured])

  const restoreFocus = useRef<(() => void) | null>(null)
  const openGame = (g: CommunityGame) => {
    restoreFocus.current = keepFocus()
    setOpenId(g.id)
  }
  const closeGame = () => {
    setOpenId(null)
    void load()
    ;(restoreFocus.current ?? (() => requestAnimationFrame(focusFirst)))()
    restoreFocus.current = null
  }

  const ctl = useRef({ n: games.length, load })
  ctl.current = { n: games.length, load }
  useEffect(() => {
    if (openId) return
    return input.pushHandler(a => {
      const c = ctl.current
      if ((a === 'lt' || a === 'rt') && c.n > 1) {
        setFeatured(f => (f + (a === 'rt' ? 1 : -1) + c.n) % c.n)
        return true
      }
      if (a === 'menu') {
        void c.load(true)
        return true
      }
      return false
    })
  }, [openId])
  useHints(
    openId
      ? null
      : [
          { glyph: 'A', label: tr('View game') },
          { glyph: 'MENU', label: tr('Refresh') },
          ...(games.length > 1 ? [{ glyph: ['L2', 'R2'] as ['L2', 'R2'], label: tr('Spotlight') }] : []),
          { glyph: 'L1', label: tr('Tabs') },
        ],
  )

  return (
    <div className="dsc cm" data-nav-scope>
      <div className="dsc-scroll">
        {hero && (
          <section className="dsc-hero cm-hero">
            <div key={hero.id} className="dsc-hero-bg" style={heroArt.hero ? { backgroundImage: `url(${imgUrl(heroArt.hero)})` } : undefined} />
            <div className="dsc-hero-shade" />
            <div className="dsc-hero-content" key={`c${hero.id}`}>
              <span className="hero-kicker">
                {[tr('Community'), hero.platform, hero.wrapper ? tr('via {name}', { name: hero.wrapper.name }) : null].filter(Boolean).join(' · ')}
              </span>
              <h1>{hero.name}</h1>
              <p className="cm-hero-summary">{hero.summary}</p>
              <div className="dsc-hero-meta">
                {[
                  tr('{size} to download', { size: formatBytes(downloadSize(hero)) }),
                  userParts(hero).length ? trn(userParts(hero).length, 'needs {n} file of yours', 'needs {n} files of yours') : null,
                  hero.authors.length ? tr('by {who}', { who: hero.authors.join(', ') }) : null,
                ]
                  .filter(Boolean)
                  .join('  ·  ')}
              </div>
              <div className="row">
                <button data-nav data-nav-default className="btn primary big" onClick={() => openGame(hero)}>
                  {hero.status.state === 'installed' ? (
                    <>
                      <Icon name="check" /> {tr('Installed')}
                    </>
                  ) : hero.status.state === 'installing' ? (
                    <>
                      <Spinner /> {tr('Installing')}
                    </>
                  ) : (
                    tr('View game')
                  )}
                </button>
              </div>
            </div>
            {games.length > 1 && (
              <div className="dsc-carousel">
                <button className="dsc-flip" tabIndex={-1} onClick={() => setFeatured(f => (f - 1 + games.length) % games.length)} aria-label={tr('Previous featured game')}>
                  <Glyph name="L2" /> ‹
                </button>
                <div className="dsc-dots">
                  {games.map((g, i) => (
                    <span key={g.id} className={i === featured % games.length ? 'on' : ''} />
                  ))}
                </div>
                <button className="dsc-flip" tabIndex={-1} onClick={() => setFeatured(f => (f + 1) % games.length)} aria-label={tr('Next featured game')}>
                  › <Glyph name="R2" />
                </button>
              </div>
            )}
          </section>
        )}

        {warning && <p className="cm-warning">{warning}</p>}
        {error && !games.length && (
          <div className="store-msg error cm-error">
            <b>{tr("Couldn't load the community catalog")}</b>
            <span>{error}</span>
            <div className="row">
              <button data-nav data-nav-default className="btn" onClick={() => void load(true)}>
                {tr('Try again')}
              </button>
            </div>
          </div>
        )}
        {loading && !games.length && !error && (
          <div className="store-msg cm-error">
            <Spinner /> {tr('Loading…')}
          </div>
        )}

        {games.length > 0 && (
          <section className="dsc-catalog">
            <div className="dsc-catalog-head">
              <h3 className="dsc-row-title">{tr('Community installers')}</h3>
              <span className="sort-label">{trn(games.length, '{n} game', '{n} games')}</span>
            </div>
            <div className="grid">
              {games.map(g => (
                <CommunityCard key={g.id} g={g} onOpen={() => openGame(g)} />
              ))}
            </div>
          </section>
        )}

        <section className="cm-how">
          <div className="cm-how-item">
            <Icon name="shield" size={28} />
            <b>{tr('Reviewed in the open')}</b>
            <span>{tr('Every installer is a pull request on piShop’s GitHub, checked by Copilot and the maintainers before it shows up here.')}</span>
          </div>
          <div className="cm-how-item">
            <Icon name="prefix" size={28} />
            <b>{tr('Runs in a sandbox')}</b>
            <span>{tr('Install scripts can’t reach the network or your files: only the game’s own folder.')}</span>
          </div>
          <div className="cm-how-item">
            <Icon name="check" size={28} />
            <b>{tr('Checked files')}</b>
            <span>{tr('Downloads and the files you pick are compared with their checksums before anything runs.')}</span>
          </div>
          {contribute && (
            <div className="cm-how-item">
              <Icon name="plus" size={28} />
              <b>{tr('Add a game')}</b>
              <span>{tr('Write a manifest and a script in community/ and open a pull request.')}</span>
              <button data-nav className="btn small" onClick={() => void api.openGithub(contribute).catch(e => toast(tr('Something went wrong'), (e as Error).message, 'error'))}>
                <Icon name="globe" size={16} /> {tr('Open on GitHub')}
              </button>
            </div>
          )}
        </section>
      </div>

      {open && (
        <CommunityGamePage
          game={open}
          onClose={closeGame}
          onChanged={() => void load()}
          onOpenGame={onOpenGame}
          onExplore={onExplore}
          onGoTransfers={onGoTransfers}
        />
      )}
    </div>
  )
}

function CommunityCard({ g, onOpen }: { g: CommunityGame; onOpen: () => void }) {
  const art = useArt(g)
  const [loaded, setLoaded] = useState(false)
  const [failed, setFailed] = useState(false)
  const s = g.status
  return (
    <button data-nav className="card poster cm-card" onClick={onOpen}>
      <div className="cover" style={{ ['--h' as string]: hue(g.name) }}>
        {(!art.cover || failed) && (
          <div className="cover-placeholder">
            <Icon name="pad" size={36} />
            <b>{g.name}</b>
          </div>
        )}
        {art.cover && !failed && (
          <img
            src={imgUrl(art.cover)}
            alt=""
            loading="lazy"
            className={loaded ? 'in' : ''}
            onLoad={() => setLoaded(true)}
            onError={() => setFailed(true)}
            draggable={false}
          />
        )}
        <span className="plat">{g.platform}</span>
        {s.state === 'installed' && (
          <span className="cm-state done">
            <Icon name="check" size={16} />
          </span>
        )}
        {s.state === 'installing' && <span className="cm-state">{s.progress != null ? `${Math.floor((s.progress ?? 0) * 100)}%` : <Spinner />}</span>}
      </div>
      <div className="card-info">
        <b className="card-title">{g.name}</b>
        <div className="card-meta">
          <span className="seed">{g.wrapper?.name ?? g.platform}</span>
          {userParts(g).length > 0 && <span className="size">{tr('Your file')}</span>}
        </div>
      </div>
    </button>
  )
}

// ---------- a game's page ----------

function CommunityGamePage({
  game: g,
  onClose,
  onChanged,
  onOpenGame,
  onExplore,
  onGoTransfers,
}: {
  game: CommunityGame
  onClose: () => void
  onChanged: () => void
  onOpenGame: (appid: number) => void
  onExplore: (path: string) => void
  onGoTransfers: () => void
}) {
  const art = useArt(g)
  const [panel, setPanel] = useState<null | 'setup' | 'script' | 'help' | 'progress'>(null)
  const [install, setInstall] = useState<CommunityInstall | null>(null)
  const actionsRef = useRef<HTMLDivElement>(null)

  // The install's progress while it's on screen.
  const watching = panel === 'progress' || g.status.state === 'installing'
  useEffect(() => {
    if (!watching) return
    let alive = true
    const tick = () =>
      api
        .communityInstalls()
        .then(list => {
          if (!alive) return
          const mine = list.find(i => i.id === g.id) ?? null
          setInstall(mine)
          if (mine?.stage === 'installed' && g.status.state !== 'installed') onChanged()
        })
        .catch(() => {})
    void tick()
    const id = setInterval(tick, 1000)
    return () => {
      alive = false
      clearInterval(id)
    }
  }, [watching, g.id, g.status.state, onChanged])

  const close = useRef(onClose)
  close.current = onClose
  useEffect(() => {
    if (panel) return
    return input.pushModal(() => close.current(), { tabs: true })
  }, [panel])
  useEffect(() => {
    if (!panel) requestAnimationFrame(() => actionsRef.current?.querySelector<HTMLElement>('[data-nav-default]')?.focus() ?? focusFirst())
  }, [panel])
  useHints(panel ? null : [{ glyph: 'A', label: tr('Select') }, { glyph: 'B', label: tr('Back') }])

  const restore = useRef<(() => void) | null>(null)
  const show = (p: typeof panel) => {
    restore.current = keepFocus()
    setPanel(p)
  }
  const hide = () => {
    setPanel(null)
    onChanged()
  }

  const state = install && install.stage !== 'installed' ? 'installing' : g.status.state
  const play = async () => {
    try {
      if (g.status.key) await api.installPlay(g.status.key)
    } catch (e) {
      toast(tr("Couldn't start the game"), (e as Error).message, 'error')
    }
  }
  const dl = downloadSize(g)
  const yours = userParts(g)

  return (
    <div className="gd cmg" data-nav-scope>
      <div className="gd-bg" style={art.hero ? { backgroundImage: `url(${imgUrl(art.hero)})` } : undefined} />
      <section className="gd-details cmg-details">
        <div className="gd-body">
          <div className="gd-cover">{art.cover ? <img src={imgUrl(art.cover)} alt="" draggable={false} /> : <Icon name="pad" size={64} />}</div>
          <div className="gd-main cmg-main">
            <span className="hero-kicker">
              {[tr('Community'), g.platform, g.wrapper ? tr('via {name}', { name: [g.wrapper.name, g.wrapper.version].filter(Boolean).join(' ') }) : null]
                .filter(Boolean)
                .join(' · ')}
            </span>
            <h1>{g.name}</h1>
            <div className="gd-byline">
              {[g.authors.length ? tr('by {who}', { who: g.authors.join(', ') }) : null, g.updated ? tr('updated {date}', { date: fmtDate(g.updated) }) : null, tr('revision {n}', { n: g.revision })]
                .filter(Boolean)
                .join('  ·  ')}
            </div>
            <div className="gd-chips">
              <span className="gd-chip">
                <small>{tr('Platform')}</small>
                {g.platform}
              </span>
              {g.wrapper && (
                <span className="gd-chip">
                  <small>{tr('Runs with')}</small>
                  {g.wrapper.name}
                </span>
              )}
              <span className="gd-chip">
                <small>{tr('Download size')}</small>
                {formatBytes(dl)}
              </span>
              {g.install.disk ? (
                <span className="gd-chip">
                  <small>{tr('Installed size')}</small>~{formatBytes(g.install.disk)}
                </span>
              ) : null}
              {g.requirements.ram_gb ? (
                <span className="gd-chip">
                  <small>RAM</small>
                  {g.requirements.ram_gb} GB
                </span>
              ) : null}
              {yours.length > 0 && (
                <span className="gd-chip warn">
                  <small>{tr('Needs')}</small>
                  {trn(yours.length, '{n} file of yours', '{n} files of yours')}
                </span>
              )}
            </div>

            <div className="row gd-actions" ref={actionsRef}>
              {state === 'installed' ? (
                <>
                  <button data-nav data-nav-default className="btn primary big" onClick={() => void play()}>
                    <Icon name="play" /> {tr('Play')}
                  </button>
                  {g.status.appid && (
                    <button data-nav className="btn big" onClick={() => onOpenGame(g.status.appid!)}>
                      <Icon name="pad" /> {tr('Open in Games')}
                    </button>
                  )}
                </>
              ) : state === 'installing' ? (
                <button data-nav data-nav-default className="btn primary big" onClick={() => show('progress')}>
                  <Spinner /> {tr('Installing')} {install && install.stage === 'downloading' && install.total ? `${Math.floor((install.done / install.total) * 100)}%` : ''}
                </button>
              ) : (
                <button data-nav data-nav-default className="btn primary big" onClick={() => show('setup')}>
                  <Icon name="download" /> {tr('Install')}
                </button>
              )}
              <button data-nav className="btn big" onClick={() => show('help')}>
                <Icon name="info" /> {tr('Troubleshooting')}
              </button>
              <button data-nav className="btn big" onClick={() => show('script')}>
                <Icon name="file" /> {tr('View script')}
              </button>
              <button data-nav className="btn big" onClick={onClose}>
                {tr('Back')}
              </button>
            </div>

            <div className="cmg-info">
              <div className="cmg-text">
                <p className="cmg-description">{g.description}</p>
                {g.steamos.length > 0 && (
                  <>
                    <h4 className="cmg-sub">{tr('On SteamOS')}</h4>
                    <ul className="cmg-notes">
                      {g.steamos.map(n => (
                        <li key={n}>{n}</li>
                      ))}
                    </ul>
                  </>
                )}
                {g.requirements.note && <p className="muted small">{g.requirements.note}</p>}
              </div>
              <div className="cmg-parts">
                <h4 className="cmg-sub">{tr('What piShop gets')}</h4>
                {g.parts.map(p => (
                  <div key={p.id} className={`cmg-part ${p.kind}`}>
                    <Icon name={p.kind === 'user' ? 'sd' : p.kind === 'torrent' ? 'torrent' : 'download'} />
                    <span className="inst-row-main">
                      <b>{p.name}</b>
                      <small>
                        {p.kind === 'user'
                          ? p.source.hint || tr('Your own copy')
                          : [...new Set([partKind({ kind: p.kind, provided: false }), p.from].filter(Boolean))].join(' · ')}
                      </small>
                    </span>
                    <span className="inst-row-side">{p.kind === 'user' ? tr('You pick it') : p.size ? formatBytes(p.size) : ''}</span>
                  </div>
                ))}
                <p className="muted small">
                  {g.launch.runtime === 'native' ? tr('A native Linux game.') : tr('Runs with Proton.')}{' '}
                  {tr('piShop adds it to Steam with its artwork when everything is ready.')}
                </p>
              </div>
            </div>
          </div>
        </div>
      </section>

      {panel === 'setup' && (
        <SetupFlow
          game={g}
          onClose={() => {
            hide()
            restore.current?.()
          }}
          onStarted={i => {
            setInstall(i)
            setPanel('progress')
            onChanged()
          }}
        />
      )}
      {panel === 'progress' && install && (
        <CommunityInstallPage
          install={install}
          onClose={() => {
            hide()
            restore.current?.()
          }}
          onExplore={onExplore}
          onOpenGame={onOpenGame}
        />
      )}
      {panel === 'progress' && !install && (
        <Dialog title={tr('Installing')} onClose={hide}>
          <p className="muted">
            <Spinner /> {tr('Loading…')}
          </p>
          <div className="row">
            <button data-nav className="btn" onClick={onGoTransfers}>
              {tr('Open Transfers')}
            </button>
          </div>
        </Dialog>
      )}
      {panel === 'script' && <ScriptDialog game={g} onClose={() => (hide(), restore.current?.())} />}
      {panel === 'help' && <TroubleshootPanel game={g} onClose={() => (hide(), restore.current?.())} />}
    </div>
  )
}

function fmtDate(iso: string) {
  const d = new Date(iso.length === 10 ? `${iso}T12:00:00` : iso)
  return isNaN(d.getTime()) ? iso : d.toLocaleDateString(locale(), { day: '2-digit', month: 'short', year: 'numeric' })
}

// ---------- installing: where, which Proton, the user's files ----------

type FileCheck = { path: string; state: 'checking' | 'ok' | 'error'; msg?: string; partial?: boolean; size?: number; source?: SourcePick }

function SetupFlow({ game: g, onClose, onStarted }: { game: CommunityGame; onClose: () => void; onStarted: (i: CommunityInstall) => void }) {
  const [setup, setSetup] = useState<CommunitySetup | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [library, setLibrary] = useState('')
  const [tool, setTool] = useState('')
  const [files, setFiles] = useState<Record<string, FileCheck>>({})
  const [view, setView] = useState<'setup' | 'library' | 'tool' | { part: string }>('setup')
  const [busy, setBusy] = useState(false)

  const load = useCallback(() => {
    api
      .communitySetup(g.id)
      .then(s => {
        setSetup(s)
        setLibrary(l => l || s.default_library || '')
        setTool(t => t || s.default_tool || '')
        setError(null)
      })
      .catch(e => setError((e as Error).message))
  }, [g.id])
  useEffect(load, [load])

  const lib = setup?.libraries.find(l => l.path === library)
  const yours = userParts(g)
  // Parts piShop downloads, which can start from a file the user already has.
  const fetchable = g.parts.filter(p => p.kind !== 'user')
  const proton = g.launch.runtime === 'proton'
  const sameDisk = !!lib && !!setup && setup.downloads.disk === lib.label
  const need = (setup?.install_size ?? 0) + (sameDisk ? (setup?.download_size ?? 0) : 0)
  const libWarn = lib?.free != null && need > 0 && lib.free < need ? tr('Not enough free space there.') : null
  const dlWarn =
    setup && !sameDisk && setup.downloads.free != null && setup.downloads.free < setup.download_size ? tr('Not enough free space there.') : null
  const filesOk = yours.every(p => files[p.id]?.state === 'ok')
  const ready = !!setup && !!library && (!proton || !!tool) && setup.steam_api && setup.bwrap && filesOk && !busy

  const pickFor = (part: string, path: string) => {
    setFiles(f => ({ ...f, [part]: { path, state: 'checking' } }))
    api
      .communityCheck(g.id, part, path)
      .then(r => setFiles(f => ({ ...f, [part]: { path, state: 'ok', partial: r.partial, size: r.size } })))
      .catch(e => setFiles(f => ({ ...f, [part]: { path, state: 'error', msg: (e as Error).message } })))
  }
  const start = async () => {
    setBusy(true)
    try {
      const ok = Object.entries(files).filter(([, c]) => c.state === 'ok')
      const paths = Object.fromEntries(ok.filter(([, c]) => !c.source).map(([k, c]) => [k, c.path]))
      const sources = Object.fromEntries(
        ok.filter(([, c]) => c.source).map(([k, c]) => [k, { source_id: c.source!.source_id, path: c.source!.path, name: c.source!.name, size: c.source!.size }]),
      )
      const i = await api.communityInstall(g.id, { library, tool, files: paths, sources })
      toast(tr('Installing {name}', { name: g.name }), tr('Follow it here or in Transfers.'), 'ok')
      onStarted(i)
    } catch (e) {
      toast(tr("Couldn't start the installation"), (e as Error).message, 'error')
      setBusy(false)
    }
  }

  if (view === 'library' && setup) {
    return (
      <Dialog title={tr('Install to')} onClose={() => setView('setup')} wide>
        <p className="muted">{tr('The game goes into a piShop folder in the Steam library you pick.')}</p>
        <div className="inst-list scroll">
          {setup.libraries.map(l => (
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
                setView('setup')
              }}
            />
          ))}
        </div>
      </Dialog>
    )
  }
  if (view === 'tool' && setup) {
    return (
      <ChooseDialog
        title={tr('Compatibility')}
        options={setup.tools.map(x => ({ value: x.name, label: x.display, detail: x.installed ? tr('Installed') : tr('Downloads on first use') }))}
        value={tool}
        onPick={v => {
          setTool(v)
          setView('setup')
        }}
        onClose={() => setView('setup')}
      />
    )
  }
  if (typeof view === 'object' && setup) {
    const p = g.parts.find(x => x.id === view.part)!
    return (
      <FilePicker
        title={tr('Pick {name}', { name: p.name })}
        accept={p.source.extensions?.length ? p.source.extensions : ['']}
        start={files[p.id]?.path?.replace(/\/[^/]+$/, '') || pickStart(setup.downloads.path)}
        onClose={() => setView('setup')}
        onPick={path => {
          setView('setup')
          pickFor(p.id, path)
        }}
        onPickSource={src => {
          setView('setup')
          const exts = p.source.extensions ?? []
          const bad = p.kind === 'user' && exts.length > 0 && !exts.some(x => src.name.toLowerCase().endsWith(x))
          setFiles(f => ({
            ...f,
            [p.id]: bad
              ? { path: src.path, state: 'error', msg: tr('this part takes a {types} file', { types: exts.join(' / ') }) }
              : { path: src.path, state: 'ok', size: src.size, source: src },
          }))
        }}
      />
    )
  }

  return (
    <Dialog title={tr('Install {name}', { name: g.name })} onClose={onClose} wide>
      {error && <p className="inst-error">{error}</p>}
      {!setup && !error && (
        <p className="muted">
          <Spinner /> {tr('Loading…')}
        </p>
      )}
      {setup && (
        <div className="cm-setup">
          {!setup.steam_api && (
            <div className="inst-banner">
              <p>
                {setup.debugging_enabled
                  ? tr('Steam’s client API is switched on but Steam hasn’t picked it up yet. Restart Steam to use it.')
                  : tr('To add games to your library, piShop uses Steam’s client API (the same one Decky Loader uses). Turning it on restarts Steam; open piShop again afterwards.')}
              </p>
              <button data-nav className="btn" onClick={() => void api.enableSteamApi().then(() => toast(tr('Restarting Steam…'), undefined, 'ok'))}>
                {tr('Turn on and restart Steam')}
              </button>
            </div>
          )}
          {!setup.bwrap && <p className="inst-error">{tr('bubblewrap (bwrap) isn’t on this system: piShop runs community scripts only inside its sandbox.')}</p>}

          {yours.length > 0 && (
            <>
              <h4 className="cmg-sub">{tr('Your files')}</h4>
              <p className="muted small">{tr('This game needs files that aren’t public. Point piShop at your own copy; it’s checked against the known good ones and stays where it is.')}</p>
              <div className="inst-list">
                {yours.map((p, i) => {
                  const c = files[p.id]
                  return (
                    <button key={p.id} data-nav data-nav-default={i === 0 && !c ? '' : undefined} className="inst-row" onClick={() => setView({ part: p.id })}>
                      <Icon name={c?.state === 'ok' ? 'check' : 'sd'} />
                      <span className="inst-row-main">
                        <b>{p.name}</b>
                        <small className={c?.state === 'error' ? 'error' : ''}>
                          {c?.state === 'checking'
                            ? tr('Checking…')
                            : c?.state === 'error'
                              ? c.msg
                              : c?.state === 'ok'
                                ? c.source
                                  ? tr('{source}: {path} · copied when the install starts', { source: c.source.source_name, path: c.source.path })
                                  : homePath(c.path)
                                : p.source.hint || tr('Your own copy')}
                        </small>
                      </span>
                      <span className="inst-row-side">{c?.state === 'ok' ? tr('Checked') : tr('Pick…')}</span>
                    </button>
                  )
                })}
              </div>
            </>
          )}

          {fetchable.length > 0 && (
            <>
              <h4 className="cmg-sub">{tr('Already have some of it?')}</h4>
              <p className="muted small">
                {tr('Point piShop at a file you already have, finished or still downloading (a browser’s .crdownload): it continues from there instead of starting over. Optional.')}
              </p>
              <div className="inst-list">
                {fetchable.map(p => {
                  const c = files[p.id]
                  return (
                    <button key={p.id} data-nav className="inst-row" onClick={() => setView({ part: p.id })}>
                      <Icon name={c?.state === 'ok' ? 'check' : 'download'} />
                      <span className="inst-row-main">
                        <b>{p.name}</b>
                        <small className={c?.state === 'error' ? 'error' : ''}>
                          {c?.state === 'checking'
                            ? tr('Checking…')
                            : c?.state === 'error'
                              ? c.msg
                              : c?.state === 'ok'
                                ? c.source
                                  ? tr('{source}: {path} · copied when the install starts', { source: c.source.source_name, path: c.source.path })
                                  : `${homePath(c.path)} · ${c.partial ? tr('continues from {size}', { size: formatBytes(c.size ?? 0) }) : tr('used as it is')}`
                                : [partKind({ kind: p.kind, provided: false }), p.size ? formatBytes(p.size) : null].filter(Boolean).join(' · ')}
                        </small>
                      </span>
                      <span className="inst-row-side">{c?.state === 'ok' ? tr('Checked') : tr('Use a file I already have…')}</span>
                    </button>
                  )
                })}
              </div>
            </>
          )}

          {lib && (
            <StorageCard
              icon="library"
              title={tr('Install to {place}', { place: lib.label })}
              path={`${lib.path}/piShop/${setup.folder}`}
              free={lib.free}
              total={lib.total}
              warn={libWarn}
              navDefault={yours.length === 0}
              onClick={() => setView('library')}
            />
          )}
          {setup.download_size > 0 && (
            <StorageCard
              icon="download"
              title={tr('{size} to download', { size: formatBytes(setup.download_size) })}
              path={setup.downloads.path}
              free={setup.downloads.free}
              total={setup.downloads.total}
              disk={setup.downloads.disk}
              warn={dlWarn}
            />
          )}
          {proton && <Choice label={tr('Compatibility')} value={setup.tools.find(t => t.name === tool)?.display ?? '—'} onOpen={() => setView('tool')} />}
          <p className="muted small">
            {setup.install_size ? tr('The installed game takes about {size}.', { size: formatBytes(setup.install_size) }) : ''}
          </p>
        </div>
      )}
      <div className="dialog-actions">
        <button data-nav className="btn" onClick={onClose}>
          {tr('Cancel')}
        </button>
        <button data-nav className="btn primary" disabled={!ready} onClick={() => void start()}>
          {busy ? <Spinner /> : <Icon name="download" size={18} />} {tr('Start installation')}
        </button>
      </div>
    </Dialog>
  )
}

// ---------- the script ----------

function ScriptDialog({ game, onClose }: { game: CommunityGame; onClose: () => void }) {
  const [script, setScript] = useState<{ name: string; source: string; sha256: string; revision: number } | null>(null)
  const [error, setError] = useState<string | null>(null)
  useEffect(() => {
    api
      .communityScript(game.id)
      .then(setScript)
      .catch(e => setError((e as Error).message))
  }, [game.id])
  return (
    <Dialog title={tr('{name}’s install script', { name: game.name })} onClose={onClose} wide>
      <p className="muted small">{tr('This is exactly what runs, inside the sandbox: no network, only the game’s folder writable.')}</p>
      {error && <p className="inst-error">{error}</p>}
      {script ? (
        <>
          <pre className="inst-log cm-script" tabIndex={-1}>
            {script.source}
          </pre>
          <p className="muted small mono">
            {script.name} · {tr('revision {n}', { n: script.revision })} · SHA-256 {script.sha256.slice(0, 16)}…
          </p>
        </>
      ) : (
        !error && (
          <p className="muted">
            <Spinner /> {tr('Loading…')}
          </p>
        )
      )}
      <div className="dialog-actions">
        <button data-nav data-nav-default className="btn" onClick={onClose}>
          {tr('Close')}
        </button>
      </div>
    </Dialog>
  )
}

// ---------- troubleshooting: GitHub issues about this game ----------

function TroubleshootPanel({ game, onClose }: { game: CommunityGame; onClose: () => void }) {
  const [data, setData] = useState<{ issues: CommunityIssue[]; new_issue: string; label: string | null } | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [sel, setSel] = useState<number | null>(null)
  const [comments, setComments] = useState<Record<number, Array<{ user: string; created: string; body: string }> | 'loading' | string>>({})

  useEffect(() => {
    api
      .communityIssues(game.id)
      .then(d => {
        setData(d)
        if (d.issues.length) setSel(d.issues[0].number)
      })
      .catch(e => setError((e as Error).message))
  }, [game.id])
  const issue = data?.issues.find(i => i.number === sel) ?? null
  useEffect(() => {
    if (!issue || comments[issue.number] || !issue.comments) return
    setComments(c => ({ ...c, [issue.number]: 'loading' }))
    api
      .communityComments(issue.number)
      .then(list => setComments(c => ({ ...c, [issue.number]: list })))
      .catch(e => setComments(c => ({ ...c, [issue.number]: (e as Error).message })))
  }, [issue?.number])

  const close = useRef(onClose)
  close.current = onClose
  useEffect(() => input.pushModal(() => close.current()), [])
  useEffect(() => {
    if (data || error) requestAnimationFrame(focusFirst)
  }, [!!data, !!error])
  useHints([
    { glyph: 'A', label: tr('Select') },
    { glyph: 'B', label: tr('Back') },
  ])
  const openUrl = (u: string) => void api.openGithub(u).catch(e => toast(tr('Something went wrong'), (e as Error).message, 'error'))
  const c = issue ? comments[issue.number] : undefined
  const sorted = useMemo(() => data?.issues ?? [], [data])

  return (
    <div className="compat cmt" data-nav-scope>
      <header className="cmt-head">
        <div>
          <span className="hero-kicker">{tr('Troubleshooting')}</span>
          <h2>{game.name}</h2>
          <span className="muted small">
            {data?.label ? tr('Reports on piShop’s GitHub labelled {label}', { label: data.label }) : tr('Reports on piShop’s GitHub')}
          </span>
        </div>
        <div className="row">
          {data && (
            <button data-nav data-nav-default={!sorted.length ? '' : undefined} className="btn primary" onClick={() => openUrl(data.new_issue)}>
              <Icon name="plus" size={18} /> {tr('Report a problem')}
            </button>
          )}
          <button data-nav className="btn" onClick={onClose}>
            {tr('Back')}
          </button>
        </div>
      </header>
      {error && <p className="inst-error cmt-msg">{error}</p>}
      {!data && !error && (
        <p className="muted cmt-msg">
          <Spinner /> {tr('Loading…')}
        </p>
      )}
      {data && !sorted.length && (
        <div className="store-msg">
          <Icon name="check" size={36} />
          <b>{tr('No reports for this game yet')}</b>
          <span>{tr('If something doesn’t work, report it: the next player will find the answer here.')}</span>
        </div>
      )}
      {sorted.length > 0 && (
        <div className="cmt-body">
          <div className="cmt-list">
            {sorted.map((i, n) => (
              <button
                key={i.number}
                data-nav
                data-nav-default={n === 0 ? '' : undefined}
                className={`inst-row cmt-issue ${i.number === sel ? 'on' : ''}`}
                onFocus={() => setSel(i.number)}
                onClick={() => setSel(i.number)}
              >
                <span className={`cmt-dot ${i.state}`} />
                <span className="inst-row-main">
                  <b>{i.title}</b>
                  <small>
                    #{i.number} · {i.state === 'open' ? tr('open') : tr('solved')} · {trn(i.comments, '{n} comment', '{n} comments')} · {fmtDate(i.updated)}
                  </small>
                </span>
              </button>
            ))}
          </div>
          {issue && (
            <article className="cmt-issue-view" tabIndex={-1}>
              <h3>{issue.title}</h3>
              <span className="muted small">
                {tr('{user} · {date}', { user: issue.user, date: fmtDate(issue.created) })} · {issue.state === 'open' ? tr('open') : tr('solved')}
              </span>
              <p className="cmt-text">{tidy(issue.body) || tr('No description.')}</p>
              {issue.comments > 0 && <h4 className="cmg-sub">{trn(issue.comments, '{n} comment', '{n} comments')}</h4>}
              {c === 'loading' && (
                <p className="muted">
                  <Spinner /> {tr('Loading…')}
                </p>
              )}
              {typeof c === 'string' && c !== 'loading' && <p className="inst-error">{c}</p>}
              {Array.isArray(c) &&
                c.map((m, n) => (
                  <div key={n} className="cmt-comment">
                    <span className="muted small">{tr('{user} · {date}', { user: m.user, date: fmtDate(m.created) })}</span>
                    <p className="cmt-text">{tidy(m.body)}</p>
                  </div>
                ))}
              <div className="row">
                <button data-nav className="btn small" onClick={() => openUrl(issue.url)}>
                  <Icon name="globe" size={16} /> {tr('Open on GitHub')}
                </button>
              </div>
            </article>
          )}
        </div>
      )}
    </div>
  )
}

/** Issue text without the form's markup: headings become plain lines, "_No response_" goes. */
function tidy(md: string) {
  return md
    .replace(/<!--[\s\S]*?-->/g, '')
    .replace(/^#{1,6}\s*/gm, '')
    .replace(/^_No response_$/gm, '—')
    .replace(/```\w*\n?/g, '')
    .replace(/\*\*(.+?)\*\*/g, '$1')
    .replace(/\n{3,}/g, '\n\n')
    .trim()
}
