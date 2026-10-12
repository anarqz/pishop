// Our own file browser for the controller: folders and the files a step can
// use (a game's .exe, an installer to run in its prefix, a ROM…). On top, the
// device's drives to jump between; with `onPickSource`, a switch to the game
// sources too (network storage set up in Settings), browsed like Explore.

import { useEffect, useState } from 'react'
import { type Place, type Source, api, formatBytes } from '../api'
import { tr } from '../i18n'
import { focusFirst } from '../input'
import { Dialog, Icon, Spinner } from '../ui'

const home = (p: string) => p.replace(/^\/home\/[^/]+/, '~')
const dirOf = (p: string) => p.replace(/\/[^/]+$/, '') || '/'

/** A file on a game source (network storage): copied to the device by whoever takes it. */
export interface SourcePick {
  source_id: string
  source_name: string
  /** Relative to the share's root. */
  path: string
  name: string
  size: number
}

type Row = { name: string; dir: boolean; size: number }

export default function FilePicker({
  title,
  start,
  accept,
  onPick,
  onPickSource,
  onClose,
}: {
  title: string
  start: string
  /** Lowercase extensions, e.g. ['.exe', '.msi']; [''] takes any file. */
  accept: string[]
  onPick: (path: string) => void
  /** Offers the game sources too; a file there comes back here. */
  onPickSource?: (pick: SourcePick) => void
  onClose: () => void
}) {
  const [mode, setMode] = useState<'drives' | 'sources'>('drives')
  const [path, setPath] = useState(start || '/')
  const [places, setPlaces] = useState<Place[]>([])
  const [sources, setSources] = useState<Source[] | null>(null)
  const [src, setSrc] = useState<Source | null>(null)
  const [rpath, setRpath] = useState('')
  const [entries, setEntries] = useState<Row[] | null>(null)
  const [error, setError] = useState<string | null>(null)
  const key = accept.join(' ')
  const keep = (rows: Row[]) => {
    const types = key.split(' ')
    return rows.filter(e => e.dir || types.some(t => e.name.toLowerCase().endsWith(t)))
  }

  useEffect(() => {
    api
      .places()
      .then(ps => setPlaces(ps.filter(p => p.group === 'place')))
      .catch(() => {})
    if (onPickSource)
      api
        .sources()
        .then(setSources)
        .catch(() => setSources([]))
  }, [!!onPickSource])

  useEffect(() => {
    setEntries(null)
    setError(null)
    if (mode === 'drives') {
      api
        .listLocal(path)
        .then(r => setEntries(keep(r.entries)))
        .catch(e => setError((e as Error).message))
    } else if (src) {
      api
        .listRemote(src.id, rpath)
        .then(r => setEntries(keep(r.entries)))
        .catch(e => setError((e as Error).message))
    } else {
      setEntries([])
    }
  }, [mode, path, src?.id, rpath, key])
  useEffect(() => {
    if (entries) requestAnimationFrame(focusFirst)
  }, [entries])

  const up = mode === 'drives' ? (path !== '/' ? dirOf(path) : null) : null
  const join = (name: string) => `${path.replace(/\/$/, '')}/${name}`
  const rjoin = (name: string) => (rpath ? `${rpath}/${name}` : name)
  const rup = () => setRpath(p => (p.includes('/') ? p.replace(/\/[^/]*$/, '') : ''))
  const where =
    mode === 'drives' ? home(path) : src ? `${src.name}: /${rpath}` : tr('Network storage set up in Settings → Game sources')

  return (
    <Dialog
      title={title}
      onClose={onClose}
      wide
      hints={[
        { glyph: 'A', label: tr('Open / Select') },
        { glyph: 'B', label: tr('Close') },
      ]}
    >
      {onPickSource && (
        <div className="chips fp-modes">
          <button data-nav className={`chip ${mode === 'drives' ? 'on' : ''}`} onClick={() => setMode('drives')}>
            <Icon name="sd" size={16} /> {tr('Drives')}
          </button>
          <button data-nav className={`chip ${mode === 'sources' ? 'on' : ''}`} onClick={() => setMode('sources')}>
            <Icon name="network" size={16} /> {tr('Game sources')}
          </button>
        </div>
      )}
      {mode === 'drives' && places.length > 1 && (
        <div className="chips wrap fp-places">
          {places.map(p => (
            <button key={p.id} data-nav className={`chip ${path.startsWith(p.path) ? 'on' : ''}`} onClick={() => setPath(p.path)}>
              {p.label}
            </button>
          ))}
        </div>
      )}
      <p className="inst-path">{where}</p>
      {mode === 'sources' && src && (
        <p className="muted small">{tr('A file from here is copied to this device first (Transfers → Copies), then checked.')}</p>
      )}
      {error && <p className="inst-error">{error}</p>}
      <div className="inst-list scroll">
        {up && (
          <button data-nav className="inst-row" onClick={() => setPath(up)}>
            <Icon name="up" />
            <span className="inst-row-main">
              <b>..</b>
            </span>
          </button>
        )}
        {mode === 'sources' && src && (
          <button data-nav className="inst-row" onClick={() => (rpath ? rup() : setSrc(null))}>
            <Icon name="up" />
            <span className="inst-row-main">
              <b>..</b>
              {!rpath && <small>{tr('All game sources')}</small>}
            </span>
          </button>
        )}
        {mode === 'sources' &&
          !src &&
          (sources ?? []).map((s, i) => (
            <button
              key={s.id}
              data-nav
              data-nav-default={i === 0 ? '' : undefined}
              className="inst-row"
              onClick={() => {
                setSrc(s)
                setRpath('')
              }}
            >
              <Icon name="network" />
              <span className="inst-row-main">
                <b>{s.name}</b>
                <small>
                  //{s.host}/{s.share}
                  {s.base_path ? `/${s.base_path}` : ''}
                </small>
              </span>
            </button>
          ))}
        {mode === 'sources' && !src && sources?.length === 0 && (
          <p className="muted">{tr('No game sources yet: add your network storage in Settings → Game sources.')}</p>
        )}
        {(entries === null || (mode === 'sources' && sources === null)) && !error && (
          <p className="muted">
            <Spinner /> {tr('Loading…')}
          </p>
        )}
        {(mode === 'drives' || src) &&
          entries?.map((e, i) => (
            <button
              key={e.name}
              data-nav
              data-nav-default={i === 0 ? '' : undefined}
              className="inst-row"
              onClick={() => {
                if (mode === 'drives') return e.dir ? setPath(join(e.name)) : onPick(join(e.name))
                if (e.dir) return setRpath(rjoin(e.name))
                if (src && onPickSource) onPickSource({ source_id: src.id, source_name: src.name, path: rjoin(e.name), name: e.name, size: e.size })
              }}
            >
              <Icon name={e.dir ? 'folder' : 'file'} />
              <span className="inst-row-main">
                <b>{e.name}</b>
              </span>
              {!e.dir && <span className="inst-row-side">{formatBytes(e.size)}</span>}
            </button>
          ))}
        {(mode === 'drives' || src) && entries?.length === 0 && (
          <p className="muted">
            {key.trim() ? tr('No folders or {types} files here.', { types: accept.join(' ') }) : tr('This folder is empty.')}
          </p>
        )}
      </div>
    </Dialog>
  )
}
