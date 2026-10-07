// Settings, laid out like SteamOS settings: categories on the left, content
// on the right. "Game sources" manages where games are collected from.

import { useEffect, useState } from 'react'
import { type ServicesConfig, type ServicesFile, type Source, type SourceInput, api } from '../api'
import { type Lang, LANGS, locale, setLang, tr, trb, trn, useLang } from '../i18n'
import { SCALE_CHOICES, type ScaleSetting, autoScale, getScaleSetting, setScaleSetting } from '../scale'
import { Dialog, Icon, Spinner, toast, useHints } from '../ui'

export type SettingsSection = 'sources' | 'indexers' | 'downloads' | 'display' | 'language' | 'about'
type Section = SettingsSection

export default function Settings({
  sources,
  reloadSources,
  info,
  initialSection,
}: {
  sources: Source[]
  reloadSources: () => void
  info: { version: string; addr: string } | null
  /** Where another screen sent the user ("Set up isitcracked" → Services). */
  initialSection?: SettingsSection
}) {
  const [section, setSection] = useState<Section>(initialSection ?? 'sources')
  const [editing, setEditing] = useState<SourceInput | null>(null)

  useHints(editing ? null : [{ glyph: 'A', label: tr('Select') }, { glyph: 'B', label: tr('Back') }])

  return (
    <div className="settings" data-nav-scope>
      <nav className="settings-nav" data-nav-exit-right=".settings-body [data-nav]:not(:disabled)" data-nav-exit-up="none" data-nav-exit-down="none">
        {(
          [
            ['sources', 'network', tr('Game sources')],
            ['indexers', 'search', tr('Services')],
            ['downloads', 'download', tr('Downloads')],
            ['display', 'display', tr('Display')],
            ['language', 'globe', tr('Language')],
            ['about', 'info', tr('About')],
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
      {/* Left goes back to the category; up/down never wander into another one. */}
      <div className="settings-body" data-nav-exit-left=".settings-tab.active" data-nav-exit-up="none" data-nav-exit-down="none">
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
        {section === 'downloads' && <DownloadsSection />}
        {section === 'display' && <DisplaySection />}
        {section === 'language' && <LanguageSection />}
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
      <h2 className="settings-title">{tr('Game sources')}</h2>
      <p className="settings-desc">{trb('Places piShop collects games from. Browse them in **Explore** and copy them to this device.')}</p>
      <h3 className="section-title">{tr('Network storage (SMB)')}</h3>
      <div className="settings-list">
        {sources.map(s => (
          <button key={s.id} data-nav className="settings-row" onClick={() => onEdit(s)}>
            <Icon name="network" />
            <div className="settings-row-main">
              <b>{s.name}</b>
              <small>
                \\{s.host}\{s.share}
                {s.base_path ? `\\${s.base_path.replaceAll('/', '\\')}` : ''} · {s.username || tr('guest')}
              </small>
            </div>
            <span className="settings-row-action">{tr('Edit')}</span>
          </button>
        ))}
        <button data-nav className="settings-row add" onClick={onAdd}>
          <Icon name="plus" />
          <div className="settings-row-main">
            <b>{tr('Add network storage')}</b>
            <small>{tr('Windows/Samba share, NAS, TrueNAS…')}</small>
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
      setTest({ state: 'ok', msg: trn(r.entries, 'Connected — {n} item in the start folder', 'Connected — {n} items in the start folder') })
    } catch (e) {
      setTest({ state: 'error', msg: (e as Error).message })
    }
  }
  const save = async () => {
    setSaving(true)
    try {
      const saved = await api.saveSource(s)
      toast(tr('Source saved'), saved.name, 'ok')
      onSaved()
    } catch (e) {
      setSaving(false)
      toast(tr("Couldn't save"), (e as Error).message, 'error')
    }
  }
  const remove = async () => {
    await api.deleteSource(s.id)
    toast(tr('Source removed'), s.name)
    onSaved()
  }

  return (
    <Dialog
      title={initial.id ? tr('Edit network storage') : tr('New network storage')}
      onClose={onClose}
      top
      wide
      hints={[
        { glyph: 'A', label: tr('Edit / Select') },
        { glyph: 'B', label: tr('Close') },
      ]}
    >
      <div className="form-grid">
        <label>
          <span>{tr('Server')}</span>
          <input data-nav data-nav-default className="field" placeholder={tr('192.168.0.10 or nas.local')} value={s.host} onChange={set('host')} />
        </label>
        <label>
          <span>{tr('Share')}</span>
          <input data-nav className="field" placeholder="games" value={s.share} onChange={set('share')} />
        </label>
        <label>
          <span>{tr('User')}</span>
          <input data-nav className="field" placeholder="guest" value={s.username} onChange={set('username')} />
        </label>
        <label>
          <span>{tr('Password')}</span>
          <input
            data-nav
            className="field"
            type="password"
            placeholder={hasPassword ? tr('•••••• (kept)') : ''}
            value={s.password}
            onChange={set('password')}
          />
        </label>
        <label>
          <span>{tr('Start folder (optional)')}</span>
          <input data-nav className="field" placeholder="consoles/roms" value={s.base_path} onChange={set('base_path')} />
        </label>
        <label>
          <span>{tr('Name (optional)')}</span>
          <input data-nav className="field" placeholder={tr('My NAS')} value={s.name} onChange={set('name')} />
        </label>
      </div>
      <div className={`test-result ${test.state}`}>
        {test.state === 'busy' && (
          <>
            <Spinner /> {tr('Connecting…')}
          </>
        )}
        {test.state === 'ok' && test.msg}
        {test.state === 'error' && test.msg}
        {test.state === 'idle' && tr('Tip: “192.168.0.10:/games” in the Server field fills in the share too.')}
      </div>
      <div className="dialog-actions">
        {initial.id && (
          <button data-nav className="btn danger" onClick={remove}>
            {tr('Remove')}
          </button>
        )}
        <span className="spacer" />
        <button data-nav className="btn" disabled={!valid || test.state === 'busy'} onClick={runTest}>
          {tr('Test connection')}
        </button>
        <button data-nav className="btn primary" disabled={!valid || saving} onClick={save}>
          {tr('Save')}
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
          <Spinner /> {tr('Connecting…')}
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
  const [tpbUrl, setTpbUrl] = useState('')
  const [tests, setTests] = useState<Record<string, TestState>>({})
  const setTest = (k: string, v: TestState) => setTests(t => ({ ...t, [k]: v }))

  const apply = (c: ServicesConfig) => {
    setCfg(c)
    setProwlarrUrl(c.prowlarr_url)
    setIicUrl(c.iic_url)
    setIicCdn(c.iic_cdn)
    setTpbUrl(c.tpb_url ?? '')
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
      toast(tr('{name} saved', { name: label }), undefined, 'ok')
    } catch (e) {
      toast(tr("Couldn't save"), (e as Error).message, 'error')
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
  const kept = (has?: boolean) => (has ? tr('•••••• (kept)') : '')
  const [importing, setImporting] = useState(false)
  const exportFile = async () => {
    try {
      const r = await api.exportServices()
      toast(tr('Services exported'), r.path.replace(/^\/home\/[^/]+/, '~'), 'ok')
    } catch (e) {
      toast(tr("Couldn't export"), (e as Error).message, 'error')
    }
  }

  return (
    <>
      <h2 className="settings-title">{tr('Services')}</h2>
      <p className="settings-desc">{tr("Addresses and keys of the APIs the Store and Discover use. They're kept on this device.")}</p>

      <h3 className="section-title">{tr('Share your setup')}</h3>
      <p className="settings-desc">
        {tr('Export saves these services, API keys included, to a file in Downloads. Import loads a file someone shared with you.')}
      </p>
      <div className="row">
        <button data-nav className="btn" onClick={() => setImporting(true)}>
          <Icon name="download" size={18} /> {tr('Import…')}
        </button>
        <button data-nav className="btn" onClick={() => void exportFile()}>
          <Icon name="up" size={18} /> {tr('Export')}
        </button>
      </div>
      <p className="settings-note">{tr('The file includes your API keys: share it only with people you trust.')}</p>

      <h3 className="section-title">{tr('The Pirate Bay · native torrent search')}</h3>
      <div className="form-grid">
        <label>
          <span>{tr('API address (apibay format)')}</span>
          <input data-nav className="field" placeholder="https://apibay.org" value={tpbUrl} onChange={e => setTpbUrl(e.target.value)} />
        </label>
      </div>
      <p className="settings-note">{tr('Empty: The Pirate Bay’s own API (apibay.org) whenever Prowlarr isn’t set up, so the Store always works.')}</p>
      <TestLine state={tests.tpb ?? { s: 'idle' }} />
      <div className="row">
        <button data-nav className="btn" onClick={() => run('tpb', async () => {
          const r = await api.testService('tpb', { url: tpbUrl || 'https://apibay.org' })
          return trn(r.total ?? 0, 'Connected · {n} result for the test search', 'Connected · {n} results for the test search')
        })}>
          {tr('Test')}
        </button>
        <button data-nav className="btn primary" onClick={() => save({ tpb_url: tpbUrl }, 'The Pirate Bay')}>
          {tr('Save')}
        </button>
      </div>

      <h3 className="section-title">{tr('Prowlarr · torrent search')}</h3>
      <div className="form-grid">
        <label>
          <span>{tr('Address')}</span>
          <input data-nav className="field" placeholder="http://192.168.0.10:9696" value={prowlarrUrl} onChange={e => setProwlarrUrl(e.target.value)} />
        </label>
        <label>
          <span>{tr('API key')}</span>
          <input data-nav className="field" type="password" placeholder={kept(cfg?.has_key) || 'Settings → General → API Key'} value={prowlarrKey} onChange={e => setProwlarrKey(e.target.value)} />
        </label>
      </div>
      <TestLine state={tests.prowlarr ?? { s: 'idle' }} />
      <div className="row">
        <button data-nav className="btn" disabled={!prowlarrUrl} onClick={() => run('prowlarr', async () => {
          const r = await api.testCatalog(prowlarrUrl, prowlarrKey)
          return tr('Prowlarr {version} · indexers: {list}', { version: r.version, list: r.indexers.join(', ') || tr('none enabled') })
        })}>
          {tr('Test')}
        </button>
        <button data-nav className="btn primary" disabled={!prowlarrUrl} onClick={() => save({ prowlarr_url: prowlarrUrl, prowlarr_key: prowlarrKey }, 'Prowlarr')}>
          {tr('Save')}
        </button>
      </div>

      <h3 className="section-title">{tr('TheGamesDB · game synopsis and details')}</h3>
      <div className="form-grid">
        <label>
          <span>{tr('API key')}</span>
          <input data-nav className="field" type="password" placeholder={kept(cfg?.has_tgdb_key) || tr('public or private key')} value={tgdbKey} onChange={e => setTgdbKey(e.target.value)} />
        </label>
      </div>
      <TestLine state={tests.tgdb ?? { s: 'idle' }} />
      <div className="row">
        <button data-nav className="btn" disabled={!tgdbKey && !cfg?.has_tgdb_key} onClick={() => run('tgdb', async () => {
          const r = await api.testService('tgdb', { key: tgdbKey })
          return tr('Connected · {n} requests left this month', { n: r.remaining ?? '?' })
        })}>
          {tr('Test')}
        </button>
        <button data-nav className="btn primary" disabled={!tgdbKey} onClick={() => save({ tgdb_key: tgdbKey }, 'TheGamesDB')}>
          {tr('Save')}
        </button>
      </div>

      <h3 className="section-title">{tr('isitcracked · Discover')}</h3>
      <div className="form-grid">
        <label>
          <span>Endpoint (RPC)</span>
          <input data-nav className="field" placeholder="https://….supabase.co/rest/v1/rpc/list_games_paged" value={iicUrl} onChange={e => setIicUrl(e.target.value)} />
        </label>
        <label>
          <span>{tr('API key')}</span>
          <input data-nav className="field" type="password" placeholder={kept(cfg?.has_iic_key) || 'apikey'} value={iicKey} onChange={e => setIicKey(e.target.value)} />
        </label>
        <label>
          <span>{tr('Covers CDN')}</span>
          <input data-nav className="field" placeholder="https://cdn.example.com" value={iicCdn} onChange={e => setIicCdn(e.target.value)} />
        </label>
      </div>
      <TestLine state={tests.iic ?? { s: 'idle' }} />
      <div className="row">
        <button data-nav className="btn" disabled={!iicUrl} onClick={() => run('iic', async () => {
          const r = await api.testService('iic', { url: iicUrl, key: iicKey, cdn: iicCdn })
          return trn(r.total ?? 0, 'Connected · {n} cracked game', 'Connected · {n} cracked games')
        })}>
          {tr('Test')}
        </button>
        <button data-nav className="btn primary" disabled={!iicUrl} onClick={() => save({ iic_url: iicUrl, iic_key: iicKey, iic_cdn: iicCdn }, 'isitcracked')}>
          {tr('Save')}
        </button>
      </div>
      {importing && (
        <ImportDialog
          onClose={() => setImporting(false)}
          onImported={() => {
            setImporting(false)
            api.catalogConfig().then(apply).catch(() => {})
          }}
        />
      )}
    </>
  )
}

/** Picks a piShop services file found on the device and applies it. */
function ImportDialog({ onClose, onImported }: { onClose: () => void; onImported: () => void }) {
  const [files, setFiles] = useState<ServicesFile[] | null>(null)
  const [busy, setBusy] = useState(false)
  useEffect(() => {
    api
      .importCandidates()
      .then(setFiles)
      .catch(() => setFiles([]))
  }, [])
  const pick = async (f: ServicesFile) => {
    setBusy(true)
    try {
      const r = await api.importServices(f.path)
      toast(tr('Services imported'), r.imported.join(', '), 'ok')
      onImported()
    } catch (e) {
      setBusy(false)
      toast(tr("Couldn't import"), (e as Error).message, 'error')
    }
  }
  const when = (secs: number) =>
    new Date(secs * 1000).toLocaleString(locale(), { day: '2-digit', month: 'short', year: 'numeric', hour: '2-digit', minute: '2-digit' })
  return (
    <Dialog title={tr('Import services')} onClose={onClose} wide>
      {files === null && (
        <p className="muted">
          <Spinner /> {tr('Looking for files…')}
        </p>
      )}
      {files?.length === 0 && (
        <p className="settings-desc">
          {trb('No piShop services file found. Put **{file}** in Downloads, your home folder, or on an SD card or USB drive.', {
            file: 'piShop-services.json',
          })}
        </p>
      )}
      {!!files?.length && (
        <div className="settings-list">
          {files.map((f, i) => (
            <button key={f.path} data-nav data-nav-default={i === 0 ? '' : undefined} className="settings-row" disabled={busy} onClick={() => void pick(f)}>
              <Icon name="file" />
              <div className="settings-row-main">
                <b>{f.name}</b>
                <small>
                  {f.path.replace(/^\/home\/[^/]+/, '~').replace(/\/[^/]+$/, '')} · {when(f.modified)}
                </small>
              </div>
              <span className="settings-row-action">{f.services.join(' · ')}</span>
            </button>
          ))}
        </div>
      )}
      <div className="dialog-actions">
        <button data-nav data-nav-default={files?.length ? undefined : ''} className="btn" onClick={onClose}>
          {tr('Cancel')}
        </button>
      </div>
    </Dialog>
  )
}

const MIB = 1024 * 1024
/** Download caps offered, in MB/s (null = unlimited). */
const SPEED_LIMITS = [null, 1, 2, 5, 10, 20, 50] as const

function DownloadsSection() {
  // bytes/s; undefined while loading.
  const [limit, setLimit] = useState<number | null | undefined>(undefined)
  useEffect(() => {
    api
      .torrentLimits()
      .then(l => setLimit(l.download_bps))
      .catch(() => setLimit(null))
  }, [])
  const choose = (mb: number | null) =>
    api
      .setTorrentLimits(mb ? mb * MIB : null)
      .then(l => {
        setLimit(l.download_bps)
        toast(tr('Download limit'), mb ? `${mb} MB/s` : tr('No limit'), 'ok')
      })
      .catch(e => toast(tr("Couldn't save"), (e as Error).message, 'error'))
  const perHour = (mb: number) => ((mb * 3600) / 1024).toLocaleString(locale(), { maximumFractionDigits: 1 })
  return (
    <>
      <h2 className="settings-title">{tr('Downloads')}</h2>
      <p className="settings-desc">{tr("Top speed for piShop's torrent client, across all downloads. It changes right away, no restart needed.")}</p>
      <h3 className="section-title">{tr('Download limit')}</h3>
      <div className="settings-list">
        {SPEED_LIMITS.map(mb => {
          const selected = limit !== undefined && (mb ? limit === mb * MIB : !limit)
          return (
            <button key={String(mb)} data-nav className={`settings-row ${selected ? 'selected' : ''}`} onClick={() => void choose(mb)}>
              <Icon name={selected ? 'check' : 'download'} />
              <div className="settings-row-main">
                <b>{mb ? `${mb} MB/s` : tr('No limit')}</b>
                <small>{mb ? tr('Up to {gb} GB per hour', { gb: perHour(mb) }) : tr('Uses your full connection speed')}</small>
              </div>
            </button>
          )
        })}
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
      <h2 className="settings-title">{tr('Display')}</h2>
      <p className="settings-desc">
        {tr("Interface size. On automatic, piShop follows the Steam interface's proportions for your screen ({w}×{h}).", {
          w: window.innerWidth,
          h: window.innerHeight,
        })}
      </p>
      <h3 className="section-title">{tr('Interface scale')}</h3>
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
              <b>{v === 'auto' ? tr('Automatic ({pct})', { pct: pct(auto) }) : pct(v)}</b>
              <small>{v === 'auto' ? tr('Recommended — follows the screen resolution') : tr('Fixed size')}</small>
            </div>
          </button>
        ))}
      </div>
    </>
  )
}

function LanguageSection() {
  const lang = useLang()
  const choose = async (l: Lang) => {
    setLang(l)
    try {
      await api.saveSettings({ lang: l })
    } catch (e) {
      toast(tr("Couldn't save"), (e as Error).message, 'error')
    }
  }
  return (
    <>
      <h2 className="settings-title">{tr('Language')}</h2>
      <p className="settings-desc">{tr('The language of piShop. Game descriptions from Steam follow it too.')}</p>
      <div className="settings-list">
        {LANGS.map(([id, name]) => (
          <button key={id} data-nav className={`settings-row ${lang === id ? 'selected' : ''}`} onClick={() => void choose(id)}>
            <Icon name={lang === id ? 'check' : 'globe'} />
            <div className="settings-row-main">
              <b>{name}</b>
              <small>{id === 'en' ? tr('Default') : tr('Brazilian Portuguese')}</small>
            </div>
          </button>
        ))}
      </div>
    </>
  )
}

function AboutSection({ info }: { info: { version: string; addr: string } | null }) {
  const [quitting, setQuitting] = useState(false)
  return (
    <>
      <h2 className="settings-title">{tr('About')}</h2>
      <div className="about">
        <img src="/icon.svg" alt="" />
        <div>
          <b>piShop {info?.version}</b>
          <small>{tr('Local server at {addr}', { addr: info?.addr ?? '' })}</small>
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
        {quitting ? tr('Quitting…') : tr('Quit piShop')}
      </button>
    </>
  )
}
