// Our own file browser for the controller: folders and the files a step can
// use (a game's .exe, an installer to run in its prefix…).

import { useEffect, useState } from 'react'
import { api, formatBytes } from '../api'
import { tr } from '../i18n'
import { focusFirst } from '../input'
import { Dialog, Icon, Spinner } from '../ui'

const home = (p: string) => p.replace(/^\/home\/[^/]+/, '~')
const dirOf = (p: string) => p.replace(/\/[^/]+$/, '') || '/'

export default function FilePicker({
  title,
  start,
  accept,
  onPick,
  onClose,
}: {
  title: string
  start: string
  /** Lowercase extensions, e.g. ['.exe', '.msi']. */
  accept: string[]
  onPick: (path: string) => void
  onClose: () => void
}) {
  const [path, setPath] = useState(start || '/')
  const [entries, setEntries] = useState<Array<{ name: string; dir: boolean; size: number }> | null>(null)
  const [error, setError] = useState<string | null>(null)
  const key = accept.join(' ')
  useEffect(() => {
    setEntries(null)
    const types = key.split(' ')
    api
      .listLocal(path)
      .then(r => {
        setEntries(r.entries.filter(e => e.dir || types.some(t => e.name.toLowerCase().endsWith(t))))
        setError(null)
      })
      .catch(e => setError((e as Error).message))
  }, [path, key])
  useEffect(() => {
    if (entries) requestAnimationFrame(focusFirst)
  }, [entries])
  const up = path !== '/' ? dirOf(path) : null
  const join = (name: string) => `${path.replace(/\/$/, '')}/${name}`
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
      <p className="inst-path">{home(path)}</p>
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
        {entries === null && !error && (
          <p className="muted">
            <Spinner /> {tr('Loading…')}
          </p>
        )}
        {entries?.map((e, i) => (
          <button
            key={e.name}
            data-nav
            data-nav-default={i === 0 ? '' : undefined}
            className="inst-row"
            onClick={() => (e.dir ? setPath(join(e.name)) : onPick(join(e.name)))}
          >
            <Icon name={e.dir ? 'folder' : 'file'} />
            <span className="inst-row-main">
              <b>{e.name}</b>
            </span>
            {!e.dir && <span className="inst-row-side">{formatBytes(e.size)}</span>}
          </button>
        ))}
        {entries?.length === 0 && <p className="muted">{tr('No folders or {types} files here.', { types: accept.join(' ') })}</p>}
      </div>
    </Dialog>
  )
}
