// Two-pane, FTP-style explorer: a network share or this device's own storage
// on the left, the device's places on the right. Driven entirely by the
// controller (custom list navigation, so folders with tens of thousands of
// entries stay instant via virtualization). Transfers can open a finished
// download here (request), already selected and ready to copy anywhere.

import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { type Entry, type Place, type Source, api, formatBytes, joinPath, parentPath } from '../api'
import { type Action, focusFirst, input } from '../input'
import { guessSystem } from '../systems'
import { tr, trn } from '../i18n'
import { Dialog, type Hint, Icon, Spinner, TextPrompt, toast, useHints } from '../ui'

const ROW_H = 46

// ---------- listing cache (stale-while-revalidate) ----------

interface Listing {
  entries: Entry[]
  free?: number | null
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

// ---------- remembered state across tab switches ----------

interface PaneMem {
  selected: number
  path: string
  cursor: number
}
const memory: { panes: [PaneMem, PaneMem]; active: 0 | 1; handled: number } = {
  panes: [
    { selected: 0, path: '', cursor: 0 },
    { selected: 0, path: '', cursor: 0 },
  ],
  active: 0,
  /** Last request already applied (so coming back to the tab doesn't redo it). */
  handled: 0,
}

/** Left pane: a network share, or this device (absolute paths, after the shares). */
type LeftSource = { kind: 'smb'; source: Source } | { kind: 'local' }

/** Opens a local file or folder in the left pane, selected and ready to copy. */
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
  const [active, setActive] = useState<0 | 1>(memory.active)
  const [head, setHead] = useState<[boolean, boolean]>([false, false])
  const [left, setLeft] = useState<PaneMem>(memory.panes[0])
  const [right, setRight] = useState<PaneMem>(memory.panes[1])
  const [filters, setFilters] = useState<[string, string]>(['', ''])
  const [marked, setMarked] = useState<Set<string>>(new Set())
  const [dialog, setDialog] = useState<null | 'copy' | 'filter' | 'mkdir'>(null)

  const leftSources = useMemo<LeftSource[]>(
    () => [...sources.map(source => ({ kind: 'smb' as const, source })), { kind: 'local' as const }],
    [sources],
  )
  const leftSrc = leftSources[Math.min(left.selected, leftSources.length - 1)]
  const isLocal = leftSrc.kind === 'local'
  const source = leftSrc.kind === 'smb' ? leftSrc.source : null
  const place = places[Math.min(right.selected, places.length - 1)]
  // Where "This device" starts: Downloads, where finished torrents land.
  const localStart = places.find(p => p.id === 'downloads')?.path ?? places.find(p => p.id === 'home')?.path ?? '/'

  // First visit / changed selection: start at the source's base folder / place root.
  useEffect(() => {
    if (source && !left.path && source.base_path) setLeft(p => ({ ...p, path: source.base_path }))
    if (isLocal && !left.path && places.length) setLeft(p => ({ ...p, path: localStart }))
  }, [source?.id, isLocal, places.length])
  useEffect(() => {
    if (place && (!right.path || !right.path.startsWith(place.path))) setRight(p => ({ ...p, path: place.path, cursor: 0 }))
  }, [place?.id])

  useEffect(() => {
    memory.panes = [left, right]
    memory.active = active
  }, [left, right, active])

  const srcList = useListing(isLocal ? (left.path ? `l:${left.path}` : null) : source ? `r:${source.id}:${left.path}` : null, async () => {
    if (isLocal) {
      const r = await api.listLocal(left.path)
      return { entries: r.entries, free: r.free }
    }
    const r = await api.listRemote(source!.id, left.path)
    return { entries: r.entries }
  })
  const local = useListing(place ? `l:${right.path}` : null, async () => {
    const r = await api.listLocal(right.path)
    return { entries: r.entries, free: r.free }
  })

  const leftRows = useMemo(
    () => rowsFor(srcList.data?.entries, filters[0], isLocal ? !!left.path && left.path !== '/' : left.path !== ''),
    [srcList.data, filters, left.path, isLocal],
  )
  const rightRows = useMemo(
    () => rowsFor(local.data?.entries, filters[1], !!place && right.path !== place.path),
    [local.data, filters, right.path, place?.path],
  )

  // Clamp cursors when listings change size.
  useEffect(() => {
    setLeft(p => (p.cursor >= leftRows.length && leftRows.length ? { ...p, cursor: leftRows.length - 1 } : p))
  }, [leftRows.length])
  useEffect(() => {
    setRight(p => (p.cursor >= rightRows.length && rightRows.length ? { ...p, cursor: rightRows.length - 1 } : p))
  }, [rightRows.length])

  const visibleRows = useRef(10)

  const enter = (side: 0 | 1, row: Row | undefined) => {
    if (!row) return
    const set = side === 0 ? setLeft : setRight
    const cur = side === 0 ? left : right
    if (row.kind === 'up') {
      goUp(side)
      return
    }
    const e = row.entry!
    if (e.dir) {
      set({ ...cur, path: joinPath(cur.path, e.name), cursor: 0 })
      setFilters(f => (side === 0 ? ['', f[1]] : [f[0], '']))
      if (side === 0) setMarked(new Set())
    } else if (side === 0) {
      toggleMark(e.name)
    }
  }

  const goUp = (side: 0 | 1) => {
    if (side === 0) {
      if (!left.path || (isLocal && left.path === '/')) return false
      const from = left.path.split('/').pop() ?? ''
      const parent = parentPath(left.path)
      if (isLocal) {
        const rows = cache.get(`l:${parent}`)?.entries ?? []
        const idx = rows.findIndex(e => e.name === from)
        setLeft({ ...left, path: parent, cursor: idx >= 0 ? idx + (parent !== '/' ? 1 : 0) : 0 })
      } else {
        const rows = cache.get(`r:${source?.id}:${parent}`)?.entries ?? []
        const idx = rows.findIndex(e => e.name === from)
        setLeft({ ...left, path: parent === '/' ? '' : parent, cursor: idx >= 0 ? idx + (parent ? 1 : 0) : 0 })
      }
      setMarked(new Set())
      setFilters(f => ['', f[1]])
      return true
    }
    if (!place || right.path === place.path) return false
    const from = right.path.split('/').pop() ?? ''
    const parent = parentPath(right.path)
    const rows = cache.get(`l:${parent}`)?.entries ?? []
    const idx = rows.findIndex(e => e.name === from)
    setRight({ ...right, path: parent, cursor: idx >= 0 ? idx + (parent !== place.path ? 1 : 0) : 0 })
    setFilters(f => [f[0], ''])
    return true
  }

  const toggleMark = (name: string) =>
    setMarked(m => {
      const n = new Set(m)
      if (n.has(name)) n.delete(name)
      else n.add(name)
      return n
    })

  const cycle = (side: 0 | 1, d: number) => {
    if (side === 0 && leftSources.length > 1) {
      const next = (left.selected + d + leftSources.length) % leftSources.length
      const to = leftSources[next]
      setLeft({ selected: next, path: to.kind === 'smb' ? to.source.base_path : localStart, cursor: 0 })
      setMarked(new Set())
    } else if (side === 1 && places.length) {
      const next = (right.selected + d + places.length) % places.length
      setRight({ selected: next, path: places[next].path, cursor: 0 })
    }
    setFilters(f => (side === 0 ? ['', f[1]] : [f[0], '']))
  }

  // Transfers → Explore: the download is selected in its folder; Y copies it.
  useEffect(() => {
    if (!request || request.n === memory.handled) return
    memory.handled = request.n
    const target = request.path.replace(/\/+$/, '') || '/'
    const parent = parentPath(target) || '/'
    const name = target.split('/').pop() ?? ''
    setActive(0)
    setHead(h => [false, h[1]])
    setFilters(f => ['', f[1]])
    api
      .listLocal(parent)
      .then(r => {
        cache.set(`l:${parent}`, { entries: r.entries, free: r.free })
        const idx = r.entries.findIndex(e => e.name === name)
        setLeft({ selected: sources.length, path: parent, cursor: idx >= 0 ? idx + (parent !== '/' ? 1 : 0) : 0 })
        setMarked(idx >= 0 ? new Set([name]) : new Set())
        if (idx < 0) toast(tr('Not found'), target, 'error')
      })
      .catch(e => toast(tr("Couldn't open {path}", { path: parent }), String((e as Error).message), 'error'))
  }, [request, sources.length])

  const copyItems = () => {
    if (marked.size) {
      return srcList.data?.entries.filter(e => marked.has(e.name)) ?? []
    }
    const row = leftRows[left.cursor]
    return row?.kind === 'entry' ? [row.entry!] : []
  }

  // ---------- controller ----------
  const handlerRef = useRef<(a: Action) => boolean>(() => false)
  handlerRef.current = (a: Action) => {
    const side = active
    const cur = side === 0 ? left : right
    const set = side === 0 ? setLeft : setRight
    const rows = side === 0 ? leftRows : rightRows
    const page = Math.max(1, visibleRows.current - 1)
    const move = (to: number) => set({ ...cur, cursor: Math.max(0, Math.min(rows.length - 1, to)) })

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
          // Only "This device" on the left: A sets up a network share instead.
          if (side === 0 && !sources.length) onGoSettings()
          else cycle(side, 1)
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
      case 'lt':
        move(cur.cursor - page)
        return true
      case 'rt':
        move(cur.cursor + page)
        return true
      case 'left':
        setActive(0)
        return true
      case 'right':
        setActive(1)
        return true
      case 'confirm':
        enter(side, rows[cur.cursor])
        return true
      case 'back':
        if (filters[side]) {
          setFilters(f => (side === 0 ? ['', f[1]] : [f[0], '']))
          return true
        }
        if (side === 0 && marked.size) {
          setMarked(new Set())
          return true
        }
        return goUp(side)
      case 'x':
        if (side === 0) {
          const row = rows[cur.cursor]
          if (row?.kind === 'entry') {
            toggleMark(row.entry!.name)
            move(cur.cursor + 1)
          }
        }
        return true
      case 'view':
        if (side === 0) {
          const all = (srcList.data?.entries ?? []).filter(e => !filters[0] || e.name.toLowerCase().includes(filters[0].toLowerCase()))
          setMarked(marked.size ? new Set() : new Set(all.map(e => e.name)))
        }
        return true
      case 'y':
        if (side === 0) {
          if (copyItems().length) setDialog('copy')
        } else if (place) {
          setDialog('mkdir')
        }
        return true
      case 'menu':
        setDialog('filter')
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

  const hints: Hint[] = head[active]
    ? active === 0 && !sources.length
      ? [
          { glyph: 'A', label: tr('Add network storage') },
          { glyph: 'B', label: tr('List') },
        ]
      : [
          { glyph: 'DPAD', label: active === 0 ? tr('Switch source') : tr('Switch location') },
          { glyph: 'B', label: tr('List') },
        ]
    : active === 0
      ? [
          { glyph: 'A', label: tr('Open') },
          { glyph: 'X', label: tr('Mark') },
          { glyph: 'Y', label: marked.size ? tr('Copy {n}', { n: marked.size }) : tr('Copy') },
          { glyph: 'MENU', label: tr('Search') },
          { glyph: 'B', label: tr('Back') },
        ]
      : [
          { glyph: 'A', label: tr('Open') },
          { glyph: 'Y', label: tr('New folder') },
          { glyph: 'MENU', label: tr('Search') },
          { glyph: 'B', label: tr('Back') },
        ]
  useHints(dialog ? null : hints)

  const freeRight = local.data?.free ?? place?.free ?? 0

  return (
    <div className="explorer">
      <Pane
        side={0}
        active={active === 0}
        headFocused={head[0]}
        kind={tr('FROM')}
        title={isLocal ? tr('This device') : (source?.name ?? '')}
        subtitle={isLocal ? tr('Files on this device') : `\\\\${source?.host}\\${source?.share}`}
        icon={isLocal ? 'home' : 'network'}
        canCycle={leftSources.length > 1}
        path={isLocal ? left.path : `/${left.path}`}
        rows={leftRows}
        cursor={left.cursor}
        marked={marked}
        loading={srcList.loading && !srcList.data}
        refreshing={srcList.loading && !!srcList.data}
        error={srcList.error}
        filter={filters[0]}
        status={statusLine(srcList.data?.entries, marked, filters[0])}
        visibleRows={visibleRows}
        onRowClick={i => {
          setActive(0)
          if (i === left.cursor) enter(0, leftRows[i])
          else setLeft({ ...left, cursor: i })
        }}
        onHeadClick={() => (sources.length ? cycle(0, 1) : onGoSettings())}
        onRetry={srcList.reload}
      />
      <Pane
        side={1}
        active={active === 1}
        headFocused={head[1]}
        kind={tr('TO')}
        title={place?.label ?? ''}
        subtitle={place ? tr('{size} free', { size: formatBytes(freeRight) }) : ''}
        icon={place?.icon ?? 'folder'}
        canCycle={places.length > 1}
        path={right.path}
        rows={rightRows}
        cursor={right.cursor}
        marked={new Set()}
        loading={local.loading && !local.data}
        refreshing={local.loading && !!local.data}
        error={local.error}
        filter={filters[1]}
        status={statusLine(local.data?.entries, new Set(), filters[1])}
        visibleRows={visibleRows}
        onRowClick={i => {
          setActive(1)
          if (i === right.cursor) enter(1, rightRows[i])
          else setRight({ ...right, cursor: i })
        }}
        onHeadClick={() => cycle(1, 1)}
        onRetry={local.reload}
      />

      {dialog === 'copy' && (source || isLocal) && place && (
        <CopyDialog
          sourceId={isLocal ? 'local' : source!.id}
          remotePath={left.path}
          items={copyItems()}
          destPath={right.path}
          destFree={freeRight}
          romsRoot={places.find(p => p.icon === 'roms')?.path}
          onClose={() => setDialog(null)}
          onQueued={n => {
            setDialog(null)
            setMarked(new Set())
            toast(trn(n, '{n} item added to the queue', '{n} items added to the queue'), tr('Track it in Transfers (R1)'), 'ok')
          }}
        />
      )}
      {dialog === 'filter' && (
        <TextPrompt
          title={active === 0 ? tr('Search the source') : tr('Search the destination')}
          placeholder={tr('Part of the name…')}
          initial={filters[active]}
          submitLabel={tr('Filter')}
          onCancel={() => setDialog(null)}
          onSubmit={v => {
            setFilters(f => (active === 0 ? [v, f[1]] : [f[0], v]))
            if (active === 0) setLeft(p => ({ ...p, cursor: 0 }))
            else setRight(p => ({ ...p, cursor: 0 }))
            setDialog(null)
          }}
        />
      )}
      {dialog === 'mkdir' && (
        <TextPrompt
          title={tr('New folder in {path}', { path: right.path })}
          placeholder={tr('Folder name')}
          submitLabel={tr('Create')}
          validate={v => v.length > 0 && !v.includes('/')}
          onCancel={() => setDialog(null)}
          onSubmit={async v => {
            setDialog(null)
            try {
              await api.mkdir(joinPath(right.path, v))
              local.reload()
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
  kind: string
  title: string
  subtitle: string
  icon: string
  canCycle: boolean
  path: string
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
      <header
        className={`pane-head ${props.headFocused && active ? 'focused' : ''}`}
        onClick={props.onHeadClick}
      >
        <span className="pane-kind">{props.kind}</span>
        <div className="pane-title">
          <Icon name={props.icon} size={26} />
          <div>
            <b>{props.title}</b>
            <small>{props.subtitle}</small>
          </div>
        </div>
        {props.canCycle && <span className="pane-cycle">◀ ▶</span>}
      </header>
      <div className="pane-path" title={props.path}>
        {props.refreshing && <Spinner />}
        <span>{props.path}</span>
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
                    {props.side === 0 && (
                      <span className={`check ${isMarked ? 'on' : ''}`}>{isMarked && <Icon name="check" size={16} />}</span>
                    )}
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
  romsRoot,
  onClose,
  onQueued,
}: {
  /** A share's id, or "local" for this device (absolute paths). */
  sourceId: string
  remotePath: string
  items: Entry[]
  destPath: string
  destFree: number
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
          <span>
            {tr('Free at destination:')} <b>{formatBytes(destFree)}</b>
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
          <button data-nav data-nav-default className="dest-option suggested" disabled={busy} onClick={() => queue(suggested)}>
            <Icon name="roms" size={26} />
            <div>
              <b>{tr('Copy to roms/{system}', { system: system ?? '' })}</b>
              <small>{tr('Suggested — emulator folder detected')}</small>
            </div>
          </button>
        )}
        {ready && (
        <button
          data-nav
          data-nav-default={suggested ? undefined : ''}
          className="dest-option"
          disabled={busy}
          onClick={() => queue(destPath)}
        >
          <Icon name="folder" size={26} />
          <div>
            <b>{tr('Copy to the folder open on the destination')}</b>
            <small>{destPath}</small>
          </div>
        </button>
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
