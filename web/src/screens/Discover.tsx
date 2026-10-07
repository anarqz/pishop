// Discover: a Big Picture / Netflix style front page of the latest cracked PC
// games (isitcracked): a featured carousel of the 5 newest (L2/R2), a centered
// search, and one "Cracks recentes" grid that keeps loading pages as you go.
// Picking a game opens its page (details + trailer).

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { type DiscoverGame, type GameMeta, api, imgUrl } from '../api'
import { tr, trn } from '../i18n'
import { focusFirst, input, keepFocus } from '../input'
import { Glyph, Icon, Spinner, TextPrompt, useHints } from '../ui'
import GameDetail, { daysAgo } from './GameDetail'

const PAGE = 30
const FEATURED = 5
/** Must match `.grid` in styles.css. */
const GRID_COLUMNS = 7
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

export default function Discover({
  onPick,
  onGoStore,
  onGoSettings,
}: {
  onPick: (game: DiscoverGame) => void
  onGoStore: () => void
  onGoSettings: () => void
}) {
  // isitcracked feeds this page; without it, point to the Store and Settings.
  const [iic, setIic] = useState<boolean | null>(null)
  useEffect(() => {
    api
      .catalogConfig()
      .then(c => setIic(!!c.iic_url && c.has_iic_key))
      .catch(() => setIic(true))
  }, [])
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
    if (iic && !memory.items.length) void load(0, memory.search)
  }, [load, iic])

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
    void load(0, q)
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

  const restoreFocus = useRef<(() => void) | null>(null)
  const openGame = (g: DiscoverGame) => {
    restoreFocus.current = keepFocus()
    setOpen(g)
  }
  const closeGame = () => {
    setOpen(null)
    ;(restoreFocus.current ?? (() => requestAnimationFrame(focusFirst)))()
    restoreFocus.current = null
  }

  const flip = (d: number) => setFeatured(f => (f + d + featuredGames.length) % Math.max(1, featuredGames.length))

  const ctl = useRef({ search, applySearch, n: featuredGames.length })
  ctl.current = { search, applySearch, n: featuredGames.length }
  useEffect(() => {
    if (prompt || open || !iic) return
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
  }, [prompt, open, iic])

  useHints(
    prompt || open
      ? null
      : iic === false
        ? [{ glyph: 'A', label: tr('Select') }, { glyph: 'L1', label: tr('Tabs') }]
        : [
          { glyph: 'A', label: tr('View game') },
          { glyph: 'MENU', label: tr('Search') },
          ...(!search ? [{ glyph: ['L2', 'R2'] as ['L2', 'R2'], label: tr('Spotlight') }] : []),
          ...(search ? [{ glyph: 'B' as const, label: tr('Clear search') }] : [{ glyph: 'L1' as const, label: tr('Tabs') }]),
        ],
  )

  if (iic === false) {
    return (
      <div className="empty-state" data-nav-scope>
        <Icon name="search" size={56} />
        <h2>{tr('See what’s been cracked lately')}</h2>
        <p>{tr('Discover lists recently cracked games from isitcracked.com. Add its address and key in Settings → Services to turn it on.')}</p>
        <p className="muted">{tr('Meanwhile, the Store finds any game for you.')}</p>
        <div className="row">
          <button data-nav data-nav-default className="btn primary" onClick={onGoStore}>
            <Icon name="search" /> {tr('Search the Store')}
          </button>
          <button data-nav className="btn" onClick={onGoSettings}>
            {tr('Set up isitcracked')}
          </button>
        </div>
      </div>
    )
  }

  return (
    <div className="dsc" data-nav-scope>
      <div className="dsc-scroll" ref={scroller}>
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
              <span className="hero-kicker">{tr('Featured · cracked {when}', { when: daysAgo(hero.crack_date) })}</span>
              <h1>{heroMetaState?.art?.name ?? hero.title}</h1>
              <div className="dsc-hero-meta">
                {[hero.scene_group, hero.drm && `DRM ${hero.drm}`, hero.release_date?.slice(0, 4)].filter(Boolean).join('  ·  ')}
              </div>
              <div className="row">
                <button
                  data-nav
                  data-nav-default
                  data-nav-id="hero-main"
                  data-nav-down='[data-nav-id="dsc-search"]'
                  className="btn primary big"
                  onClick={() => openGame(hero)}
                >
                  {tr('View game')}
                </button>
                <button data-nav data-nav-down='[data-nav-id="dsc-search"]' className="btn big" onClick={() => onPick(hero)}>
                  <Icon name="search" /> {tr('Search torrents')}
                </button>
              </div>
            </div>
            {featuredGames.length > 1 && (
              <div className="dsc-carousel">
                <button className="dsc-flip" tabIndex={-1} onClick={() => flip(-1)} aria-label={tr('Previous featured game')}>
                  <Glyph name="L2" /> ‹
                </button>
                <div className="dsc-dots">
                  {featuredGames.map((g, i) => (
                    <span key={g.id} className={i === featured % featuredGames.length ? 'on' : ''} />
                  ))}
                </div>
                <button className="dsc-flip" tabIndex={-1} onClick={() => flip(1)} aria-label={tr('Next featured game')}>
                  › <Glyph name="R2" />
                </button>
              </div>
            )}
          </section>
        )}

        <div className="dsc-search">
          <button
            data-nav
            data-nav-id="dsc-search"
            data-nav-up='[data-nav-id="hero-main"]'
            data-nav-down=".dsc-catalog .grid .card"
            data-nav-left="none"
            data-nav-right="none"
            className="searchbar center"
            onClick={() => setPrompt(true)}
          >
            <Icon name="search" />
            <span className={`searchbar-text ${search ? '' : 'placeholder'}`}>{search || tr('Search cracked games…')}</span>
            {loading && (
              <span className="searchbar-busy">
                <Spinner /> {tr('Loading…')}
              </span>
            )}
          </button>
        </div>

        {error && (
          <div className="store-msg error">
            <b>{tr("Couldn't load")}</b>
            <span>{error}</span>
            <div className="row">
              <button data-nav className="btn" onClick={() => load(items.length, search)}>
                {tr('Try again')}
              </button>
              <button data-nav className="btn" onClick={onGoStore}>
                <Icon name="search" /> {tr('Search the Store')}
              </button>
            </div>
          </div>
        )}

        <section className="dsc-catalog">
          <div className="dsc-catalog-head">
            <h3 className="dsc-row-title">{search ? tr('Results for “{q}”', { q: search }) : tr('Recent cracks')}</h3>
            <span className="sort-label">{total ? trn(total, '{shown} of {n} game', '{shown} of {n} games', { shown: Math.min(items.length, total) }) : ''}</span>
          </div>
          {!error && !loading && items.length === 0 && <div className="store-msg">{tr('No games found.')}</div>}
          {items.length === 0 && loading && (
            <div className="store-msg">
              <Spinner /> {tr('Loading…')}
            </div>
          )}
          <div className="grid">
            {items.map((g, i) => (
              <PosterCard
                key={g.id}
                g={g}
                first={!!search && i === 0}
                // First row (7 columns): Up goes to the search bar.
                upToSearch={i < GRID_COLUMNS}
                onOpen={() => openGame(g)}
                // Two rows before the end: fetch the next page already.
                onFocus={() => i >= items.length - 14 && loadMore()}
                withMeta
              />
            ))}
          </div>
          <div ref={sentinel} className="sentinel">
            {loading && items.length > 0 && (
              <>
                <Spinner /> {tr('Loading more…')}
              </>
            )}
            {!hasMore && items.length > 0 && <span className="muted">{tr("You've seen all {total} games", { total })}</span>}
          </div>
        </section>
      </div>

      {prompt && (
        <TextPrompt
          title={tr('Search cracked games')}
          placeholder={tr('Game name')}
          initial={search}
          submitLabel={tr('Search')}
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
          onClose={closeGame}
        />
      )}
    </div>
  )
}

function PosterCard({
  g,
  first,
  upToSearch,
  onOpen,
  onFocus,
  withMeta,
}: {
  g: DiscoverGame
  first?: boolean
  upToSearch?: boolean
  onOpen: () => void
  onFocus?: () => void
  withMeta?: boolean
}) {
  const [loaded, setLoaded] = useState(false)
  const [failed, setFailed] = useState(false)
  let h = 0
  for (const c of g.title) h = (h * 31 + c.charCodeAt(0)) % 360
  return (
    <button
      data-nav
      data-nav-default={first ? '' : undefined}
      data-nav-up={upToSearch ? '[data-nav-id="dsc-search"]' : undefined}
      className="card poster"
      onClick={onOpen}
      onFocus={onFocus}
    >
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
