// Discover: a Big Picture / Netflix style front page of the latest cracked PC
// games (isitcracked): a rotating featured hero, horizontal rows and the full
// paginated catalog. Picking a game opens its page (details + trailer).

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { type DiscoverGame, type GameMeta, api, imgUrl } from '../api'
import { focusFirst, input } from '../input'
import { Icon, Spinner, TextPrompt, useHints } from '../ui'
import GameDetail, { daysAgo } from './GameDetail'

const PAGE = 30
/** First load: enough games to fill the themed rows. */
const INITIAL = 90
const FEATURED = 5
const ROTATE_MS = 9000

const memory: { search: string; items: DiscoverGame[]; total: number; featured: number; open: DiscoverGame | null } = {
  search: '',
  items: [],
  total: 0,
  featured: 0,
  open: null,
}

// SteamGridDB art for the featured hero, fetched once per title.
const metaCache = new Map<string, Promise<GameMeta | null>>()
function heroMeta(title: string) {
  let p = metaCache.get(title)
  if (!p) {
    p = api.gameInfo(title, false).catch(() => null)
    metaCache.set(title, p)
  }
  return p
}

interface Row {
  title: string
  games: DiscoverGame[]
}

function buildRows(items: DiscoverGame[]): Row[] {
  const rows: Row[] = [{ title: 'Cracks recentes', games: items.slice(0, 20) }]
  const recent = items
    .filter(g => g.release_date)
    .sort((a, b) => (b.release_date ?? '').localeCompare(a.release_date ?? ''))
    .slice(0, 20)
  if (recent.length > 4) rows.push({ title: 'Lançamentos recentes', games: recent })
  const byGroup = new Map<string, DiscoverGame[]>()
  for (const g of items) {
    if (!g.scene_group) continue
    byGroup.set(g.scene_group, [...(byGroup.get(g.scene_group) ?? []), g])
  }
  for (const [group, games] of [...byGroup.entries()].sort((a, b) => b[1].length - a[1].length).slice(0, 3)) {
    if (games.length > 3) rows.push({ title: `Crackeados por ${group}`, games: games.slice(0, 20) })
  }
  return rows
}

export default function Discover({ onPick }: { onPick: (game: DiscoverGame) => void }) {
  const [search, setSearch] = useState(memory.search)
  const [items, setItems] = useState<DiscoverGame[]>(memory.items)
  const [total, setTotal] = useState(memory.total)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [prompt, setPrompt] = useState(false)
  const [featured, setFeatured] = useState(memory.featured)
  const [open, setOpen] = useState<DiscoverGame | null>(memory.open)
  const loadingRef = useRef(false)

  useEffect(() => {
    Object.assign(memory, { search, items, total, featured, open })
  }, [search, items, total, featured, open])

  const load = useCallback(async (offset: number, q: string, limit = PAGE) => {
    if (loadingRef.current) return
    loadingRef.current = true
    setLoading(true)
    setError(null)
    try {
      const page = await api.discover(offset, q, limit)
      setTotal(page.total)
      setItems(prev => {
        const base = offset === 0 ? [] : prev
        const seen = new Set(base.map(g => g.id))
        return [...base, ...page.items.filter(g => !seen.has(g.id))]
      })
      if (offset === 0) requestAnimationFrame(focusFirst)
    } catch (e) {
      setError((e as Error).message)
    } finally {
      loadingRef.current = false
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    if (!memory.items.length) void load(0, memory.search, memory.search ? PAGE : INITIAL)
  }, [load])

  const hasMore = items.length < total
  const more = useRef({ hasMore, items, search, load })
  more.current = { hasMore, items, search, load }
  const loadMore = useCallback(() => {
    const m = more.current
    if (m.hasMore) void m.load(m.items.length, m.search)
  }, [])

  const scroller = useRef<HTMLDivElement>(null)
  const sentinel = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const el = sentinel.current
    if (!el || !scroller.current) return
    const io = new IntersectionObserver(es => es.some(e => e.isIntersecting) && loadMore(), {
      root: scroller.current,
      rootMargin: '0px 0px 900px 0px',
    })
    io.observe(el)
    return () => io.disconnect()
  }, [items.length, loadMore])

  const applySearch = (q: string) => {
    setSearch(q)
    setItems([])
    void load(0, q, q ? PAGE : INITIAL)
  }

  // Featured hero: rotates by itself; L2/R2 flips through it.
  const featuredGames = useMemo(() => items.slice(0, FEATURED), [items])
  const hero = featuredGames[featured % Math.max(1, featuredGames.length)]
  const [heroMetaState, setHeroMeta] = useState<GameMeta | null>(null)
  useEffect(() => {
    if (!hero) return
    let alive = true
    setHeroMeta(null)
    void heroMeta(hero.title).then(m => alive && setHeroMeta(m))
    // Warm the next ones so rotation shows art immediately.
    featuredGames.forEach(g => void heroMeta(g.title))
    return () => {
      alive = false
    }
  }, [hero?.id])
  useEffect(() => {
    if (search || open || featuredGames.length < 2) return
    const id = setInterval(() => setFeatured(f => (f + 1) % featuredGames.length), ROTATE_MS)
    return () => clearInterval(id)
  }, [search, open, featuredGames.length, featured])

  const ctl = useRef({ search, applySearch, n: featuredGames.length })
  ctl.current = { search, applySearch, n: featuredGames.length }
  useEffect(() => {
    if (prompt || open) return
    return input.pushHandler(a => {
      const c = ctl.current
      if (a === 'menu') {
        setPrompt(true)
        return true
      }
      if ((a === 'lt' || a === 'rt') && !c.search && c.n > 1) {
        setFeatured(f => (f + (a === 'rt' ? 1 : -1) + c.n) % c.n)
        return true
      }
      if (a === 'back' && c.search) {
        c.applySearch('')
        return true
      }
      return false
    })
  }, [prompt, open])

  useHints(
    prompt || open
      ? null
      : [
          { glyph: 'A', label: 'Ver jogo' },
          { glyph: 'MENU', label: 'Pesquisar' },
          ...(!search ? [{ glyph: 'R2' as const, label: 'Destaques' }] : []),
          ...(search ? [{ glyph: 'B' as const, label: 'Limpar busca' }] : [{ glyph: 'L1' as const, label: 'Abas' }]),
        ],
  )

  const rows = useMemo(() => (search ? [] : buildRows(items)), [items, search])

  return (
    <div className="dsc" data-nav-scope>
      <div className="dsc-scroll" ref={scroller}>
        <div className="dsc-top">
          <button data-nav className="searchbar slim" onClick={() => setPrompt(true)}>
            <Icon name="search" />
            <span className={`searchbar-text ${search ? '' : 'placeholder'}`}>{search || 'Pesquisar jogos crackeados…'}</span>
            {loading && (
              <span className="searchbar-busy">
                <Spinner /> Carregando…
              </span>
            )}
          </button>
        </div>

        {error && (
          <div className="store-msg error">
            <b>Não foi possível carregar</b>
            <span>{error}</span>
            <button data-nav className="btn" onClick={() => load(items.length, search)}>
              Tentar de novo
            </button>
          </div>
        )}

        {!search && hero && (
          <section className="dsc-hero">
            <div
              key={hero.id}
              className="dsc-hero-bg"
              style={{
                backgroundImage: heroMetaState?.hero
                  ? `url(${imgUrl(heroMetaState.hero)})`
                  : hero.header
                    ? `url(${imgUrl(hero.header)})`
                    : undefined,
              }}
            />
            <div className="dsc-hero-shade" />
            <div className="dsc-hero-content" key={`c${hero.id}`}>
              <span className="hero-kicker">Em destaque · crackeado {daysAgo(hero.crack_date)}</span>
              <h1>{heroMetaState?.art?.name ?? hero.title}</h1>
              <div className="dsc-hero-meta">
                {[hero.scene_group, hero.drm && `DRM ${hero.drm}`, hero.release_date?.slice(0, 4)].filter(Boolean).join('  ·  ')}
              </div>
              <div className="row">
                <button data-nav data-nav-default className="btn primary big" onClick={() => setOpen(hero)}>
                  Ver jogo
                </button>
                <button data-nav className="btn big" onClick={() => onPick(hero)}>
                  <Icon name="search" /> Buscar torrents
                </button>
              </div>
            </div>
            <div className="dsc-dots">
              {featuredGames.map((g, i) => (
                <span key={g.id} className={i === featured % featuredGames.length ? 'on' : ''} />
              ))}
            </div>
          </section>
        )}

        {rows.map(row => (
          <section key={row.title} className="dsc-row">
            <h3 className="dsc-row-title">{row.title}</h3>
            <div className="dsc-strip">
              {row.games.map(g => (
                <PosterCard key={g.id} g={g} onOpen={() => setOpen(g)} />
              ))}
            </div>
          </section>
        ))}

        <section className="dsc-catalog">
          <div className="dsc-catalog-head">
            <h3 className="dsc-row-title">{search ? `Resultados para “${search}”` : 'Catálogo completo'}</h3>
            <span className="sort-label">{total ? `${Math.min(items.length, total)} de ${total} jogos` : ''}</span>
          </div>
          {!error && !loading && items.length === 0 && <div className="store-msg">Nenhum jogo encontrado.</div>}
          {items.length === 0 && loading && (
            <div className="store-msg">
              <Spinner /> Carregando…
            </div>
          )}
          <div className="grid">
            {items.map((g, i) => (
              <PosterCard
                key={g.id}
                g={g}
                first={!!search && i === 0}
                onOpen={() => setOpen(g)}
                // Two rows before the end: fetch the next page already.
                onFocus={() => i >= items.length - 14 && loadMore()}
                withMeta
              />
            ))}
          </div>
          <div ref={sentinel} className="sentinel">
            {loading && items.length > 0 && (
              <>
                <Spinner /> Carregando mais…
              </>
            )}
            {!hasMore && items.length > 0 && <span className="muted">Fim da lista</span>}
          </div>
        </section>
      </div>

      {prompt && (
        <TextPrompt
          title="Pesquisar jogos crackeados"
          placeholder="Nome do jogo"
          initial={search}
          submitLabel="Pesquisar"
          onCancel={() => setPrompt(false)}
          onSubmit={v => {
            setPrompt(false)
            applySearch(v)
          }}
        />
      )}
      {open && (
        <GameDetail
          game={open}
          onSearch={() => {
            const g = open
            setOpen(null)
            onPick(g)
          }}
          onClose={() => setOpen(null)}
        />
      )}
    </div>
  )
}

function PosterCard({
  g,
  first,
  onOpen,
  onFocus,
  withMeta,
}: {
  g: DiscoverGame
  first?: boolean
  onOpen: () => void
  onFocus?: () => void
  withMeta?: boolean
}) {
  const [loaded, setLoaded] = useState(false)
  const [failed, setFailed] = useState(false)
  let h = 0
  for (const c of g.title) h = (h * 31 + c.charCodeAt(0)) % 360
  return (
    <button data-nav data-nav-default={first ? '' : undefined} className="card poster" onClick={onOpen} onFocus={onFocus}>
      <div className="cover" style={{ ['--h' as string]: h }}>
        {(!g.cover || failed) && (
          <div className="cover-placeholder">
            <Icon name="pad" size={36} />
            <b>{g.title}</b>
          </div>
        )}
        {g.cover && !failed && (
          <img
            src={imgUrl(g.cover)}
            alt=""
            loading="lazy"
            className={loaded ? 'in' : ''}
            onLoad={() => setLoaded(true)}
            onError={() => setFailed(true)}
            draggable={false}
          />
        )}
        <span className="poster-age">{daysAgo(g.crack_date)}</span>
      </div>
      <div className="card-info">
        <b className="card-title">{g.title}</b>
        {withMeta && (
          <div className="card-meta">
            <span className="seed">{g.scene_group ?? 'Crack'}</span>
            {g.drm && <span className="size">{g.drm}</span>}
          </div>
        )}
      </div>
    </button>
  )
}
