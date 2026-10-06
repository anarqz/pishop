import { type Job, type Place, type Source, formatBytes, formatEta } from '../api'
import { Icon, Progress, useHints } from '../ui'

export type Tab = 'discover' | 'home' | 'store' | 'explorer' | 'jobs' | 'settings'

export default function Home({
  sources,
  places,
  jobs,
  go,
}: {
  sources: Source[]
  places: Place[]
  jobs: Job[]
  go: (t: Tab) => void
}) {
  useHints([
    { glyph: 'A', label: 'Abrir' },
    { glyph: 'L1', label: 'Abas' },
  ])
  const current = jobs.find(j => j.status === 'running' || j.status === 'scanning')
  const queued = jobs.filter(j => j.status === 'queued').length
  const storage = places.filter(p => p.icon === 'roms' || p.icon === 'sd' || p.icon === 'home')

  return (
    <div className="home" data-nav-scope>
      <button data-nav data-nav-default className="hero" onClick={() => go(sources.length ? 'explorer' : 'settings')}>
        <div className="hero-art">
          <Icon name="network" size={72} />
        </div>
        <div className="hero-text">
          <span className="hero-kicker">Fonte · Armazenamento de rede</span>
          <h1>{sources.length ? 'Explorar jogos na rede' : 'Conectar armazenamento de rede'}</h1>
          <p>
            {sources.length
              ? `${sources.map(s => s.name).join(' · ')} — navegue e copie jogos direto para este aparelho.`
              : 'Adicione um compartilhamento SMB (NAS, PC, TrueNAS) para trazer seus jogos.'}
          </p>
        </div>
      </button>

      <div className="tiles">
        <button data-nav className="tile" onClick={() => go('jobs')}>
          <div className="tile-head">
            <Icon name="download" />
            <b>Transferências</b>
          </div>
          {current ? (
            <>
              <span className="tile-line">{current.name}</span>
              <Progress value={current.total_bytes ? current.done_bytes / current.total_bytes : 0} />
              <span className="tile-sub">
                {formatBytes(current.speed)}/s
                {current.speed > 0 && ` · ${formatEta((current.total_bytes - current.done_bytes) / current.speed)}`}
                {queued > 0 && ` · +${queued} na fila`}
              </span>
            </>
          ) : (
            <span className="tile-sub">{queued ? `${queued} na fila` : 'Nada em andamento'}</span>
          )}
        </button>
        <button data-nav className="tile" onClick={() => go('store')}>
          <div className="tile-head">
            <Icon name="search" />
            <b>Loja</b>
          </div>
          <span className="tile-sub">Busque jogos nos seus indexadores</span>
        </button>
        <button data-nav className="tile" onClick={() => go('discover')}>
          <div className="tile-head">
            <Icon name="torrent" />
            <b>Descobrir</b>
          </div>
          <span className="tile-sub">Cracks mais recentes</span>
        </button>
        <button data-nav className="tile" onClick={() => go('settings')}>
          <div className="tile-head">
            <Icon name="gear" />
            <b>Configurações</b>
          </div>
          <span className="tile-sub">
            {sources.length} {sources.length === 1 ? 'fonte' : 'fontes'} · controle · sobre
          </span>
        </button>
      </div>

      <h3 className="section-title">Armazenamento</h3>
      <div className="storage">
        {storage.map(p => (
          <div key={p.id} className="storage-item">
            <div className="storage-head">
              <Icon name={p.icon} />
              <b>{p.label}</b>
              <span className="muted">
                {formatBytes(p.free)} livres de {formatBytes(p.total)}
              </span>
            </div>
            <Progress value={p.total ? 1 - p.free / p.total : 0} />
          </div>
        ))}
      </div>
    </div>
  )
}
