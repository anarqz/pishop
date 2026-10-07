// Two-pane explorer, driven by the controller. Both panes are alike and start
// in the home folder; each can go to any quick location — folders and cards,
// Steam libraries, each non-Steam game's prefix (its C:), network shares —
// with L2/R2 or the pane's header, and shows how full its disk is. Y copies
// what's marked (or the item under the cursor) into the folder open on the
// other side. Other tabs open a folder here, on the left (`request`). Lists
// are virtualized, so folders with tens of thousands of entries stay instant.

import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { type Entry, type Place, type Source, api, formatBytes, joinPath, parentPath } from '../api'
import { type Action, focusFirst, input } from '../input'
import { guessSystem } from '../systems'
import { tr, trn } from '../i18n'
import { StorageCard, StorageStatus, homePath } from '../storage'
import { Dialog, type Hint, Icon, Spinner, TextPrompt, toast, useHints } from '../ui'

const ROW_H = 46

// ---------- listing cache (stale-while-revalidate) ----------

interface Listing {
  entries: Entry[]
  free?: number | null
  total?: number | null
}
const cache = new Map<string, Listing>()

function useListing(key: string | null, load: () => Promise<Listing>) {
  const [state, setState] = useState<{ key: string | null; data?: Listing; error?: string; loading: boolean }>({
    key,
    data: key ? cache.get(key) : undefined,
    loading: !!key,
  })
  const [nonce, setNonce] = useState(0)
  const loadRef = useRef(load)
  loadRef.current = load

  useEffect(() => {
    if (!key) return
    let alive = true
    setState({ key, data: cache.get(key), loading: true })
    loadRef
      .current()
      .then(d => {
        cache.set(key, d)
        if (alive) setState({ key, data: d, loading: false })
      })
      .catch(e => alive && setState({ key, data: cache.get(key), error: String(e.message ?? e), loading: false }))
    return () => {
      alive = false
    }
  }, [key, nonce])

  return { ...state, reload: () => setNonce(n => n + 1) }
}

// ---------- locations ----------

/** Where a pane is: this device (absolute paths) or a network share (paths inside it). */
type Where = { kind: 'local' } | { kind: 'smb'; id: string }

/** A quick location: a place on this device or a network share. */
interface Loc {
  id: string
  label: string
  icon: string
  where: Where
  /** Absolute path (device) or the share's start folder. */
  path: string
  group: Place['group'] | 'network'
  free?: number
  total?: number
  disk?: string
}

function roster(places: Place[], sources: Source[]): Loc[] {
  const order: Record<Loc['group'], number> = { place: 0, library: 1, prefix: 2, network: 3 }
  const local: Loc[] = places.map(p => ({
    id: p.id,
    label: p.label,
    icon: p.icon,
    where: { kind: 'local' },
    path: p.path,
    group: p.group ?? 'place',
    free: p.free,
    total: p.total,
    disk: p.disk,
  }))
  // Home first: both panes start there.
  local.sort((a, b) => order[a.group] - order[b.group] || (a.id === 'home' ? -1 : b.id === 'home' ? 1 : 0))
  const shares: Loc[] = sources.map(s => ({
    id: `smb:${s.id}`,
    label: s.name,
    icon: 'network',
    where: { kind: 'smb', id: s.id },
    path: s.base_path,
    group: 'network',
    disk: `\\\\${s.host}\\${s.share}`,
  }))
  return [...local, ...shares]
}

const sameWhere = (a: Where, b: Where) => a.kind === b.kind && (a.kind === 'local' || (b.kind === 'smb' && a.id === b.id))

/** The quick location a pane's folder is in (the deepest match). */
function locFor(locs: Loc[], where: Where, path: string): Loc | undefined {
  return locs
    .filter(l => sameWhere(l.where, where))
    .filter(l => path === l.path || path.startsWith(l.path.endsWith('/') ? l.path : `${l.path}/`) || (where.kind === 'smb' && !l.path))
    .sort((a, b) => b.path.length - a.path.length)[0]
}

// ---------- remembered state across tab switches ----------

interface PaneMem {
  where: Where
  path: string
  cursor: number
}
const memory: { panes: [PaneMem, PaneMem] | null; active: 0 | 1; handled: number } = {
  panes: null,
  active: 0,
  /** Last request already applied (so coming back to the tab doesn't redo it). */
  handled: 0,
}

/** Opens a local folder in the left pane (or a file, selected in its folder). */
export interface ExplorerRequest {
  path: string
  n: number
}

interface Row {
  kind: 'up' | 'entry'
  entry?: Entry
}

export default function Explorer({
  sources,
  places,
  request,
  onGoSettings,
}: {
  sources: Source[]
  places: Place[]
  request?: ExplorerRequest | null
  onGoSettings: () => void
}) {
  const locs = useMemo(() => roster(places, sources), [places, sources])
  const homeDir = places.find(p => p.id === 'home')?.path ?? ''
  const start: PaneMem = { where: { kind: 'local' }, path: homeDir, cursor: 0 }
  const [active, setActive] = useState<0 | 1>(memory.active)
  const [head, setHead] = useState<[boolean, boolean]>([false, false])
  const [panes, setPanes] = useState<[PaneMem, PaneMem]>(memory.panes ?? [start, start])
  const [filters, setFilters] = useState<[string, string]>(['', ''])
  const [marked, setMarked] = useState<{ side: 0 | 1; names: Set<string> }>({ side: 0, names: new Set() })
  const [dialog, setDialog] = useState<null | 'copy' | 'filter' | 'mkdir' | 'goto' | 'menu'>(null)

  const setPane = useCallback((side: 0 | 1, f: (p: PaneMem) => PaneMem) => {
    setPanes(ps => (side === 0 ? [f(ps[0]), ps[1]] : [ps[0], f(ps[1])]))
  }, [])

  // First visit: both panes in the home folder, once the places arrive.
  useEffect(() => {
    if (!homeDir) return
    setPanes(ps => ps.map(p => (p.where.kind === 'local' && !p.path ? { ...p, path: homeDir } : p)) as [PaneMem, PaneMem])
  }, [homeDir])
  // A share deleted in Settings: back home.
  useEffect(() => {
    setPanes(
      ps =>
        ps.map(p => (p.where.kind === 'smb' && !sources.some(s => s.id === (p.where as { id: string }).id) ? { ...start, path: homeDir } : p)) as [
          PaneMem,
          PaneMem,
        ],
    )
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sources])
  useEffect(() => {
    memory.panes = panes
    memory.active = active
  }, [panes, active])

  const keyOf = (p: PaneMem) => (p.where.kind === 'local' ? (p.path ? `l:${p.path}` : null) : `r:${p.where.id}:${p.path}`)
  const loader = (p: PaneMem) => async (): Promise<Listing> => {
    if (p.where.kind === 'local') {
      const r = await api.listLocal(p.path)
      return { entries: r.entries, free: r.free, total: r.total }
    }
    const r = await api.listRemote(p.where.id, p.path)
    return { entries: r.entries }
  }
  const listings = [useListing(keyOf(panes[0]), loader(panes[0])), useListing(keyOf(panes[1]), loader(panes[1]))] as const

  const atRoot = (p: PaneMem) => (p.where.kind === 'local' ? !p.path || p.path === '/' : p.path === '')
  const rows = [
    useMemo(() => rowsFor(listings[0].data?.entries, filters[0], !atRoot(panes[0])), [listings[0].data, filters, panes[0]]),
    useMemo(() => rowsFor(listings[1].data?.entries, filters[1], !atRoot(panes[1])), [listings[1].data, filters, panes[1]]),
  ] as const

  // Clamp cursors when listings change size.
  useEffect(() => {
    setPanes(ps =>
      ps.map((p, i) => (p.cursor >= rows[i].length && rows[i].length ? { ...p, cursor: rows[i].length - 1 } : p)) as [PaneMem, PaneMem],
    )
  }, [rows[0].length, rows[1].length])

  const visibleRows = useRef(10)
  const clearFilter = (side: 0 | 1) => setFilters(f => (side === 0 ? ['', f[1]] : [f[0], '']))
  const clearMarks = () => setMarked(m => ({ side: m.side, names: new Set() }))

  const goTo = (side: 0 | 1, where: Where, path: string, cursor = 0) => {
    setPane(side, () => ({ where, path, cursor }))
    clearFilter(side)
    if (marked.side === side) clearMarks()
  }

  const enter = (side: 0 | 1, row: Row | undefined) => {
    if (!row) return
    if (row.kind === 'up') {
      goUp(side)
      return
    }
    const e = row.entry!
    const p = panes[side]
    if (e.dir) goTo(side, p.where, joinPath(p.path, e.name))
    else toggleMark(side, e.name)
  }

  const goUp = (side: 0 | 1) => {
    const p = panes[side]
    if (atRoot(p)) return false
    const from = p.path.split('/').pop() ?? ''
    const parent = parentPath(p.path)
    const to = p.where.kind === 'smb' && parent === '/' ? '' : parent || '/'
    const key = p.where.kind === 'local' ? `l:${to}` : `r:${p.where.id}:${to}`
    const idx = (cache.get(key)?.entries ?? []).findIndex(e => e.name === from)
    const hasUp = !atRoot({ ...p, path: to })
    goTo(side, p.where, to, idx >= 0 ? idx + (hasUp ? 1 : 0) : 0)
    return true
  }

  const toggleMark = (side: 0 | 1, name: string) =>
    setMarked(m => {
      const names = new Set(m.side === side ? m.names : [])
      if (names.has(name)) names.delete(name)
      else names.add(name)
      return { side, names }
    })

  /** L2/R2 or the header: the previous/next quick location for a pane. */
  const cycle = (side: 0 | 1, d: number) => {
    if (!locs.length) return
    const p = panes[side]
    const cur = locFor(locs, p.where, p.path)
    const i = cur ? locs.indexOf(cur) : -1
    const next = locs[(i + d + locs.length) % locs.length]
    goTo(side, next.where, next.path)
  }

  // Another tab → Explore: that folder open on the left (a file: selected in its folder).
  useEffect(() => {
    if (!request || request.n === memory.handled) return
    memory.handled = request.n
    const target = request.path.replace(/\/+$/, '') || '/'
    setActive(0)
    setHead(h => [false, h[1]])
    clearFilter(0)
    api
      .listLocal(target)
      .then(r => {
        cache.set(`l:${target}`, { entries: r.entries, free: r.free, total: r.total })
        goTo(0, { kind: 'local' }, target)
      })
      .catch(() => {
        // Not a folder: open the one around it with the file selected.
        const parent = parentPath(target) || '/'
        const name = target.split('/').pop() ?? ''
        api
          .listLocal(parent)
          .then(r => {
            cache.set(`l:${parent}`, { entries: r.entries, free: r.free, total: r.total })
            const idx = r.entries.findIndex(e => e.name === name)
            goTo(0, { kind: 'local' }, parent, idx >= 0 ? idx + (parent !== '/' ? 1 : 0) : 0)
            if (idx >= 0) setMarked({ side: 0, names: new Set([name]) })
            else toast(tr('Not found'), target, 'error')
          })
          .catch(e => toast(tr("Couldn't open {path}", { path: parent }), String((e as Error).message), 'error'))
      })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [request])

  const copyItems = (side: 0 | 1) => {
    const entries = listings[side].data?.entries ?? []
    if (marked.side === side && marked.names.size) return entries.filter(e => marked.names.has(e.name))
    const row = rows[side][panes[side].cursor]
    return row?.kind === 'entry' ? [row.entry!] : []
  }
  const other = (side: 0 | 1) => (side === 0 ? 1 : 0) as 0 | 1
  const startCopy = () => {
    const to = panes[other(active)]
    if (to.where.kind !== 'local') {
      toast(tr("Can't copy to network storage"), tr('Open a folder of this device on the other side.'), 'error')
      return
    }
    if (copyItems(active).length) setDialog('copy')
  }

  // ---------- controller ----------
  const handlerRef = useRef<(a: Action) => boolean>(() => false)
  handlerRef.current = (a: Action) => {
    const side = active
    const cur = panes[side]
    const list = rows[side]
    const move = (to: number) => setPane(side, p => ({ ...p, cursor: Math.max(0, Math.min(list.length - 1, to)) }))

    if (a === 'lt' || a === 'rt') {
      cycle(side, a === 'lt' ? -1 : 1)
      return true
    }
    if (head[side]) {
      switch (a) {
        case 'left':
          cycle(side, -1)
          return true
        case 'right':
          cycle(side, 1)
          return true
        case 'down':
        case 'back':
          setHead(h => (side === 0 ? [false, h[1]] : [h[0], false]))
          return true
        case 'confirm':
          setDialog('goto')
          return true
        default:
          return a === 'up'
      }
    }

    switch (a) {
      case 'up':
        if (cur.cursor === 0) setHead(h => (side === 0 ? [true, h[1]] : [h[0], true]))
        else move(cur.cursor - 1)
        return true
      case 'down':
        move(cur.cursor + 1)
        return true
      case 'left':
        setActive(0)
        return true
      case 'right':
        setActive(1)
        return true
      case 'confirm':
        enter(side, list[cur.cursor])
        return true
      case 'back':
        if (filters[side]) {
          clearFilter(side)
          return true
        }
        if (marked.side === side && marked.names.size) {
          clearMarks()
          return true
        }
        return goUp(side)
      case 'x': {
        const row = list[cur.cursor]
        if (row?.kind === 'entry') {
          toggleMark(side, row.entry!.name)
          move(cur.cursor + 1)
        }
        return true
      }
      case 'view': {
        const all = (listings[side].data?.entries ?? []).filter(e => !filters[side] || e.name.toLowerCase().includes(filters[side].toLowerCase()))
        setMarked(m => (m.side === side && m.names.size ? { side, names: new Set() } : { side, names: new Set(all.map(e => e.name)) }))
        return true
      }
      case 'y':
        startCopy()
        return true
      case 'menu':
        setDialog('menu')
        return true
      default:
        return false
    }
  }

  useEffect(() => {
    if (dialog) return
    return input.pushHandler(a => handlerRef.current(a))
  }, [dialog])

  // Right stick: the virtual list scrolls by moving its cursor a row at a time.
  const stickAcc = useRef(0)
  useEffect(() => {
    if (dialog) return
    return input.pushScroll(dy => {
      stickAcc.current += dy
      while (Math.abs(stickAcc.current) >= ROW_H) {
        const step = Math.sign(stickAcc.current)
        stickAcc.current -= step * ROW_H
        handlerRef.current(step > 0 ? 'down' : 'up')
      }
      return true
    })
  }, [dialog])

  const markedHere = marked.side === active ? marked.names.size : 0
  const hints: Hint[] = head[active]
    ? [
        { glyph: 'A', label: tr('Go to…') },
        { glyph: 'DPAD', label: tr('Switch location') },
        { glyph: 'B', label: tr('List') },
      ]
    : [
        { glyph: 'A', label: tr('Open') },
        { glyph: 'X', label: tr('Mark') },
        { glyph: 'Y', label: markedHere ? tr('Copy {n}', { n: markedHere }) : tr('Copy') },
        { glyph: ['L2', 'R2'], label: tr('Location') },
        { glyph: 'MENU', label: tr('Options') },
        { glyph: 'B', label: tr('Back') },
      ]
  useHints(dialog ? null : hints)

  const pane = (side: 0 | 1) => {
    const p = panes[side]
    const l = listings[side]
    const here = locFor(locs, p.where, p.path)
    const src = p.where.kind === 'smb' ? sources.find(s => s.id === (p.where as { id: string }).id) : null
    const name = p.path.split('/').filter(Boolean).pop()
    return (
      <Pane
        side={side}
        active={active === side}
        headFocused={head[side]}
        title={src ? src.name : here && here.path === p.path ? here.label : (name ?? '/')}
        subtitle={src ? `\\\\${src.host}\\${src.share}` : here && here.path !== p.path ? here.label : groupLabel(here?.group)}
        icon={src ? 'network' : (here?.icon ?? 'folder')}
        path={src ? `/${p.path}` : homePath(p.path)}
        local={p.where.kind === 'local'}
        free={l.data?.free}
        total={l.data?.total}
        disk={here?.disk}
        rows={rows[side]}
        cursor={p.cursor}
        marked={marked.side === side ? marked.names : new Set()}
        loading={l.loading && !l.data}
        refreshing={l.loading && !!l.data}
        error={l.error}
        filter={filters[side]}
        status={statusLine(l.data?.entries, marked.side === side ? marked.names : new Set(), filters[side])}
        visibleRows={visibleRows}
        onRowClick={i => {
          setActive(side)
          if (i === p.cursor) enter(side, rows[side][i])
          else setPane(side, q => ({ ...q, cursor: i }))
        }}
        onHeadClick={() => {
          setActive(side)
          setDialog('goto')
        }}
        onRetry={l.reload}
      />
    )
  }

  const to = panes[other(active)]
  const toListing = listings[other(active)]
  const from = panes[active]
  return (
    <div className="explorer">
      {pane(0)}
      {pane(1)}

      {dialog === 'copy' && to.where.kind === 'local' && (
        <CopyDialog
          sourceId={from.where.kind === 'local' ? 'local' : from.where.id}
          remotePath={from.path}
          items={copyItems(active)}
          destPath={to.path}
          destFree={toListing.data?.free}
          destTotal={toListing.data?.total}
          destDisk={locFor(locs, to.where, to.path)?.disk}
          romsRoot={places.find(p => p.icon === 'roms')?.path}
          onClose={() => setDialog(null)}
          onQueued={n => {
            setDialog(null)
            clearMarks()
            toast(trn(n, '{n} item added to the queue', '{n} items added to the queue'), tr('Track it in Transfers'), 'ok')
          }}
        />
      )}
      {dialog === 'goto' && (
        <Dialog title={tr('Go to…')} onClose={() => setDialog(null)} wide>
          <div className="inst-list scroll goto-list">
            {(['place', 'library', 'prefix', 'network'] as const).map(g => {
              const items = locs.filter(l => l.group === g)
              if (!items.length) return null
              const cur = locFor(locs, panes[active].where, panes[active].path)
              return (
                <div key={g} className="goto-group">
                  <h4 className="inst-sub">
                    {g === 'place' ? tr('This device') : g === 'library' ? tr('Steam libraries') : g === 'prefix' ? tr('Game prefixes (C:)') : tr('Network storage')}
                  </h4>
                  {items.map(l =>
                    l.group === 'network' ? (
                      <button
                        key={l.id}
                        data-nav
                        data-nav-default={cur?.id === l.id ? '' : undefined}
                        className="dest-option"
                        onClick={() => {
                          setDialog(null)
                          goTo(active, l.where, l.path)
                        }}
                      >
                        <Icon name="network" size={26} />
                        <div>
                          <b>{l.label}</b>
                          <small>{l.disk}</small>
                        </div>
                      </button>
                    ) : (
                      <StorageCard
                        key={l.id}
                        icon={l.icon}
                        title={l.label}
                        path={l.path}
                        free={l.free}
                        total={l.total}
                        disk={l.disk}
                        selected={cur?.id === l.id}
                        navDefault={cur?.id === l.id}
                        onClick={() => {
                          setDialog(null)
                          goTo(active, l.where, l.path)
                        }}
                      />
                    ),
                  )}
                </div>
              )
            })}
            {!sources.length && (
              <button
                data-nav
                className="dest-option"
                onClick={() => {
                  setDialog(null)
                  onGoSettings()
                }}
              >
                <Icon name="plus" size={26} />
                <div>
                  <b>{tr('Add network storage')}</b>
                  <small>{tr('Settings → Game sources')}</small>
                </div>
              </button>
            )}
          </div>
        </Dialog>
      )}
      {dialog === 'menu' && (
        <Dialog title={tr('Options')} onClose={() => setDialog(null)}>
          <div className="menu-list">
            <button data-nav data-nav-default className="menu-item" onClick={() => setDialog('goto')}>
              {tr('Go to…')}
              <small>{tr('Folders, cards, Steam libraries, game prefixes, network storage')}</small>
            </button>
            <button data-nav className="menu-item" onClick={() => setDialog('filter')}>
              {tr('Search this folder')}
            </button>
            {panes[active].where.kind === 'local' && (
              <button data-nav className="menu-item" onClick={() => setDialog('mkdir')}>
                {tr('New folder')}
              </button>
            )}
            <button
              data-nav
              className="menu-item"
              onClick={() => {
                setDialog(null)
                handlerRef.current('view')
              }}
            >
              {markedHere ? tr('Unmark all') : tr('Mark all')}
            </button>
          </div>
        </Dialog>
      )}
      {dialog === 'filter' && (
        <TextPrompt
          title={tr('Search this folder')}
          placeholder={tr('Part of the name…')}
          initial={filters[active]}
          submitLabel={tr('Filter')}
          onCancel={() => setDialog(null)}
          onSubmit={v => {
            setFilters(f => (active === 0 ? [v, f[1]] : [f[0], v]))
            setPane(active, p => ({ ...p, cursor: 0 }))
            setDialog(null)
          }}
        />
      )}
      {dialog === 'mkdir' && (
        <TextPrompt
          title={tr('New folder in {path}', { path: homePath(panes[active].path) })}
          placeholder={tr('Folder name')}
          submitLabel={tr('Create')}
          validate={v => v.length > 0 && !v.includes('/')}
          onCancel={() => setDialog(null)}
          onSubmit={async v => {
            setDialog(null)
            try {
              await api.mkdir(joinPath(panes[active].path, v))
              listings[active].reload()
              toast(tr('Folder created'), v, 'ok')
            } catch (e) {
              toast(tr("Couldn't create the folder"), String((e as Error).message), 'error')
            }
          }}
        />
      )}
    </div>
  )
}

/** What kind of place a location is, under its name in a pane's header. */
function groupLabel(g?: Loc['group']) {
  switch (g) {
    case 'library':
      return tr('Steam library')
    case 'prefix':
      return tr('Game prefix (C:)')
    case 'network':
      return tr('Network storage')
    default:
      return tr('This device')
  }
}

function rowsFor(entries: Entry[] | undefined, filter: string, withUp: boolean): Row[] {
  const list = entries ?? []
  const f = filter.toLowerCase()
  const shown = f ? list.filter(e => e.name.toLowerCase().includes(f)) : list
  const rows: Row[] = shown.map(entry => ({ kind: 'entry', entry }))
  return withUp && !f ? [{ kind: 'up' }, ...rows] : rows
}

function statusLine(entries: Entry[] | undefined, marked: Set<string>, filter: string) {
  if (!entries) return ''
  const dirs = entries.filter(e => e.dir).length
  const files = entries.length - dirs
  const parts = [trn(dirs, '{n} folder', '{n} folders'), trn(files, '{n} file', '{n} files')]
  if (filter) parts.push(tr('filter “{filter}”', { filter }))
  if (marked.size) {
    const size = entries.filter(e => marked.has(e.name)).reduce((s, e) => s + e.size, 0)
    parts.push(trn(marked.size, '{n} item marked', '{n} items marked') + (size ? ` (${formatBytes(size)}+)` : ''))
  }
  return parts.join(' · ')
}

// ---------- pane ----------

function Pane(props: {
  side: 0 | 1
  active: boolean
  headFocused: boolean
  title: string
  subtitle: string
  icon: string
  path: string
  local: boolean
  free?: number | null
  total?: number | null
  disk?: string
  rows: Row[]
  cursor: number
  marked: Set<string>
  loading: boolean
  refreshing: boolean
  error?: string
  filter: string
  status: string
  visibleRows: React.MutableRefObject<number>
  onRowClick: (i: number) => void
  onHeadClick: () => void
  onRetry: () => void
}) {
  const { rows, cursor, active } = props
  const listRef = useRef<HTMLDivElement>(null)
  const [height, setHeight] = useState(480)
  const [top, setTop] = useState(0)

  useLayoutEffect(() => {
    const el = listRef.current
    if (!el) return
    const ro = new ResizeObserver(() => setHeight(el.clientHeight))
    ro.observe(el)
    setHeight(el.clientHeight)
    return () => ro.disconnect()
  }, [])

  const visible = Math.max(1, Math.floor(height / ROW_H))
  if (active) props.visibleRows.current = visible

  // Keep the cursor inside the viewport with a small margin.
  useEffect(() => {
    setTop(t => {
      const margin = Math.min(2, Math.floor(visible / 3))
      if (cursor < t + margin) return Math.max(0, cursor - margin)
      if (cursor > t + visible - 1 - margin) return Math.min(Math.max(0, rows.length - visible), cursor - visible + 1 + margin)
      return Math.min(t, Math.max(0, rows.length - visible))
    })
  }, [cursor, visible, rows.length])

  const start = Math.max(0, top - 2)
  const end = Math.min(rows.length, top + visible + 2)
  const slice = rows.slice(start, end)

  return (
    <section className={`pane ${active ? 'active' : ''}`}>
      <header className={`pane-head ${props.headFocused && active ? 'focused' : ''}`} onClick={props.onHeadClick}>
        <div className="pane-title">
          <Icon name={props.icon} size={26} />
          <div>
            <b>{props.title}</b>
            <small>{props.subtitle}</small>
          </div>
        </div>
        {props.local ? <StorageStatus free={props.free} total={props.total} disk={props.disk} /> : <span className="pane-cycle">L2 ◀ ▶ R2</span>}
      </header>
      <div className="pane-path" title={props.path}>
        {props.refreshing && <Spinner />}
        {/* Left-to-right marks: the line is right-aligned (long paths keep their
            end), which would otherwise move a leading "~/" or "/" to the end. */}
        <span>{`\u200E${props.path}\u200E`}</span>
      </div>
      <div className="pane-list" ref={listRef}>
        {props.loading && (
          <div className="pane-msg">
            <Spinner /> {tr('Loading…')}
          </div>
        )}
        {props.error && !props.loading && (
          <div className="pane-msg error">
            <b>{tr("Couldn't list this folder")}</b>
            <span>{props.error}</span>
            <button className="btn small" onClick={props.onRetry}>
              {tr('Try again')}
            </button>
          </div>
        )}
        {!props.loading && !props.error && rows.length === 0 && (
          <div className="pane-msg">{props.filter ? tr('Nothing matches this filter') : tr('Empty folder')}</div>
        )}
        <div className="rows" style={{ height: rows.length * ROW_H, transform: `translateY(${-top * ROW_H}px)` }}>
          {slice.map((row, k) => {
            const i = start + k
            const isCursor = i === cursor
            const e = row.entry
            const isMarked = !!e && props.marked.has(e.name)
            return (
              <div
                key={row.kind === 'up' ? '..' : e!.name}
                className={`frow ${isCursor ? (active && !props.headFocused ? 'cursor' : 'cursor-dim') : ''} ${isMarked ? 'marked' : ''}`}
                style={{ top: i * ROW_H }}
                onClick={() => props.onRowClick(i)}
              >
                {row.kind === 'up' ? (
                  <>
                    <Icon name="up" />
                    <span className="frow-name">..</span>
                    <span className="frow-meta">{tr('Back')}</span>
                  </>
                ) : (
                  <>
                    <span className={`check ${isMarked ? 'on' : ''}`}>{isMarked && <Icon name="check" size={16} />}</span>
                    <Icon name={e!.dir ? 'folder' : 'file'} />
                    <span className="frow-name">{e!.name}</span>
                    <span className="frow-meta">{e!.dir ? '' : formatBytes(e!.size)}</span>
                  </>
                )}
              </div>
            )
          })}
        </div>
      </div>
      <div className="pane-status">{props.status}</div>
    </section>
  )
}

// ---------- copy confirmation ----------

function CopyDialog({
  sourceId,
  remotePath,
  items,
  destPath,
  destFree,
  destTotal,
  destDisk,
  romsRoot,
  onClose,
  onQueued,
}: {
  /** A share's id, or "local" for this device (absolute paths). */
  sourceId: string
  remotePath: string
  items: Entry[]
  destPath: string
  destFree?: number | null
  destTotal?: number | null
  destDisk?: string
  romsRoot?: string
  onClose: () => void
  onQueued: (n: number) => void
}) {
  const [busy, setBusy] = useState(false)
  const [romsDirs, setRomsDirs] = useState<string[] | null>(null)

  useEffect(() => {
    if (!romsRoot) return
    api
      .listLocal(romsRoot)
      .then(r => setRomsDirs(r.entries.filter(e => e.dir).map(e => e.name)))
      .catch(() => setRomsDirs([]))
  }, [romsRoot])

  const system = guessSystem(remotePath) ?? (items.length === 1 ? guessSystem(items[0].name) : null)
  const suggested =
    romsRoot && system && romsDirs?.includes(system) && destPath.startsWith(romsRoot) && destPath !== `${romsRoot}/${system}`
      ? `${romsRoot}/${system}`
      : null

  // The ROM folder list arrives after the dialog opened: move focus to the
  // suggestion (or the plain option) once the choices are final.
  const ready = !romsRoot || romsDirs !== null
  useEffect(() => {
    if (ready) requestAnimationFrame(focusFirst)
  }, [ready, suggested])

  const knownSize = items.reduce((s, e) => s + e.size, 0)
  const hasDirs = items.some(e => e.dir)
  const tight = destFree != null && knownSize > 0 && destFree < knownSize

  const queue = useCallback(
    async (dest: string) => {
      setBusy(true)
      try {
        await api.enqueue(
          sourceId,
          dest,
          items.map(e => ({ path: joinPath(remotePath, e.name), name: e.name, dir: e.dir })),
        )
        onQueued(items.length)
      } catch (e) {
        setBusy(false)
        toast(tr("Couldn't add to the queue"), String((e as Error).message), 'error')
      }
    },
    [items, onQueued, remotePath, sourceId],
  )

  return (
    <Dialog
      title={items.length === 1 ? tr('Copy “{name}”', { name: items[0].name }) : tr('Copy {n} items', { n: items.length })}
      onClose={onClose}
      wide
    >
      <div className="copy-summary">
        <ul className="copy-items">
          {items.slice(0, 5).map(e => (
            <li key={e.name}>
              <Icon name={e.dir ? 'folder' : 'file'} size={18} />
              <span>{e.name}</span>
              <small>{e.dir ? tr('folder') : formatBytes(e.size)}</small>
            </li>
          ))}
          {items.length > 5 && <li className="muted">{tr('+ {n} more', { n: items.length - 5 })}</li>}
        </ul>
        <div className="copy-facts">
          <span>
            {tr('Size:')} <b>{knownSize ? formatBytes(knownSize) : '—'}</b>
            {hasDirs && ` ${tr('+ folder contents')}`}
          </span>
        </div>
      </div>
      <div className="dest-options">
        {!ready && (
          <div className="muted">
            <Spinner /> {tr('Looking for the emulator folder…')}
          </div>
        )}
        {ready && suggested && (
          <StorageCard
            icon="roms"
            title={tr('Copy to roms/{system}', { system: system ?? '' })}
            path={suggested}
            note={tr('Suggested — emulator folder detected')}
            free={destFree}
            total={destTotal}
            disk={destDisk}
            disabled={busy}
            navDefault
            onClick={() => void queue(suggested)}
          />
        )}
        {ready && (
          <StorageCard
            icon="folder"
            title={tr('Copy to the folder open on the other side')}
            path={destPath}
            warn={tight ? tr('Not enough free space there.') : null}
            free={destFree}
            total={destTotal}
            disk={destDisk}
            disabled={busy}
            navDefault={!suggested}
            onClick={() => void queue(destPath)}
          />
        )}
      </div>
      <div className="dialog-actions">
        <button data-nav className="btn" onClick={onClose}>
          {tr('Cancel')}
        </button>
      </div>
    </Dialog>
  )
}
