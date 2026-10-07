// Games: every non-Steam shortcut in Steam's library, as shelves of covers:
// the games installed through piShop, then every other shortcut. A opens the game's page: Play, Proton,
// components, its files and prefix (with their disks' free space), moving it,
// running installers in its prefix, artwork, removing it.

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { type ArtJob, type GameItem, type GamesList, type InstallStatus, api, formatBytes, imgUrl } from '../api'
import { locale, tr, trb, trn } from '../i18n'
import { focusFirst, input, keepFocus } from '../input'
import { homePath } from '../storage'
import { Icon, Spinner, toast, useHints } from '../ui'
import { ProtonBadge } from './Compat'
import FilePicker from './FilePicker'
import GameSetup from './GameSetup'
import PatchesPanel from './Patches'

/** Opens a game's page (Transfers → Open in Games). */
export interface GamesRequest {
  appid: number
  n: number
}
let handled = 0

function hue(s: string) {
  let h = 0
  for (const c of s) h = (h * 31 + c.charCodeAt(0)) % 360
  return h
}

/** Steam's artwork for the shortcut, else the game's art from the Store, else its name. */
function Cover({ g }: { g: GameItem }) {
  const sources = [g.art.cover, g.game?.cover ? imgUrl(g.game.cover) : null].filter(Boolean) as string[]
  const [i, setI] = useState(0)
  const src = sources[i]
  return src ? (
    <img src={src} alt="" draggable={false} onError={() => setI(n => n + 1)} />
  ) : (
    <div className="dl-cover-ph">
      <Icon name="pad" size={38} />
      <b>{g.name}</b>
    </div>
  )
}

/** "Proton Experimental", "Linux (native)"… */
function runsWith(g: GameItem, tools: GamesList['tools']) {
  if (g.tool) return tools.find(t => t.name === g.tool)?.display ?? g.tool
  return g.windows ? tr('Windows · no Proton set') : tr('Linux (native)')
}

const day = (secs: number) => new Date(secs * 1000).toLocaleDateString(locale(), { day: 'numeric', month: 'short', year: 'numeric' })

function played(g: GameItem) {
  return g.last_played ? tr('Played {date}', { date: day(g.last_played) }) : tr('Never played')
}

/** What's special about a game right now, if anything. */
function status(g: GameItem) {
  if (g.missing) return { text: tr('Not in Steam anymore'), cls: 'error' }
  if (g.running) return { text: tr('Running'), cls: 'done' }
  if (g.stage === 'installing') return { text: tr('Installing…'), cls: 'active' }
  return null
}

function GameTile({ g, tools, first, onOpen }: { g: GameItem; tools: GamesList['tools']; first: boolean; onOpen: () => void }) {
  const s = status(g) ?? { text: runsWith(g, tools), cls: '' }
  return (
    <div
      data-nav
      data-nav-default={first ? '' : undefined}
      data-appid={g.appid}
      tabIndex={0}
      className={`dl-tile game-tile ${g.missing ? 'missing' : ''}`}
      onClick={onOpen}
    >
      <div className="dl-cover" style={{ ['--h' as string]: hue(g.name) }}>
        <Cover g={g} />
        {g.running && <span className="game-badge run">{tr('Running')}</span>}
      </div>
      <div className="dl-info">
        <b className="dl-name">{g.name}</b>
        <span className={`dl-line ${s.cls}`}>{s.text}</span>
        <span className="dl-line sub">{played(g)}</span>
      </div>
    </div>
  )
}

export default function Games({
  request,
  onExplore,
}: {
  request?: GamesRequest | null
  /** Opens a folder in Explore (left pane). */
  onExplore: (path: string) => void
}) {
  const [list, setList] = useState<GamesList | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [openId, setOpenId] = useState<number | null>(null)
  const [focusId, setFocusId] = useState<number | null>(null)
  const [art, setArt] = useState<ArtJob | null>(null)
  const [patches, setPatches] = useState<GameItem | null>(null)
  const restore = useRef<(() => void) | null>(null)

  const load = useCallback(
    () =>
      api
        .games()
        .then(l => {
          setList(l)
          setError(null)
        })
        .catch(e => setError((e as Error).message)),
    [],
  )
  // Fresh while the tab is open (a game starts or stops, an install lands).
  useEffect(() => {
    void load()
    if (openId) return
    const id = setInterval(() => void load(), 5000)
    return () => clearInterval(id)
  }, [load, openId])

  const mine = useMemo(() => list?.games.filter(g => g.origin === 'pishop') ?? [], [list])
  const others = useMemo(() => list?.games.filter(g => g.origin === 'shortcut') ?? [], [list])
  const tools = list?.tools ?? []
  const focused = list?.games.find(g => g.appid === focusId) ?? mine[0] ?? others[0] ?? null
  const open = list?.games.find(g => g.appid === openId) ?? null

  // The list arrives after the tab opens: land on the first game then.
  const landed = useRef(false)
  useEffect(() => {
    if (landed.current || !list?.games.length) return
    landed.current = true
    const el = document.activeElement as HTMLElement | null
    if (!el || el === document.body || el.closest('.dl-head')) requestAnimationFrame(focusFirst)
  }, [list])

  // Transfers → Open in Games: straight to that game's page.
  useEffect(() => {
    if (!request || !list || request.n === handled) return
    const g = list.games.find(x => x.appid === request.appid)
    if (!g) return
    handled = request.n
    setFocusId(g.appid)
    setOpenId(g.appid)
  }, [request, list])

  const openGame = (g: GameItem) => {
    restore.current = keepFocus()
    setOpenId(g.appid)
  }
  const closeGame = () => {
    setOpenId(null)
    void load()
    requestAnimationFrame(() => restore.current?.())
  }

  // Bulk artwork refresh for piShop's installs.
  useEffect(() => {
    if (!art?.running) return
    const id = setInterval(() => {
      api
        .libraryArtworkStatus()
        .then(j => {
          setArt(j)
          if (!j.running) {
            toast(tr('Artwork updated'), trn(j.done, '{n} game', '{n} games'), 'ok')
            void load()
          }
        })
        .catch(() => {})
    }, 1500)
    return () => clearInterval(id)
  }, [art?.running, load])

  // X plays the focused game, Y opens its patches.
  const ref = useRef({ focused, open, patches })
  ref.current = { focused, open, patches }
  useEffect(
    () =>
      input.pushHandler(a => {
        const { focused: g, open, patches } = ref.current
        if (open || patches || !g || (a !== 'x' && a !== 'y')) return false
        const el = document.activeElement as HTMLElement | null
        if (!el?.dataset.appid || g.missing) return true
        if (a === 'y') {
          restore.current = keepFocus()
          setPatches(g)
          return true
        }
        api
          .gameManage(g.appid)
          .then(r => api.installPlay(r.key))
          .catch(e => toast(tr("Couldn't start the game"), (e as Error).message, 'error'))
        return true
      }),
    [],
  )
  const onTile = !!focused && !open
  useHints(
    open || patches
      ? null
      : [
          ...(onTile ? [{ glyph: 'A' as const, label: tr('Open') }] : []),
          ...(onTile && !focused?.missing ? [{ glyph: 'X' as const, label: focused?.running ? tr('Resume') : tr('Play') }] : []),
          ...(onTile && !focused?.missing ? [{ glyph: 'Y' as const, label: tr('Patches') }] : []),
          { glyph: 'B', label: tr('Back') },
        ],
  )

  const shelf = (games: GameItem[], firstShelf: boolean) => (
    <div className="dl-grid">
      {games.map((g, i) => (
        <GameTile key={g.appid} g={g} tools={tools} first={firstShelf && i === 0} onOpen={() => openGame(g)} />
      ))}
    </div>
  )

  return (
    <div
      className="jobs transfers games"
      data-nav-scope
      onFocusCapture={e => {
        const id = (e.target as HTMLElement).dataset.appid
        if (id) setFocusId(Number(id))
      }}
    >
      <div className="transfers-scroll">
        {error && <p className="inst-error">{error}</p>}
        {!list && !error && (
          <p className="muted">
            <Spinner /> {tr('Loading…')}
          </p>
        )}
        {list && !list.steam_api && (
          <div className="inst-banner">
            <p>{tr('Steam’s client API is off: the list comes from Steam’s files and the games can’t be changed. Turning it on restarts Steam.')}</p>
            <button
              data-nav
              className="btn"
              onClick={() =>
                void api
                  .enableSteamApi()
                  .then(() => toast(tr('Restarting Steam…'), '', 'ok'))
                  .catch(e => toast(tr('Something went wrong'), (e as Error).message, 'error'))
              }
            >
              {tr('Turn on and restart Steam')}
            </button>
          </div>
        )}
        {list && (
          <>
            <section className="dl-section">
              <div className="dl-head">
                <h3 className="section-title">{tr('Installed through piShop')}</h3>
                <span className="dl-head-stats">{trn(mine.length, '{n} game', '{n} games')}</span>
                {mine.some(g => !g.missing) && list.steam_api && (
                  <button
                    data-nav
                    data-nav-default={list.games.length ? undefined : ''}
                    className="btn small"
                    disabled={!!art?.running}
                    onClick={() =>
                      void api
                        .libraryArtwork()
                        .then(setArt)
                        .catch(e => toast(tr('Something went wrong'), (e as Error).message, 'error'))
                    }
                  >
                    <Icon name="refresh" size={18} />{' '}
                    {art?.running ? tr('Artwork {done}/{total}…', { done: art.done, total: art.total }) : tr('Refresh artwork')}
                  </button>
                )}
              </div>
              {mine.length ? (
                shelf(mine, true)
              ) : (
                <div className="jobs-empty">
                  <Icon name="download" size={28} />
                  <span>{tr('Nothing installed through piShop yet. Download a game in the Store and install it from Transfers.')}</span>
                </div>
              )}
            </section>
            <section className="dl-section">
              <div className="dl-head">
                <h3 className="section-title">{tr('Other shortcuts')}</h3>
                <span className="dl-head-stats">{tr('Non-Steam games added some other way: emulators, other launchers, by hand')}</span>
              </div>
              {others.length ? (
                shelf(others, !mine.length)
              ) : (
                <div className="jobs-empty">
                  <Icon name="pad" size={28} />
                  <span>{tr('No other non-Steam shortcuts in your library.')}</span>
                </div>
              )}
            </section>
          </>
        )}
      </div>
      {open && <GamePage key={open.appid} item={open} onClose={closeGame} onExplore={onExplore} onChanged={() => void load()} />}
      {patches && (
        <PatchesPanel
          appid={patches.appid}
          name={patches.name}
          onClose={() => {
            setPatches(null)
            requestAnimationFrame(() => restore.current?.())
          }}
        />
      )}
    </div>
  )
}

/** One game: Play, the installer's state while installing, and its whole setup. */
function GamePage({
  item,
  onClose,
  onExplore,
  onChanged,
}: {
  item: GameItem
  onClose: () => void
  onExplore: (path: string) => void
  onChanged: () => void
}) {
  const [key, setKey] = useState<string | null>(item.missing ? item.key : null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [status, setStatus] = useState<InstallStatus | null>(null)
  const [picker, setPicker] = useState(false)
  const g = item.game

  useEffect(() => {
    if (item.missing) return
    api
      .gameManage(item.appid)
      .then(r => setKey(r.key))
      .catch(e => setError((e as Error).message))
  }, [item.appid, item.missing])

  // A piShop install still waiting for its executable: follow the installer.
  const installing = item.origin === 'pishop' && item.stage === 'installing' && status?.state?.stage !== 'installed'
  useEffect(() => {
    if (!key || !installing) return
    let alive = true
    const tick = () =>
      api
        .installStatus(key)
        .then(s => alive && setStatus(s))
        .catch(() => {})
    void tick()
    const id = setInterval(tick, 2000)
    return () => {
      alive = false
      clearInterval(id)
    }
  }, [key, installing])

  const close = useRef(onClose)
  close.current = onClose
  useEffect(() => input.pushModal(() => close.current(), { tabs: true }), [])
  useEffect(() => {
    requestAnimationFrame(focusFirst)
  }, [key])
  useHints([
    { glyph: 'A', label: tr('Select') },
    { glyph: 'B', label: tr('Back') },
  ])

  const run = async (f: () => Promise<unknown>, okTitle?: string, okText?: string) => {
    setBusy(true)
    try {
      await f()
      if (okTitle) toast(okTitle, okText ?? item.name, 'ok')
    } catch (e) {
      toast(tr('Something went wrong'), (e as Error).message, 'error')
    } finally {
      setBusy(false)
    }
  }

  const hero = item.art.hero ?? (g?.hero ? imgUrl(g.hero) : null)
  const cover = item.art.cover ?? (g?.cover ? imgUrl(g.cover) : null)
  const candidates = status?.candidates ?? []
  const target = status?.state?.target ?? ''

  return (
    <div className="inst" data-nav-scope>
      {hero && <div className="inst-bg" style={{ backgroundImage: `url(${hero})` }} />}
      <div className="inst-shade" />
      <div className="inst-content">
        <aside className="inst-side">
          <div className="inst-cover">{cover ? <img src={cover} alt="" draggable={false} /> : <Icon name="pad" size={48} />}</div>
          {!item.missing && (
            <button
              data-nav
              data-nav-default={installing ? undefined : ''}
              className="btn primary big"
              disabled={busy || !key || installing}
              onClick={() => key && void run(() => api.installPlay(key))}
            >
              <Icon name="play" /> {item.running ? tr('Resume') : tr('Play')}
            </button>
          )}
          {g?.steam_appid && <ProtonBadge appid={Number(g.steam_appid)} />}
        </aside>
        <div className="inst-main">
          <span className="hero-kicker">
            {[item.origin === 'pishop' ? tr('Installed through piShop') : tr('Non-Steam shortcut'), g?.year, g?.developers?.[0]]
              .filter(Boolean)
              .join(' · ')}
          </span>
          <h1>{item.name}</h1>
          {error && <p className="inst-error">{error}</p>}

          {item.missing && key && (
            <section className="inst-card">
              <h3>{tr('Not in your Steam library anymore')}</h3>
              <p className="muted">
                {tr('Its shortcut was deleted in Steam. Add it back (same executable and Proton; its old prefix comes along when it’s still there), or take it off Games.')}
              </p>
              {item.exe && <p className="inst-path">{homePath(item.exe)}</p>}
              <div className="row">
                <button
                  data-nav
                  data-nav-default
                  className="btn primary"
                  disabled={busy}
                  onClick={() =>
                    void run(async () => {
                      await api.gameReadd(key)
                      onChanged()
                      onClose()
                    }, tr('Back in your Steam library'))
                  }
                >
                  <Icon name="plus" /> {tr('Add to Steam again')}
                </button>
                <button
                  data-nav
                  className="btn"
                  disabled={busy}
                  onClick={() =>
                    void run(async () => {
                      await api.gameForget(key)
                      onChanged()
                      onClose()
                    }, tr('Taken off Games'))
                  }
                >
                  {tr('Forget')}
                </button>
              </div>
            </section>
          )}

          {!item.missing && installing && key && (
            <section className="inst-card">
              {status?.running !== false ? (
                <>
                  <h3>
                    <Spinner /> {tr('The installer is running')}
                  </h3>
                  <p>{tr('Follow it on screen. When it closes, piShop comes back here to pick the game’s executable.')}</p>
                  {target && <p className="muted">{trb('Install folder: **{dir}**', { dir: homePath(target) })}</p>}
                </>
              ) : (
                <>
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
                        onClick={() => void run(() => api.installFinish(key, c.path).then(s => setStatus({ state: s })), tr('Installed'))}
                      >
                        <Icon name="file" />
                        <span className="inst-row-main">
                          <b>{c.path.split('/').pop()}</b>
                          <small>{homePath(c.path.replace(/\/[^/]+$/, ''))}</small>
                        </span>
                        <span className="inst-row-side">
                          {c.registered ? tr('Registered by the installer') : i === 0 ? tr('Best guess') : formatBytes(c.size)}
                        </span>
                      </button>
                    ))}
                    {!candidates.length && <p className="muted">{tr('No executable found where the installer usually puts games.')}</p>}
                  </div>
                  <div className="row">
                    <button data-nav className="btn" onClick={() => setPicker(true)}>
                      {tr('Browse…')}
                    </button>
                  </div>
                </>
              )}
            </section>
          )}

          {!item.missing && key && (
            <GameSetup key={`${key}:${status?.state?.exe ?? ''}`} gameKey={key} name={item.name} onChanged={onChanged} onExplore={onExplore} onRemoved={onClose} />
          )}
          {!key && !error && !item.missing && (
            <p className="muted">
              <Spinner /> {tr('Reading the game’s setup…')}
            </p>
          )}
        </div>
      </div>
      {picker && key && (
        <FilePicker
          title={tr('Pick the game’s executable')}
          accept={['.exe']}
          start={target || '/home'}
          onClose={() => setPicker(false)}
          onPick={path => {
            setPicker(false)
            void run(() => api.installFinish(key, path).then(s => setStatus({ state: s })), tr('Installed'))
          }}
        />
      )}
    </div>
  )
}
