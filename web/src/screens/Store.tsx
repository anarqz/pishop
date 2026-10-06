// Store: searches Prowlarr (consoles / PC games), shows releases as a cover
// grid matched against SteamGridDB, with a full details page and download.

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { type Art, type DiscoverGame, type Place, type Release, type ReleaseDetails, api, formatBytes, imgUrl } from '../api'
import { focusFirst, input } from '../input'
import { Dialog, type Hint, Icon, Spinner, TextPrompt, toast, useHints } from '../ui'

export type Kind = 'console' | 'pc'
type Sort = 'relevance' | 'seeders' | 'size' | 'recent'

const SORT_LABEL: Record<Sort, string> = {
  relevance: 'Mais relevantes',
  seeders: 'Mais seeders',
  size: 'Maior tamanho',
  recent: 'Mais recentes',
}
const SORT_ORDER: Sort[] = ['relevance', 'seeders', 'size', 'recent']

/** Token overlap (Dice) between a release's game name and the query. */
function relevance(r: Release, query: string) {
  const toks = (t: string) =>
    t
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, ' ')
      .split(' ')
      .filter(w => w && !['the', 'of', 'a', 'and'].includes(w))
  const a = toks(r.parsed.name)
  const b = toks(query)
  if (!a.length || !b.length) return 0
  const inter = a.filter(w => b.includes(w)).length
  return (2 * inter) / (a.length + b.length)
}
const SUGGESTIONS = ['Zelda', 'Mario', 'Pokémon', 'Metroid', 'Sonic', 'Final Fantasy', 'Resident Evil', 'God of War']
const HISTORY_KEY = 'pishop.store.history'

// ---------- remembered across tab switches ----------

const memory: {
  kind: Kind
  sort: Sort
  query: string
  results: Release[] | null
  platform: string | null
  origin: DiscoverGame | null
  ctx: SearchContext | null
  /** Last Discover request already searched (so returning doesn't redo it). */
  handled: number
} = { kind: 'console', sort: 'seeders', query: '', results: null, platform: null, origin: null, ctx: null, handled: 0 }

/** Header data for a search: SteamGridDB art + isitcracked status. */
interface SearchContext {
  q: string
  art: Art | null
  hero: string | null
  crack: DiscoverGame | null
}

function readHistory(): string[] {
  try {
    return JSON.parse(localStorage.getItem(HISTORY_KEY) ?? '[]')
  } catch {
    return []
  }
}
function pushHistory(q: string) {
  try {
    const h = [q, ...readHistory().filter(x => x.toLowerCase() !== q.toLowerCase())].slice(0, 8)
    localStorage.setItem(HISTORY_KEY, JSON.stringify(h))
  } catch {
    // storage unavailable
  }
}

function displayName(r: Release, art?: Art | null) {
  return art?.name ?? r.parsed.name
}

// ---------- screen ----------

export interface StoreRequest {
  q: string
  kind: Kind
  n: number
  /** Set when the search came from Discover: shown as a header. */
  game?: DiscoverGame
}

export default function Store({
  places,
  request,
  onGoSettings,
}: {
  places: Place[]
  request?: StoreRequest | null
  onGoSettings: () => void
}) {
  const [kind, setKind] = useState<Kind>(memory.kind)
  const [query, setQuery] = useState(memory.query)
  const [results, setResults] = useState<Release[] | null>(memory.results)
  const [platform, setPlatform] = useState<string | null>(memory.platform)
  const [sort, setSort] = useState<Sort>(memory.sort)
  const [origin, setOrigin] = useState<DiscoverGame | null>(memory.origin)
  const [ctx, setCtx] = useState<SearchContext | null>(memory.ctx)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [prompt, setPrompt] = useState(false)
  const [details, setDetails] = useState<string | null>(null)
  const [configured, setConfigured] = useState<boolean | null>(null)

  useEffect(() => {
    api
      .catalogConfig()
      .then(c => setConfigured(!!c.prowlarr_url && c.has_key))
      .catch(() => setConfigured(false))
  }, [])

  useEffect(() => {
    Object.assign(memory, { kind, sort, query, results, platform, origin, ctx })
  }, [kind, sort, query, results, platform, origin, ctx])

  const runSearch = useCallback(async (q: string, k: Kind, from?: DiscoverGame | null) => {
    if (!q.trim()) return
    setQuery(q)
    // Header in parallel with the torrent search; Discover already gave the crack.
    setCtx({ q, art: null, hero: null, crack: from ?? null })
    void api
      .context(q, !!from)
      .then(c => setCtx(cur => (cur?.q === q ? { q, art: c.art, hero: c.hero, crack: from ?? c.crack } : cur)))
      .catch(() => {})
    setLoading(true)
    setError(null)
    setPlatform(null)
    try {
      const r = await api.search(q, k)
      setResults(r)
      pushHistory(q)
      requestAnimationFrame(focusFirst)
    } catch (e) {
      setError((e as Error).message)
      setResults(null)
    } finally {
      setLoading(false)
    }
  }, [])

  // A game picked elsewhere (Discover) arrives as a search request.
  useEffect(() => {
    if (!request || request.n === memory.handled) return
    memory.handled = request.n
    setKind(request.kind)
    setDetails(null)
    setResults(null)
    setOrigin(request.game ?? null)
    setSort('relevance')
    void runSearch(request.q, request.kind, request.game)
  }, [request, runSearch])

  const platforms = useMemo(() => {
    const m = new Map<string, number>()
    for (const r of results ?? []) {
      const p = r.parsed.platform_label ?? 'Outros'
      m.set(p, (m.get(p) ?? 0) + 1)
    }
    return [...m.entries()].sort((a, b) => b[1] - a[1])
  }, [results])

  const shown = useMemo(() => {
    const list = (results ?? []).filter(r => !platform || (r.parsed.platform_label ?? 'Outros') === platform)
    const sorted = [...list]
    if (sort === 'relevance') {
      const score = new Map(sorted.map(r => [r.id, Math.round(relevance(r, query) * 10)]))
      sorted.sort((a, b) => score.get(b.id)! - score.get(a.id)! || b.seeders - a.seeders)
    } else if (sort === 'size') sorted.sort((a, b) => b.size - a.size)
    else if (sort === 'recent') sorted.sort((a, b) => b.publish_date.localeCompare(a.publish_date))
    else sorted.sort((a, b) => b.seeders - a.seeders || b.leechers - a.leechers)
    return sorted
  }, [results, platform, sort, query])

  const switchKind = () => {
    const next: Kind = kind === 'console' ? 'pc' : 'console'
    setKind(next)
    if (query) void runSearch(query, next)
    else setResults(null)
  }
  const cyclePlatform = (d: number) => {
    const opts: Array<string | null> = [null, ...platforms.map(([p]) => p)]
    const i = opts.indexOf(platform)
    setPlatform(opts[(i + d + opts.length) % opts.length])
  }

  const ctl = useRef({ switchKind, cyclePlatform, sort, setSort })
  ctl.current = { switchKind, cyclePlatform, sort, setSort }
  useEffect(() => {
    if (prompt || details) return
    return input.pushHandler(a => {
      const c = ctl.current
      switch (a) {
        case 'menu':
          setPrompt(true)
          return true
        case 'y':
          c.switchKind()
          return true
        case 'x':
          c.setSort(SORT_ORDER[(SORT_ORDER.indexOf(c.sort) + 1) % SORT_ORDER.length])
          return true
        case 'lt':
          c.cyclePlatform(-1)
          return true
        case 'rt':
          c.cyclePlatform(1)
          return true
        default:
          return false
      }
    })
  }, [prompt, details])

  const hints: Hint[] = [
    { glyph: 'A', label: results?.length ? 'Abrir torrent' : 'Selecionar' },
    { glyph: 'MENU', label: 'Buscar' },
    { glyph: 'Y', label: kind === 'console' ? 'Ver PC' : 'Ver consoles' },
    ...(results?.length ? [{ glyph: 'X' as const, label: 'Ordenar' }, { glyph: 'R2' as const, label: 'Plataforma' }] : []),
    { glyph: 'B', label: 'Voltar' },
  ]
  useHints(prompt || details ? null : hints)

  if (configured === false) {
    return (
      <div className="empty-state" data-nav-scope>
        <Icon name="search" size={56} />
        <h2>Conecte o Prowlarr</h2>
        <p>A loja busca jogos nos indexadores do seu Prowlarr. Configure o endereço e a chave de API.</p>
        <button data-nav data-nav-default className="btn primary" onClick={onGoSettings}>
          Configurar indexadores
        </button>
      </div>
    )
  }

  const history = readHistory()

  return (
    <div className="store" data-nav-scope>
      <div className="store-head">
        <button data-nav data-nav-default={!results?.length ? '' : undefined} className="searchbar" onClick={() => setPrompt(true)}>
          <Icon name="search" />
          <span className={`searchbar-text ${query ? '' : 'placeholder'}`}>{query || 'Buscar jogos…'}</span>
          {loading && (
            <span className="searchbar-busy">
              <Spinner /> Buscando…
            </span>
          )}
        </button>
        <div className="segmented">
          <button data-nav className={kind === 'console' ? 'on' : ''} onClick={() => kind !== 'console' && switchKind()}>
            Consoles
          </button>
          <button data-nav className={kind === 'pc' ? 'on' : ''} onClick={() => kind !== 'pc' && switchKind()}>
            PC
          </button>
        </div>
      </div>

      {results && results.length > 0 && (
        <div className="store-filters">
          <div className="chips">
            <button data-nav className={`chip ${platform === null ? 'on' : ''}`} onClick={() => setPlatform(null)}>
              Todas <small>{results.length}</small>
            </button>
            {platforms.map(([p, n]) => (
              <button key={p} data-nav className={`chip ${platform === p ? 'on' : ''}`} onClick={() => setPlatform(p)}>
                {p} <small>{n}</small>
              </button>
            ))}
          </div>
          <span className="sort-label">{SORT_LABEL[sort]}</span>
        </div>
      )}

      <div className="store-body">
        {ctx && query && <ContextHeader ctx={ctx} query={query} count={results?.length ?? null} loading={loading} />}
        {error && (
          <div className="store-msg error">
            <b>A busca falhou</b>
            <span>{error}</span>
          </div>
        )}
        {loading && !results && (
          <div className="store-msg">
            <Spinner /> Buscando nos indexadores…
          </div>
        )}
        {!loading && !error && !results && (
          <div className="store-start">
            <h2>{kind === 'console' ? 'Jogos de console' : 'Jogos de PC'}</h2>
            <p className="muted">Pesquise pelo nome. Os resultados vêm dos indexadores do seu Prowlarr.</p>
            {history.length > 0 && (
              <>
                <h3 className="section-title">Buscas recentes</h3>
                <div className="chips wrap">
                  {history.map(h => (
                    <button key={h} data-nav className="chip" onClick={() => runSearch(h, kind)}>
                      <Icon name="search" size={16} /> {h}
                    </button>
                  ))}
                </div>
              </>
            )}
            <h3 className="section-title">Sugestões</h3>
            <div className="chips wrap">
              {SUGGESTIONS.map(s => (
                <button key={s} data-nav className="chip" onClick={() => runSearch(s, kind)}>
                  {s}
                </button>
              ))}
            </div>
          </div>
        )}
        {results && results.length === 0 && !loading && (
          <div className="store-msg">Nada encontrado para “{query}”.</div>
        )}
        {shown.length > 0 && (
          <div className={`release-list ${loading ? 'dim' : ''}`}>
            <div className="release-cols">
              <span>Plataforma</span>
              <span>Torrent</span>
              <span>Tamanho</span>
              <span>Data</span>
              <span>Seeders</span>
              <span>Leechers</span>
            </div>
            {shown.map((r, i) => (
              <ReleaseRow key={r.id} r={r} first={i === 0} onOpen={() => setDetails(r.id)} />
            ))}
          </div>
        )}
      </div>

      {prompt && (
        <TextPrompt
          title={kind === 'console' ? 'Buscar jogos de console' : 'Buscar jogos de PC'}
          placeholder="Nome do jogo"
          initial={query}
          submitLabel="Buscar"
          validate={v => v.length > 1}
          onCancel={() => setPrompt(false)}
          onSubmit={v => {
            setPrompt(false)
            setOrigin(null)
            setSort('relevance')
            void runSearch(v, kind)
          }}
        />
      )}
      {details && (
        <Details
          id={details}
          siblings={results ?? []}
          places={places}
          onOpen={setDetails}
          onClose={() => setDetails(null)}
        />
      )}
    </div>
  )
}

// ---------- grid card ----------

function hue(s: string) {
  let h = 0
  for (const c of s) h = (h * 31 + c.charCodeAt(0)) % 360
  return h
}

function Cover({ r, art, big }: { r: Release; art: Art | null | undefined; big?: boolean }) {
  const [loaded, setLoaded] = useState(false)
  const src = art ? imgUrl(big ? art.cover : art.cover_thumb) : null
  const h = hue(r.parsed.name)
  return (
    <div className="cover" style={{ ['--h' as string]: h }}>
      {!src && (
        <div className="cover-placeholder">
          {art === undefined ? <Spinner /> : <Icon name="pad" size={big ? 64 : 40} />}
          <b>{r.parsed.name}</b>
        </div>
      )}
      {src && <img src={src} alt="" className={loaded ? 'in' : ''} onLoad={() => setLoaded(true)} draggable={false} />}
      {r.parsed.platform_label && <span className="plat">{r.parsed.platform_label}</span>}
    </div>
  )
}

function shortDate(iso: string) {
  if (!iso) return '—'
  const d = new Date(iso.length === 10 ? `${iso}T12:00:00` : iso)
  return d.toLocaleDateString('pt-BR', { day: '2-digit', month: '2-digit', year: 'numeric' })
}

function ReleaseRow({ r, first, onOpen }: { r: Release; first: boolean; onOpen: () => void }) {
  const extra = [r.parsed.region, r.parsed.version, r.parsed.group, r.indexer].filter(Boolean).join(' · ')
  return (
    <button data-nav data-nav-default={first ? '' : undefined} className="release-row" onClick={onOpen}>
      <span className={`plat-badge ${r.parsed.platform_label ? '' : 'none'}`}>{r.parsed.platform_label ?? '—'}</span>
      <span className="release-name">
        <b>{r.title}</b>
        <small>{extra}</small>
      </span>
      <span className="num">{formatBytes(r.size)}</span>
      <span className="num">{shortDate(r.publish_date)}</span>
      <span className="num seed">▲ {r.seeders}</span>
      <span className="num leech">▼ {r.leechers}</span>
    </button>
  )
}

function daysAgo(iso: string | null) {
  if (!iso) return ''
  const days = Math.round((Date.now() - new Date(`${iso}T12:00:00`).getTime()) / 86_400_000)
  return days <= 0 ? 'hoje' : days === 1 ? 'ontem' : `há ${days} dias`
}

/** Header for any search: the game (SteamGridDB) and its crack (isitcracked). */
function ContextHeader({ ctx, query, count, loading }: { ctx: SearchContext; query: string; count: number | null; loading: boolean }) {
  const g = ctx.crack
  const bg = ctx.hero ?? g?.header ?? null
  const cover = g?.cover ?? ctx.art?.cover_thumb ?? null
  const title = ctx.art?.name ?? g?.title ?? query
  return (
    <div className="origin">
      {bg && <div className="origin-bg" style={{ backgroundImage: `url(${imgUrl(bg)})` }} />}
      <div className="origin-shade" />
      {cover ? (
        <img className="origin-cover" src={imgUrl(cover)} alt="" draggable={false} />
      ) : (
        <div className="origin-cover placeholder">
          <Icon name="pad" size={40} />
        </div>
      )}
      <div className="origin-info">
        <span className="hero-kicker">
          {ctx.art ? `SteamGridDB${ctx.art.year ? ` · ${ctx.art.year}` : ''}` : 'Busca'}
          {g ? ' · isitcracked' : ''}
        </span>
        <h2>{title}</h2>
        {g ? (
          <div className="origin-facts">
            {g.crack_date && (
              <span>
                <small>Crackeado</small>
                <b className="ok">
                  {shortDate(g.crack_date)} · {daysAgo(g.crack_date)}
                </b>
              </span>
            )}
            {g.scene_group && (
              <span>
                <small>Grupo</small>
                <b>{g.scene_group}</b>
              </span>
            )}
            {g.drm && (
              <span>
                <small>DRM</small>
                <b>{g.drm}</b>
              </span>
            )}
            {g.release_date && (
              <span>
                <small>Lançamento</small>
                <b>{shortDate(g.release_date)}</b>
              </span>
            )}
          </div>
        ) : (
          <div className="origin-facts">
            <span>
              <small>isitcracked</small>
              <b className="muted">Sem registro de crack</b>
            </span>
          </div>
        )}
        <span className="origin-count">
          {loading ? 'Buscando torrents no Prowlarr…' : count === null ? '' : `${count} ${count === 1 ? 'torrent encontrado' : 'torrents encontrados'}`}
        </span>
      </div>
    </div>
  )
}

// ---------- details ----------

function Details({
  id,
  siblings,
  places,
  onOpen,
  onClose,
}: {
  id: string
  siblings: Release[]
  places: Place[]
  onOpen: (id: string) => void
  onClose: () => void
}) {
  const [d, setD] = useState<ReleaseDetails | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [files, setFiles] = useState<{ state: 'idle' | 'busy' | 'ok' | 'error'; list?: { name: string; length: number }[]; msg?: string }>({ state: 'idle' })
  const [choose, setChoose] = useState(false)
  const close = useRef(onClose)
  close.current = onClose

  useEffect(() => {
    setD(null)
    setError(null)
    setFiles({ state: 'idle' })
    api
      .release(id)
      .then(setD)
      .catch(e => setError((e as Error).message))
  }, [id])

  useEffect(() => {
    if (choose) return
    return input.pushModal(() => close.current())
  }, [choose])
  useEffect(() => {
    if (d) requestAnimationFrame(focusFirst)
  }, [d])
  useHints(choose ? null : [{ glyph: 'A', label: 'Selecionar' }, { glyph: 'B', label: 'Voltar' }])

  const loadFiles = async () => {
    setFiles({ state: 'busy' })
    try {
      const r = await api.releaseFiles(id)
      setFiles({ state: 'ok', list: r.files })
    } catch (e) {
      setFiles({ state: 'error', msg: (e as Error).message })
    }
  }

  const r = d?.release ?? siblings.find(s => s.id === id)
  const others = r ? siblings.filter(s => s.id !== id && s.parsed.name.toLowerCase() === r.parsed.name.toLowerCase()).slice(0, 8) : []
  const date = r?.publish_date ? new Date(r.publish_date).toLocaleDateString('pt-BR') : '—'
  const name = r ? displayName(r, d?.art) : ''

  return (
    <div className="details" data-nav-scope>
      <div
        className="details-bg"
        style={{ backgroundImage: d?.hero ? `url(${imgUrl(d.hero)})` : d?.art ? `url(${imgUrl(d.art.cover)})` : undefined }}
      />
      <div className="details-shade" />
      {!r && !error && (
        <div className="store-msg">
          <Spinner /> Carregando…
        </div>
      )}
      {error && (
        <div className="store-msg error">
          <b>Não foi possível abrir</b>
          <span>{error}</span>
        </div>
      )}
      {r && (
        <div className="details-content">
          <div className="details-left">
            <Cover r={r} art={d ? d.art : undefined} big />
          </div>
          <div className="details-main">
            <h1 className="details-title">{name}</h1>
            <div className="details-tags">
              {r.parsed.platform_label && <span className="tag strong">{r.parsed.platform_label}</span>}
              {d?.art?.year && <span className="tag">{d.art.year}</span>}
              {r.parsed.region && <span className="tag">{r.parsed.region}</span>}
              {r.parsed.version && <span className="tag">{r.parsed.version}</span>}
              {r.parsed.group && <span className="tag">{r.parsed.group}</span>}
              <span className="tag">{r.indexer}</span>
            </div>
            <div className="release-title">{r.title}</div>

            <div className="stats">
              <div className="stat seed">
                <b>{r.seeders}</b>
                <span>Seeders</span>
              </div>
              <div className="stat leech">
                <b>{r.leechers}</b>
                <span>Leechers</span>
              </div>
              <div className="stat">
                <b>{formatBytes(r.size)}</b>
                <span>Tamanho</span>
              </div>
              <div className="stat">
                <b>{r.files ?? (files.list ? files.list.length : '—')}</b>
                <span>Arquivos</span>
              </div>
              <div className="stat">
                <b>{date}</b>
                <span>Publicado</span>
              </div>
            </div>

            <div className="row details-actions">
              <button data-nav data-nav-default className="btn primary big" onClick={() => setChoose(true)}>
                <Icon name="download" /> Baixar
              </button>
              <button data-nav className="btn big" disabled={files.state === 'busy' || files.state === 'ok'} onClick={loadFiles}>
                {files.state === 'busy' ? 'Lendo do enxame…' : 'Ver arquivos'}
              </button>
              <button data-nav className="btn big" onClick={onClose}>
                Voltar
              </button>
            </div>

            <div className="details-sections">
              {(files.state === 'ok' || files.state === 'error') && (
                <section>
                  <h3 className="section-title">Arquivos</h3>
                  {files.state === 'error' && <p className="error">{files.msg}</p>}
                  <ul className="file-list">
                    {files.list?.map(f => (
                      <li key={f.name}>
                        <Icon name="file" size={18} />
                        <span>{f.name}</span>
                        <small>{formatBytes(f.length)}</small>
                      </li>
                    ))}
                  </ul>
                </section>
              )}
              <section>
                <h3 className="section-title">Descrição</h3>
                {d ? (
                  <p className="description">{d.description ?? 'O indexador não forneceu descrição para este lançamento.'}</p>
                ) : (
                  <p className="muted">
                    <Spinner /> Carregando…
                  </p>
                )}
              </section>
              {others.length > 0 && (
                <section>
                  <h3 className="section-title">Outras versões deste jogo</h3>
                  <div className="others">
                    {others.map(o => (
                      <button key={o.id} data-nav className="other" onClick={() => onOpen(o.id)}>
                        <b>{o.title}</b>
                        <span>
                          {o.parsed.platform_label ?? '—'} · ▲ {o.seeders} ▼ {o.leechers} · {formatBytes(o.size)}
                        </span>
                      </button>
                    ))}
                  </div>
                </section>
              )}
            </div>
          </div>
        </div>
      )}
      {choose && r && <DownloadDialog r={r} places={places} onClose={() => setChoose(false)} />}
    </div>
  )
}

function DownloadDialog({ r, places, onClose }: { r: Release; places: Place[]; onClose: () => void }) {
  const [busy, setBusy] = useState(false)
  const [romsDirs, setRomsDirs] = useState<string[] | null>(null)
  const romsRoot = places.find(p => p.icon === 'roms')?.path
  const sys = r.parsed.platform

  useEffect(() => {
    if (!romsRoot) return setRomsDirs([])
    api
      .listLocal(romsRoot)
      .then(x => setRomsDirs(x.entries.filter(e => e.dir).map(e => e.name)))
      .catch(() => setRomsDirs([]))
  }, [romsRoot])
  useEffect(() => {
    if (romsDirs) requestAnimationFrame(focusFirst)
  }, [romsDirs])

  const romsTarget = romsRoot && sys && romsDirs?.includes(sys) ? `${romsRoot}/${sys}` : null

  const go = async (dest?: string) => {
    setBusy(true)
    try {
      const res = await api.download(r.id, dest)
      toast('Download iniciado', `${res.name ?? r.title} · acompanhe em Torrents`, 'ok')
      onClose()
    } catch (e) {
      setBusy(false)
      toast('Não foi possível baixar', (e as Error).message, 'error')
    }
  }

  return (
    <Dialog title="Baixar" onClose={onClose} wide>
      <div className="copy-facts">
        <span>
          <b>{r.title}</b>
        </span>
      </div>
      <div className="copy-facts">
        <span>
          Tamanho: <b>{formatBytes(r.size)}</b>
        </span>
        <span>
          Seeders: <b>{r.seeders}</b>
        </span>
      </div>
      <div className="dest-options">
        {!romsDirs && (
          <div className="muted">
            <Spinner /> Procurando a pasta do emulador…
          </div>
        )}
        {romsDirs && romsTarget && (
          <button data-nav data-nav-default className="dest-option suggested" disabled={busy} onClick={() => go(romsTarget)}>
            <Icon name="roms" size={26} />
            <div>
              <b>Baixar direto em roms/{sys}</b>
              <small>Sugerido — os arquivos vão para a pasta do emulador</small>
            </div>
          </button>
        )}
        {romsDirs && (
          <button data-nav data-nav-default={romsTarget ? undefined : ''} className="dest-option" disabled={busy} onClick={() => go()}>
            <Icon name="download" size={26} />
            <div>
              <b>Baixar em Downloads</b>
              <small>~/Downloads/piShop/&lt;nome do torrent&gt;</small>
            </div>
          </button>
        )}
      </div>
      <div className="dialog-actions">
        <button data-nav className="btn" onClick={onClose}>
          Cancelar
        </button>
      </div>
    </Dialog>
  )
}
