// Game page between Discover and the Store, Xbox-style: details, with the
// trailer "above" — Up slides a full-screen player down over everything (it
// starts playing), Down/B slides it back up.

import { useEffect, useRef, useState } from 'react'
import { type DiscoverGame, type GameMeta, type Trailer, api, imgUrl } from '../api'
import { focusFirst, input } from '../input'
import { Glyph, Icon, Spinner, useHints } from '../ui'

function fmtDate(iso: string | null | undefined) {
  if (!iso) return null
  const d = new Date(iso.length === 10 ? `${iso}T12:00:00` : iso)
  return isNaN(d.getTime()) ? null : d.toLocaleDateString('pt-BR', { day: '2-digit', month: 'short', year: 'numeric' })
}

export function daysAgo(iso: string | null) {
  if (!iso) return ''
  const days = Math.round((Date.now() - new Date(`${iso}T12:00:00`).getTime()) / 86_400_000)
  return days <= 0 ? 'hoje' : days === 1 ? 'ontem' : `há ${days} dias`
}

export default function GameDetail({
  game,
  onSearch,
  onClose,
}: {
  game: DiscoverGame
  onSearch: () => void
  onClose: () => void
}) {
  const [meta, setMeta] = useState<GameMeta | null>(null)
  const [trailer, setTrailer] = useState<{ state: 'loading' | 'ok' | 'none' | 'error'; t?: Trailer; msg?: string }>({ state: 'loading' })
  const [view, setView] = useState<'details' | 'trailer'>('details')

  useEffect(() => {
    let alive = true
    api.gameInfo(game.title, true, game.steam_appid).then(m => alive && setMeta(m)).catch(() => alive && setMeta({ art: null, hero: null, tgdb: null }))
    api
      .trailer(game.title)
      .then(t => alive && setTrailer(t ? { state: 'ok', t } : { state: 'none' }))
      .catch(e => alive && setTrailer({ state: 'error', msg: (e as Error).message }))
    return () => {
      alive = false
    }
  }, [game.title])

  // Full-screen player: muted state and the auto-hiding control bar.
  const [muted, setMuted] = useState(false)
  const [barVisible, setBarVisible] = useState(true)
  const barTimer = useRef(0)
  const frame = useRef<HTMLIFrameElement>(null)
  const pokeBar = () => {
    setBarVisible(true)
    clearTimeout(barTimer.current)
    barTimer.current = window.setTimeout(() => setBarVisible(false), 3000)
  }
  // YouTube's iframe API (enablejsapi=1) takes commands over postMessage.
  const toggleMute = () => {
    setMuted(m => {
      frame.current?.contentWindow?.postMessage(JSON.stringify({ event: 'command', func: m ? 'unMute' : 'mute', args: [] }), '*')
      return !m
    })
    pokeBar()
  }
  useEffect(() => {
    if (view === 'trailer') {
      setMuted(false)
      pokeBar()
    }
    return () => clearTimeout(barTimer.current)
  }, [view])

  const ctl = useRef({ view, setView, onClose, toggleMute, pokeBar })
  ctl.current = { view, setView, onClose, toggleMute, pokeBar }
  useEffect(
    () =>
      input.pushHandler(a => {
        const c = ctl.current
        if (c.view === 'trailer') {
          if (a === 'down' || a === 'back') c.setView('details')
          else if (a === 'y') c.toggleMute()
          else c.pokeBar()
          // Everything else is swallowed while the trailer plays.
          return a !== 'quit'
        }
        if (a === 'back') {
          c.onClose()
          return true
        }
        // L1/R1 still switch tabs from the game page.
        if (a === 'lb' || a === 'rb') return false
        if (a === 'up') {
          // From the top row of the details, Up slides to the trailer.
          const el = document.activeElement as HTMLElement | null
          if (el?.closest('.gd-actions')) {
            c.setView('trailer')
            return true
          }
        }
        return 'nav'
      }),
    [],
  )
  useEffect(() => {
    if (view === 'details') requestAnimationFrame(focusFirst)
  }, [view])
  useHints(
    view === 'trailer'
      ? [{ glyph: 'B', label: 'Voltar' }, { glyph: 'Y', label: muted ? 'Ativar som' : 'Silenciar' }]
      : [
          { glyph: 'A', label: 'Selecionar' },
          { glyph: 'DPAD', label: 'Trailer ▲' },
          { glyph: 'B', label: 'Voltar' },
        ],
  )

  // Steam store data first (when the game has an appid), TheGamesDB otherwise.
  const st = meta?.steam ?? null
  const t = meta?.tgdb
  const appid = st?.appid ?? game.steam_appid
  const overview = st?.overview ?? t?.overview ?? null
  const developers = st?.developers.length ? st.developers : (t?.developers ?? [])
  const publishers = st?.publishers.length ? st.publishers : (t?.publishers ?? [])
  const genres = st?.genres.length ? st.genres : (t?.genres ?? [])
  const heroUrl = meta?.hero ?? st?.screenshot ?? st?.background ?? game.header
  const bg = heroUrl ? imgUrl(heroUrl) : null
  const cover = game.cover ? imgUrl(game.cover) : meta?.art ? imgUrl(meta.art.cover) : null
  const year = (t?.release_date ?? game.release_date)?.slice(0, 4)
  const byline = [year, developers[0], genres.slice(0, 3).join(' · ')].filter(Boolean).join('  ·  ')
  const facts: Array<[string, string | null | undefined]> = [
    ['Desenvolvedora', developers.join(', ')],
    ['Publicadora', publishers.join(', ')],
    ['Lançamento', fmtDate(game.release_date ?? t?.release_date) ?? st?.release_date],
    ['Gêneros', genres.join(', ')],
    ['Jogadores', t?.players ? String(t.players) : null],
    ['Classificação', t?.rating],
  ]
  const source = st?.overview ? 'Steam' : t ? 'TheGamesDB' : null

  return (
    <div className="gd" data-nav-scope>
      <div className="gd-bg" style={bg ? { backgroundImage: `url(${bg})` } : undefined} />
        <section className="gd-details">
          <button className="gd-chevron up" tabIndex={-1} onClick={() => setView('trailer')}>
            ▲ Trailer
          </button>
          <div className="gd-body">
            <div className="gd-cover">{cover ? <img src={cover} alt="" draggable={false} /> : <Icon name="pad" size={64} />}</div>
            <div className="gd-main">
              <span className="hero-kicker">Crackeado {daysAgo(game.crack_date)}</span>
              <h1>{meta?.art?.name ?? game.title}</h1>
              {byline && <div className="gd-byline">{byline}</div>}
              <div className="gd-chips">
                {game.crack_date && (
                  <span className="gd-chip ok">
                    <small>Crack</small>
                    {fmtDate(game.crack_date)}
                  </span>
                )}
                {game.scene_group && (
                  <span className="gd-chip">
                    <small>Grupo</small>
                    {game.scene_group}
                  </span>
                )}
                {game.drm && (
                  <span className="gd-chip">
                    <small>DRM</small>
                    {game.drm}
                  </span>
                )}
              </div>
              <div className="row gd-actions">
                <button data-nav data-nav-default className="btn primary big" onClick={onSearch}>
                  <Icon name="search" /> Buscar torrents
                </button>
                <button data-nav className="btn big" onClick={() => setView('trailer')}>
                  ▲ Assistir trailer
                </button>
                {appid && (
                  <button data-nav className="btn big" onClick={() => void api.openSteamStore(appid).catch(() => {})}>
                    Abrir na Steam
                  </button>
                )}
                <button data-nav className="btn big" onClick={onClose}>
                  Voltar
                </button>
              </div>
              <div className="gd-info">
                <p className="gd-overview">
                  {meta === null ? (
                    <>
                      <Spinner /> Carregando detalhes…
                    </>
                  ) : (
                    (overview ?? 'Sem sinopse disponível para este jogo.')
                  )}
                </p>
                <dl className="gd-facts">
                  {source && (
                    <div>
                      <dt>Fonte</dt>
                      <dd className="muted">{source}</dd>
                    </div>
                  )}
                  {facts
                    .filter(([, v]) => v)
                    .map(([k, v]) => (
                      <div key={k}>
                        <dt>{k}</dt>
                        <dd>{v}</dd>
                      </div>
                    ))}
                </dl>
              </div>
            </div>
          </div>
        </section>

      {/* Trailer: full screen, slides down from the top over everything. */}
      <div className={`gd-player ${view === 'trailer' ? 'open' : ''}`} aria-hidden={view !== 'trailer'}>
        {view === 'trailer' && trailer.state === 'ok' && (
          <iframe
            ref={frame}
            // Not focusable: the controller keeps driving the page.
            tabIndex={-1}
            src={`https://www.youtube-nocookie.com/embed/${trailer.t!.video_id}?autoplay=1&controls=0&rel=0&modestbranding=1&playsinline=1&iv_load_policy=3&disablekb=1&fs=0&enablejsapi=1&origin=${encodeURIComponent(window.location.origin)}`}
            title={trailer.t!.title}
            allow="autoplay; encrypted-media"
            referrerPolicy="strict-origin-when-cross-origin"
            // Belt and braces for autoplay: ask the player to start once loaded.
            onLoad={e => {
              const w = e.currentTarget.contentWindow
              window.setTimeout(() => w?.postMessage(JSON.stringify({ event: 'command', func: 'playVideo', args: [] }), '*'), 700)
            }}
          />
        )}
        {trailer.state === 'loading' && (
          <div className="gd-player-msg">
            <Spinner /> Procurando o trailer…
          </div>
        )}
        {(trailer.state === 'none' || trailer.state === 'error') && (
          <div className="gd-player-msg">Nenhum trailer encontrado para este jogo.</div>
        )}
        <div className={`gd-player-bar ${barVisible ? '' : 'hidden'}`}>
          <div className="gd-player-title">
            <span className="hero-kicker">Trailer · {meta?.art?.name ?? game.title}</span>
            <b>{trailer.t?.title ?? ''}</b>
          </div>
          <button className="gd-player-ctl" tabIndex={-1} onClick={() => setView('details')}>
            <Glyph name="B" /> Voltar
          </button>
          <button className="gd-player-ctl" tabIndex={-1} onClick={toggleMute}>
            <Glyph name="Y" /> {muted ? 'Ativar som' : 'Silenciar'}
          </button>
        </div>
      </div>
    </div>
  )
}
