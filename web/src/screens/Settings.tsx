// Settings, laid out like SteamOS settings: categories on the left, content
// on the right. "Fontes de jogos" manages where games are collected from.

import { useEffect, useRef, useState } from 'react'
import { type ServicesConfig, type Source, type SourceInput, api } from '../api'
import { BUTTON_NAMES, type PadSnapshot, input } from '../input'
import { SCALE_CHOICES, type ScaleSetting, autoScale, getScaleSetting, setScaleSetting } from '../scale'
import { Dialog, Icon, Spinner, toast, useHints } from '../ui'

type Section = 'sources' | 'indexers' | 'display' | 'controller' | 'about'

export default function Settings({
  sources,
  reloadSources,
  info,
}: {
  sources: Source[]
  reloadSources: () => void
  info: { version: string; addr: string } | null
}) {
  const [section, setSection] = useState<Section>('sources')
  const [editing, setEditing] = useState<SourceInput | null>(null)

  useHints(editing ? null : [{ glyph: 'A', label: 'Selecionar' }, { glyph: 'B', label: 'Voltar' }])

  return (
    <div className="settings" data-nav-scope>
      <nav className="settings-nav">
        {(
          [
            ['sources', 'network', 'Fontes de jogos'],
            ['indexers', 'search', 'Serviços'],
            ['display', 'display', 'Tela'],
            ['controller', 'pad', 'Controle'],
            ['about', 'info', 'Sobre'],
          ] as const
        ).map(([id, icon, label]) => (
          <button
            key={id}
            data-nav
            data-nav-default={id === section ? '' : undefined}
            className={`settings-tab ${section === id ? 'active' : ''}`}
            onFocus={() => setSection(id)}
            onClick={() => setSection(id)}
          >
            <Icon name={icon} />
            <span>{label}</span>
          </button>
        ))}
      </nav>
      <div className="settings-body">
        {section === 'sources' && (
          <SourcesSection
            sources={sources}
            onEdit={s =>
              setEditing({
                id: s.id,
                kind: 'smb',
                name: s.name,
                host: s.host,
                share: s.share,
                username: s.username,
                password: '',
                base_path: s.base_path,
              })
            }
            onAdd={() =>
              setEditing({ id: '', kind: 'smb', name: '', host: '', share: '', username: 'guest', password: '', base_path: '' })
            }
          />
        )}
        {section === 'indexers' && <IndexersSection />}
        {section === 'display' && <DisplaySection />}
        {section === 'controller' && <ControllerSection />}
        {section === 'about' && <AboutSection info={info} />}
      </div>
      {editing && (
        <SourceForm
          initial={editing}
          hasPassword={!!sources.find(s => s.id === editing.id)?.has_password}
          onClose={() => setEditing(null)}
          onSaved={() => {
            setEditing(null)
            reloadSources()
          }}
        />
      )}
    </div>
  )
}

function SourcesSection({ sources, onEdit, onAdd }: { sources: Source[]; onEdit: (s: Source) => void; onAdd: () => void }) {
  return (
    <>
      <h2 className="settings-title">Fontes de jogos</h2>
      <p className="settings-desc">
        Lugares de onde o piShop coleta jogos. Navegue por eles em <b>Explorar</b> e copie para o Deck.
      </p>
      <h3 className="section-title">Armazenamento de rede (SMB)</h3>
      <div className="settings-list">
        {sources.map(s => (
          <button key={s.id} data-nav className="settings-row" onClick={() => onEdit(s)}>
            <Icon name="network" />
            <div className="settings-row-main">
              <b>{s.name}</b>
              <small>
                \\{s.host}\{s.share}
                {s.base_path ? `\\${s.base_path.replaceAll('/', '\\')}` : ''} · {s.username || 'convidado'}
              </small>
            </div>
            <span className="settings-row-action">Editar</span>
          </button>
        ))}
        <button data-nav className="settings-row add" onClick={onAdd}>
          <Icon name="plus" />
          <div className="settings-row-main">
            <b>Adicionar armazenamento de rede</b>
            <small>Compartilhamento Windows/Samba, NAS, TrueNAS…</small>
          </div>
        </button>
      </div>
    </>
  )
}

function SourceForm({
  initial,
  hasPassword,
  onClose,
  onSaved,
}: {
  initial: SourceInput
  hasPassword: boolean
  onClose: () => void
  onSaved: () => void
}) {
  const [s, setS] = useState(initial)
  const [test, setTest] = useState<{ state: 'idle' | 'busy' | 'ok' | 'error'; msg?: string }>({ state: 'idle' })
  const [saving, setSaving] = useState(false)
  const set = (k: keyof SourceInput) => (e: React.ChangeEvent<HTMLInputElement>) => setS({ ...s, [k]: e.target.value })
  const valid = s.host.trim().length > 0 && (s.share.trim().length > 0 || /[/\\]/.test(s.host.replace(/^smb:\/\//, '').replace(/^[/\\]+/, '')))

  const runTest = async () => {
    setTest({ state: 'busy' })
    try {
      const r = await api.testSource(s)
      setTest({ state: 'ok', msg: `Conectado — ${r.entries} itens na pasta inicial` })
    } catch (e) {
      setTest({ state: 'error', msg: (e as Error).message })
    }
  }
  const save = async () => {
    setSaving(true)
    try {
      const saved = await api.saveSource(s)
      toast('Fonte salva', saved.name, 'ok')
      onSaved()
    } catch (e) {
      setSaving(false)
      toast('Não foi possível salvar', (e as Error).message, 'error')
    }
  }
  const remove = async () => {
    await api.deleteSource(s.id)
    toast('Fonte removida', s.name)
    onSaved()
  }

  return (
    <Dialog
      title={initial.id ? 'Editar armazenamento de rede' : 'Novo armazenamento de rede'}
      onClose={onClose}
      top
      wide
      hints={[
        { glyph: 'A', label: 'Editar / Selecionar' },
        { glyph: 'B', label: 'Fechar' },
      ]}
    >
      <div className="form-grid">
        <label>
          <span>Servidor</span>
          <input data-nav data-nav-default className="field" placeholder="192.168.0.10 ou nas.local" value={s.host} onChange={set('host')} />
        </label>
        <label>
          <span>Compartilhamento</span>
          <input data-nav className="field" placeholder="games" value={s.share} onChange={set('share')} />
        </label>
        <label>
          <span>Usuário</span>
          <input data-nav className="field" placeholder="guest" value={s.username} onChange={set('username')} />
        </label>
        <label>
          <span>Senha</span>
          <input
            data-nav
            className="field"
            type="password"
            placeholder={hasPassword ? '•••••• (mantida)' : ''}
            value={s.password}
            onChange={set('password')}
          />
        </label>
        <label>
          <span>Pasta inicial (opcional)</span>
          <input data-nav className="field" placeholder="consoles/roms" value={s.base_path} onChange={set('base_path')} />
        </label>
        <label>
          <span>Nome (opcional)</span>
          <input data-nav className="field" placeholder="Meu NAS" value={s.name} onChange={set('name')} />
        </label>
      </div>
      <div className={`test-result ${test.state}`}>
        {test.state === 'busy' && (
          <>
            <Spinner /> Conectando…
          </>
        )}
        {test.state === 'ok' && test.msg}
        {test.state === 'error' && test.msg}
        {test.state === 'idle' && 'Dica: “192.168.68.91:/games” no campo Servidor já preenche o compartilhamento.'}
      </div>
      <div className="dialog-actions">
        {initial.id && (
          <button data-nav className="btn danger" onClick={remove}>
            Remover
          </button>
        )}
        <span className="spacer" />
        <button data-nav className="btn" disabled={!valid || test.state === 'busy'} onClick={runTest}>
          Testar conexão
        </button>
        <button data-nav className="btn primary" disabled={!valid || saving} onClick={save}>
          Salvar
        </button>
      </div>
    </Dialog>
  )
}

type TestState = { s: 'idle' | 'busy' | 'ok' | 'error'; msg?: string }

function TestLine({ state }: { state: TestState }) {
  return (
    <div className={`test-result ${state.s}`}>
      {state.s === 'busy' ? (
        <>
          <Spinner /> Conectando…
        </>
      ) : (
        state.msg
      )}
    </div>
  )
}

/** API endpoints and keys for the online services (nothing is built in). */
function IndexersSection() {
  const [cfg, setCfg] = useState<ServicesConfig | null>(null)
  const [prowlarrUrl, setProwlarrUrl] = useState('')
  const [prowlarrKey, setProwlarrKey] = useState('')
  const [tgdbKey, setTgdbKey] = useState('')
  const [iicUrl, setIicUrl] = useState('')
  const [iicKey, setIicKey] = useState('')
  const [iicCdn, setIicCdn] = useState('')
  const [tests, setTests] = useState<Record<string, TestState>>({})
  const setTest = (k: string, v: TestState) => setTests(t => ({ ...t, [k]: v }))

  const apply = (c: ServicesConfig) => {
    setCfg(c)
    setProwlarrUrl(c.prowlarr_url)
    setIicUrl(c.iic_url)
    setIicCdn(c.iic_cdn)
  }
  useEffect(() => {
    api.catalogConfig().then(apply).catch(() => {})
  }, [])

  const save = async (u: Parameters<typeof api.saveServices>[0], label: string) => {
    try {
      apply(await api.saveServices(u))
      setProwlarrKey('')
      setTgdbKey('')
      setIicKey('')
      toast(`${label} salvo`, undefined, 'ok')
    } catch (e) {
      toast('Não foi possível salvar', (e as Error).message, 'error')
    }
  }
  const run = async (k: string, f: () => Promise<string>) => {
    setTest(k, { s: 'busy' })
    try {
      setTest(k, { s: 'ok', msg: await f() })
    } catch (e) {
      setTest(k, { s: 'error', msg: (e as Error).message })
    }
  }
  const kept = (has?: boolean) => (has ? '•••••• (mantida)' : '')

  return (
    <>
      <h2 className="settings-title">Serviços</h2>
      <p className="settings-desc">
        Endereços e chaves das APIs usadas pela Loja e pelo Descobrir. Ficam só neste aparelho.
      </p>

      <h3 className="section-title">Prowlarr · busca de torrents</h3>
      <div className="form-grid">
        <label>
          <span>Endereço</span>
          <input data-nav className="field" placeholder="http://192.168.0.10:9696" value={prowlarrUrl} onChange={e => setProwlarrUrl(e.target.value)} />
        </label>
        <label>
          <span>Chave de API</span>
          <input data-nav className="field" type="password" placeholder={kept(cfg?.has_key) || 'Settings → General → API Key'} value={prowlarrKey} onChange={e => setProwlarrKey(e.target.value)} />
        </label>
      </div>
      <TestLine state={tests.prowlarr ?? { s: 'idle' }} />
      <div className="row">
        <button data-nav className="btn" disabled={!prowlarrUrl} onClick={() => run('prowlarr', async () => {
          const r = await api.testCatalog(prowlarrUrl, prowlarrKey)
          return `Prowlarr ${r.version} · indexadores: ${r.indexers.join(', ') || 'nenhum ativo'}`
        })}>
          Testar
        </button>
        <button data-nav className="btn primary" disabled={!prowlarrUrl} onClick={() => save({ prowlarr_url: prowlarrUrl, prowlarr_key: prowlarrKey }, 'Prowlarr')}>
          Salvar
        </button>
      </div>

      <h3 className="section-title">TheGamesDB · sinopse e detalhes dos jogos</h3>
      <div className="form-grid">
        <label>
          <span>Chave de API</span>
          <input data-nav className="field" type="password" placeholder={kept(cfg?.has_tgdb_key) || 'chave pública ou privada'} value={tgdbKey} onChange={e => setTgdbKey(e.target.value)} />
        </label>
      </div>
      <TestLine state={tests.tgdb ?? { s: 'idle' }} />
      <div className="row">
        <button data-nav className="btn" disabled={!tgdbKey && !cfg?.has_tgdb_key} onClick={() => run('tgdb', async () => {
          const r = await api.testService('tgdb', { key: tgdbKey })
          return `Conectado · ${r.remaining ?? '?'} requisições restantes este mês`
        })}>
          Testar
        </button>
        <button data-nav className="btn primary" disabled={!tgdbKey} onClick={() => save({ tgdb_key: tgdbKey }, 'TheGamesDB')}>
          Salvar
        </button>
      </div>

      <h3 className="section-title">isitcracked · Descobrir</h3>
      <div className="form-grid">
        <label>
          <span>Endpoint (RPC)</span>
          <input data-nav className="field" placeholder="https://….supabase.co/rest/v1/rpc/list_games_paged" value={iicUrl} onChange={e => setIicUrl(e.target.value)} />
        </label>
        <label>
          <span>Chave de API</span>
          <input data-nav className="field" type="password" placeholder={kept(cfg?.has_iic_key) || 'apikey'} value={iicKey} onChange={e => setIicKey(e.target.value)} />
        </label>
        <label>
          <span>CDN das capas</span>
          <input data-nav className="field" placeholder="https://cdn.exemplo.com" value={iicCdn} onChange={e => setIicCdn(e.target.value)} />
        </label>
      </div>
      <TestLine state={tests.iic ?? { s: 'idle' }} />
      <div className="row">
        <button data-nav className="btn" disabled={!iicUrl} onClick={() => run('iic', async () => {
          const r = await api.testService('iic', { url: iicUrl, key: iicKey, cdn: iicCdn })
          return `Conectado · ${r.total ?? 0} jogos crackeados`
        })}>
          Testar
        </button>
        <button data-nav className="btn primary" disabled={!iicUrl} onClick={() => save({ iic_url: iicUrl, iic_key: iicKey, iic_cdn: iicCdn }, 'isitcracked')}>
          Salvar
        </button>
      </div>
    </>
  )
}

function DisplaySection() {
  const [setting, setSetting] = useState<ScaleSetting>(getScaleSetting)
  const [auto, setAuto] = useState(autoScale)
  useEffect(() => {
    const refresh = () => setAuto(autoScale())
    window.addEventListener('resize', refresh)
    return () => window.removeEventListener('resize', refresh)
  }, [])
  const pct = (v: number) => `${Math.round(v * 100)}%`
  return (
    <>
      <h2 className="settings-title">Tela</h2>
      <p className="settings-desc">
        Tamanho da interface. No automático o piShop segue a proporção da interface da Steam para a sua tela (
        {window.innerWidth}×{window.innerHeight}).
      </p>
      <h3 className="section-title">Escala da interface</h3>
      <div className="settings-list">
        {SCALE_CHOICES.map(v => (
          <button
            key={String(v)}
            data-nav
            className={`settings-row ${setting === v ? 'selected' : ''}`}
            onClick={() => {
              setSetting(v)
              setScaleSetting(v)
            }}
          >
            <Icon name={setting === v ? 'check' : 'display'} />
            <div className="settings-row-main">
              <b>{v === 'auto' ? `Automático (${pct(auto)})` : pct(v)}</b>
              <small>{v === 'auto' ? 'Recomendado — acompanha a resolução da tela' : 'Tamanho fixo'}</small>
            </div>
          </button>
        ))}
      </div>
    </>
  )
}

function ControllerSection() {
  const [pads, setPads] = useState<PadSnapshot[]>([])
  const last = useRef('')
  useEffect(
    () =>
      input.onPads(next => {
        const key = JSON.stringify(next.map(p => [p.id, p.buttons.map(b => b.toFixed(2)), p.axes.map(a => a.toFixed(2))]))
        if (key !== last.current) {
          last.current = key
          setPads(next)
        }
      }),
    [],
  )
  return (
    <>
      <h2 className="settings-title">Controle</h2>
      <p className="settings-desc">Aperte os botões para testar. Trackpads funcionam como mouse e a tela é touch.</p>
      {pads.length === 0 && <p className="muted">Nenhum controle detectado. Aperte qualquer botão.</p>}
      {pads.map(p => (
        <div key={p.index} className="pad">
          <div className="pad-id">
            {p.id} <span className="muted">({p.mapping || 'sem mapeamento'})</span>
          </div>
          <div className="pad-buttons">
            {p.buttons.map((v, i) => (
              <span key={i} className={`pad-btn ${v > 0.1 ? 'on' : ''}`}>
                {BUTTON_NAMES[i] ?? `B${i}`}
              </span>
            ))}
          </div>
          <div className="sticks">
            {[0, 2].map(i => (
              <div key={i} className="stick">
                <div className="knob" style={{ transform: `translate(${(p.axes[i] ?? 0) * 28}px, ${(p.axes[i + 1] ?? 0) * 28}px)` }} />
                <span>{i === 0 ? 'L' : 'R'}</span>
              </div>
            ))}
          </div>
        </div>
      ))}
    </>
  )
}

function AboutSection({ info }: { info: { version: string; addr: string } | null }) {
  const [quitting, setQuitting] = useState(false)
  return (
    <>
      <h2 className="settings-title">Sobre</h2>
      <div className="about">
        <img src="/icon.svg" alt="" />
        <div>
          <b>piShop {info?.version}</b>
          <small>Servidor local em {info?.addr}</small>
        </div>
      </div>
      <button
        data-nav
        className="btn danger"
        disabled={quitting}
        onClick={() => {
          setQuitting(true)
          void api.quit().catch(() => {})
        }}
      >
        {quitting ? 'Saindo…' : 'Sair do piShop'}
      </button>
    </>
  )
}
