// Game page between Discover and the Store, Xbox-style: details below, the
// trailer above. Up slides to the trailer (it starts playing), Down returns.

import { useEffect, useRef, useState } from 'react'
import { type DiscoverGame, type GameMeta, type Trailer, api, imgUrl } from '../api'
import { focusFirst, input } from '../input'
import { Icon, Spinner, useHints } from '../ui'

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

  const ctl = useRef({ view, setView, onClose })
  ctl.current = { view, setView, onClose }
  useEffect(
    () =>
      input.pushHandler(a => {
        const c = ctl.current
        if (c.view === 'trailer') {
          if (a === 'down' || a === 'back') c.setView('details')
          // Everything else is swallowed while the trailer plays.
          return a !== 'quit'
        }
        if (a === 'back') {
          c.onClose()
          return true
        }
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
      ? [{ glyph: 'DPAD', label: 'Detalhes ▼' }, { glyph: 'B', label: 'Voltar' }]
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
      <div className={`gd-track gd-show-${view}`}>
        {/* Trailer (above) */}
        <section className="gd-page gd-trailer">
          <div className="gd-trailer-head">
            <span className="hero-kicker">Trailer</span>
            <b>{trailer.t?.title ?? game.title}</b>
          </div>
          <div className="gd-video">
            {view === 'trailer' && trailer.state === 'ok' && (
              <iframe
                // Not focusable: the controller keeps driving the page.
                tabIndex={-1}
                src={`https://www.youtube-nocookie.com/embed/${trailer.t!.video_id}?autoplay=1&controls=0&rel=0&modestbranding=1&playsinline=1&iv_load_policy=3`}
                title={trailer.t!.title}
                allow="autoplay; encrypted-media"
                referrerPolicy="strict-origin-when-cross-origin"
              />
            )}
            {trailer.state === 'loading' && (
              <div className="gd-video-msg">
                <Spinner /> Procurando o trailer…
              </div>
            )}
            {(trailer.state === 'none' || trailer.state === 'error') && (
              <div className="gd-video-msg">Nenhum trailer encontrado para este jogo.</div>
            )}
          </div>
          <div className="gd-chevron">▼ Detalhes</div>
        </section>

        {/* Details (below) */}
        <section className="gd-page gd-details">
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
      </div>
    </div>
  )
}
