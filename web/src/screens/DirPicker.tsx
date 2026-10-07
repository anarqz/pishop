// Picks a folder and gives it back (where to download, a game's folder…):
// the storage status of the folder open now stays on top, quick locations
// (folders, cards, Steam libraries) are one button away, and folders can be
// created on the spot. A opens a folder, Y picks the one open, B goes up.

import { useEffect, useRef, useState } from 'react'
import { type Entry, type Place, api, joinPath, parentPath } from '../api'
import { tr } from '../i18n'
import { focusFirst, input } from '../input'
import { StorageCard, homePath } from '../storage'
import { Dialog, Icon, Spinner, TextPrompt, toast } from '../ui'

/** The quick location a path is in (the deepest one), if any. */
export function placeFor(places: Place[], path: string): Place | undefined {
  return places
    .filter(p => path === p.path || path.startsWith(p.path.endsWith('/') ? p.path : `${p.path}/`))
    .sort((a, b) => b.path.length - a.path.length)[0]
}

export default function DirPicker({
  title,
  start,
  confirmLabel,
  onPick,
  onClose,
}: {
  title: string
  start: string
  confirmLabel?: string
  onPick: (path: string) => void
  onClose: () => void
}) {
  const [path, setPath] = useState(start || '/')
  const [list, setList] = useState<{ entries: Entry[]; free: number | null; total: number | null } | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [places, setPlaces] = useState<Place[]>([])
  const [view, setView] = useState<'browse' | 'places' | 'mkdir'>('browse')
  const [nonce, setNonce] = useState(0)

  useEffect(() => {
    api
      .places()
      .then(p => setPlaces(p.filter(x => x.group !== 'prefix')))
      .catch(() => {})
  }, [])
  useEffect(() => {
    let alive = true
    setList(null)
    api
      .listLocal(path)
      .then(r => {
        if (!alive) return
        setList({ entries: r.entries.filter(e => e.dir), free: r.free, total: r.total })
        setError(null)
      })
      .catch(e => alive && setError((e as Error).message))
    return () => {
      alive = false
    }
  }, [path, nonce])
  useEffect(() => {
    if ((list || error) && view === 'browse') requestAnimationFrame(focusFirst)
  }, [list, error, view])

  const here = placeFor(places, path)
  const up = path !== '/' ? parentPath(path) || '/' : null

  // B goes up a folder before closing; Y picks the folder open now.
  const pick = useRef(onPick)
  pick.current = onPick
  const state = useRef({ up, path, view })
  state.current = { up, path, view }
  useEffect(
    () =>
      input.pushHandler(a => {
        const s = state.current
        if (s.view !== 'browse') return false
        if (a === 'back' && s.up) {
          setPath(s.up)
          return true
        }
        if (a === 'y') {
          pick.current(s.path)
          return true
        }
        return false
      }),
    [],
  )

  const name = path === '/' ? '/' : (path.split('/').filter(Boolean).pop() ?? path)
  return (
    <>
      <Dialog
        title={title}
        onClose={onClose}
        wide
        hints={[
          { glyph: 'A', label: tr('Open') },
          { glyph: 'Y', label: confirmLabel ?? tr('Use this folder') },
          { glyph: 'B', label: up ? tr('Up a folder') : tr('Close') },
        ]}
      >
        <StorageCard
          icon={here?.icon ?? 'folder'}
          title={here && here.path === path ? here.label : name}
          path={path}
          free={list?.free}
          total={list?.total}
          disk={here?.disk}
        />
        {error && <p className="inst-error">{error}</p>}
        <div className="inst-list scroll dirpick-list">
          {up && (
            <button data-nav className="inst-row" onClick={() => setPath(up)}>
              <Icon name="up" />
              <span className="inst-row-main">
                <b>..</b>
                <small>{homePath(up)}</small>
              </span>
            </button>
          )}
          {list === null && !error && (
            <p className="muted">
              <Spinner /> {tr('Loading…')}
            </p>
          )}
          {list?.entries.map((e, i) => (
            <button
              key={e.name}
              data-nav
              data-nav-default={i === 0 ? '' : undefined}
              className="inst-row"
              onClick={() => setPath(joinPath(path, e.name))}
            >
              <Icon name="folder" />
              <span className="inst-row-main">
                <b>{e.name}</b>
              </span>
            </button>
          ))}
          {list?.entries.length === 0 && <p className="muted">{tr('No folders here.')}</p>}
        </div>
        <div className="dialog-actions">
          <button data-nav className="btn" onClick={() => setView('places')}>
            <Icon name="home" size={18} /> {tr('Locations…')}
          </button>
          <button data-nav className="btn" disabled={!list} onClick={() => setView('mkdir')}>
            <Icon name="plus" size={18} /> {tr('New folder…')}
          </button>
          <span className="spacer" />
          <button data-nav className="btn primary" disabled={!list} onClick={() => onPick(path)}>
            {confirmLabel ?? tr('Use this folder')}
          </button>
        </div>
      </Dialog>
      {view === 'places' && (
        <Dialog title={tr('Locations')} onClose={() => setView('browse')} wide>
          <div className="inst-list scroll">
            {places.map((p, i) => (
              <StorageCard
                key={p.id}
                icon={p.icon}
                title={p.label}
                path={p.path}
                free={p.free}
                total={p.total}
                disk={p.disk}
                navDefault={here ? p.id === here.id : i === 0}
                onClick={() => {
                  setPath(p.path)
                  setView('browse')
                }}
              />
            ))}
          </div>
        </Dialog>
      )}
      {view === 'mkdir' && (
        <TextPrompt
          title={tr('New folder in {path}', { path: homePath(path) })}
          placeholder={tr('Folder name')}
          submitLabel={tr('Create')}
          validate={v => v.trim().length > 0 && !v.includes('/')}
          onCancel={() => setView('browse')}
          onSubmit={async v => {
            const to = joinPath(path, v.trim())
            try {
              await api.mkdir(to)
              setView('browse')
              setPath(to)
              setNonce(n => n + 1)
            } catch (e) {
              setView('browse')
              toast(tr("Couldn't create the folder"), (e as Error).message, 'error')
            }
          }}
        />
      )}
    </>
  )
}
