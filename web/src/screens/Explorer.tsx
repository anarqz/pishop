// Two-pane, FTP-style explorer: network source on the left, Deck storage on
// the right. Driven entirely by the controller (custom list navigation, so
// folders with tens of thousands of entries stay instant via virtualization).

import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react'
import { type Entry, type Place, type Source, api, formatBytes, joinPath, parentPath } from '../api'
import { type Action, focusFirst, input } from '../input'
import { guessSystem } from '../systems'
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
const memory: { panes: [PaneMem, PaneMem]; active: 0 | 1 } = {
  panes: [
    { selected: 0, path: '', cursor: 0 },
    { selected: 0, path: '', cursor: 0 },
  ],
  active: 0,
}

interface Row {
  kind: 'up' | 'entry'
  entry?: Entry
}

export default function Explorer({ sources, places, onGoSettings }: {
  sources: Source[]
  places: Place[]
  onGoSettings: () => void
}) {
  const [active, setActive] = useState<0 | 1>(memory.active)
  const [head, setHead] = useState<[boolean, boolean]>([false, false])
  const [left, setLeft] = useState<PaneMem>(memory.panes[0])
  const [right, setRight] = useState<PaneMem>(memory.panes[1])
  const [filters, setFilters] = useState<[string, string]>(['', ''])
  const [marked, setMarked] = useState<Set<string>>(new Set())
  const [dialog, setDialog] = useState<null | 'copy' | 'filter' | 'mkdir'>(null)

  const source = sources[Math.min(left.selected, sources.length - 1)]
  const place = places[Math.min(right.selected, places.length - 1)]

  // First visit / changed selection: start at the source's base folder / place root.
  useEffect(() => {
    if (source && !left.path && source.base_path) setLeft(p => ({ ...p, path: source.base_path }))
  }, [source?.id])
  useEffect(() => {
    if (place && (!right.path || !right.path.startsWith(place.path))) setRight(p => ({ ...p, path: place.path, cursor: 0 }))
  }, [place?.id])

  useEffect(() => {
    memory.panes = [left, right]
    memory.active = active
  }, [left, right, active])

  const remote = useListing(source ? `r:${source.id}:${left.path}` : null, async () => {
    const r = await api.listRemote(source!.id, left.path)
    return { entries: r.entries }
  })
  const local = useListing(place ? `l:${right.path}` : null, async () => {
    const r = await api.listLocal(right.path)
    return { entries: r.entries, free: r.free }
  })

  const leftRows = useMemo(
    () => rowsFor(remote.data?.entries, filters[0], left.path !== ''),
    [remote.data, filters, left.path],
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
      if (!left.path) return false
      const from = left.path.split('/').pop() ?? ''
      const parent = parentPath(left.path)
      const rows = cache.get(`r:${source?.id}:${parent}`)?.entries ?? []
      const idx = rows.findIndex(e => e.name === from)
      setLeft({ ...left, path: parent === '/' ? '' : parent, cursor: idx >= 0 ? idx + (parent ? 1 : 0) : 0 })
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
    if (side === 0 && sources.length) {
      const next = (left.selected + d + sources.length) % sources.length
      setLeft({ selected: next, path: sources[next].base_path, cursor: 0 })
      setMarked(new Set())
    } else if (side === 1 && places.length) {
      const next = (right.selected + d + places.length) % places.length
      setRight({ selected: next, path: places[next].path, cursor: 0 })
    }
    setFilters(f => (side === 0 ? ['', f[1]] : [f[0], '']))
  }

  const copyItems = () => {
    if (marked.size) {
      return remote.data?.entries.filter(e => marked.has(e.name)) ?? []
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
          cycle(side, 1)
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
          const all = (remote.data?.entries ?? []).filter(e => !filters[0] || e.name.toLowerCase().includes(filters[0].toLowerCase()))
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
    ? [
        { glyph: 'DPAD', label: active === 0 ? 'Trocar fonte' : 'Trocar local' },
        { glyph: 'B', label: 'Lista' },
      ]
    : active === 0
      ? [
          { glyph: 'A', label: 'Abrir' },
          { glyph: 'X', label: 'Marcar' },
          { glyph: 'Y', label: marked.size ? `Copiar ${marked.size}` : 'Copiar' },
          { glyph: 'MENU', label: 'Buscar' },
          { glyph: 'B', label: 'Voltar' },
        ]
      : [
          { glyph: 'A', label: 'Abrir' },
          { glyph: 'Y', label: 'Nova pasta' },
          { glyph: 'MENU', label: 'Buscar' },
          { glyph: 'B', label: 'Voltar' },
        ]
  useHints(dialog ? null : hints)

  if (!sources.length) {
    return (
      <div className="empty-state" data-nav-scope>
        <Icon name="network" size={56} />
        <h2>Nenhuma fonte configurada</h2>
        <p>Adicione um armazenamento de rede (SMB) para navegar pelos seus jogos.</p>
        <button data-nav data-nav-default className="btn primary" onClick={onGoSettings}>
          Adicionar armazenamento de rede
        </button>
      </div>
    )
  }

  const freeRight = local.data?.free ?? place?.free ?? 0

  return (
    <div className="explorer">
      <Pane
        side={0}
        active={active === 0}
        headFocused={head[0]}
        kind="ORIGEM"
        title={source?.name ?? ''}
        subtitle={`\\\\${source?.host}\\${source?.share}`}
        icon="network"
        canCycle={sources.length > 1}
        path={`/${left.path}`}
        rows={leftRows}
        cursor={left.cursor}
        marked={marked}
        loading={remote.loading && !remote.data}
        refreshing={remote.loading && !!remote.data}
        error={remote.error}
        filter={filters[0]}
        status={statusLine(remote.data?.entries, marked, filters[0])}
        visibleRows={visibleRows}
        onRowClick={i => {
          setActive(0)
          if (i === left.cursor) enter(0, leftRows[i])
          else setLeft({ ...left, cursor: i })
        }}
        onHeadClick={() => cycle(0, 1)}
        onRetry={remote.reload}
      />
      <Pane
        side={1}
        active={active === 1}
        headFocused={head[1]}
        kind="DESTINO"
        title={place?.label ?? ''}
        subtitle={place ? `${formatBytes(freeRight)} livres` : ''}
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

      {dialog === 'copy' && source && place && (
        <CopyDialog
          source={source}
          remotePath={left.path}
          items={copyItems()}
          destPath={right.path}
          destFree={freeRight}
          romsRoot={places.find(p => p.icon === 'roms')?.path}
          onClose={() => setDialog(null)}
          onQueued={n => {
            setDialog(null)
            setMarked(new Set())
            toast(`${n} ${n === 1 ? 'item adicionado' : 'itens adicionados'} à fila`, 'Acompanhe em Transferências (R1)', 'ok')
          }}
        />
      )}
      {dialog === 'filter' && (
        <TextPrompt
          title={active === 0 ? 'Buscar na origem' : 'Buscar no destino'}
          placeholder="Parte do nome…"
          initial={filters[active]}
          submitLabel="Filtrar"
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
          title={`Nova pasta em ${right.path}`}
          placeholder="Nome da pasta"
          submitLabel="Criar"
          validate={v => v.length > 0 && !v.includes('/')}
          onCancel={() => setDialog(null)}
          onSubmit={async v => {
            setDialog(null)
            try {
              await api.mkdir(joinPath(right.path, v))
              local.reload()
              toast('Pasta criada', v, 'ok')
            } catch (e) {
              toast('Não foi possível criar a pasta', String((e as Error).message), 'error')
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
  const parts = [`${dirs} pastas`, `${files} arquivos`]
  if (filter) parts.push(`filtro “${filter}”`)
  if (marked.size) {
    const size = entries.filter(e => marked.has(e.name)).reduce((s, e) => s + e.size, 0)
    parts.push(`${marked.size} marcados${size ? ` (${formatBytes(size)}+)` : ''}`)
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
            <Spinner /> Carregando…
          </div>
        )}
        {props.error && !props.loading && (
          <div className="pane-msg error">
            <b>Não foi possível listar</b>
            <span>{props.error}</span>
            <button className="btn small" onClick={props.onRetry}>
              Tentar de novo
            </button>
          </div>
        )}
        {!props.loading && !props.error && rows.length === 0 && (
          <div className="pane-msg">{props.filter ? 'Nada encontrado com esse filtro' : 'Pasta vazia'}</div>
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
                    <span className="frow-meta">Voltar</span>
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
  source,
  remotePath,
  items,
  destPath,
  destFree,
  romsRoot,
  onClose,
  onQueued,
}: {
  source: Source
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
          source.id,
          dest,
          items.map(e => ({ path: joinPath(remotePath, e.name), name: e.name, dir: e.dir })),
        )
        onQueued(items.length)
      } catch (e) {
        setBusy(false)
        toast('Não foi possível adicionar à fila', String((e as Error).message), 'error')
      }
    },
    [items, onQueued, remotePath, source.id],
  )

  return (
    <Dialog title={items.length === 1 ? `Copiar “${items[0].name}”` : `Copiar ${items.length} itens`} onClose={onClose} wide>
      <div className="copy-summary">
        <ul className="copy-items">
          {items.slice(0, 5).map(e => (
            <li key={e.name}>
              <Icon name={e.dir ? 'folder' : 'file'} size={18} />
              <span>{e.name}</span>
              <small>{e.dir ? 'pasta' : formatBytes(e.size)}</small>
            </li>
          ))}
          {items.length > 5 && <li className="muted">+ {items.length - 5} itens</li>}
        </ul>
        <div className="copy-facts">
          <span>
            Tamanho: <b>{knownSize ? formatBytes(knownSize) : '—'}</b>
            {hasDirs && ' + conteúdo das pastas'}
          </span>
          <span>
            Livre no destino: <b>{formatBytes(destFree)}</b>
          </span>
        </div>
      </div>
      <div className="dest-options">
        {!ready && (
          <div className="muted">
            <Spinner /> Procurando a pasta do emulador…
          </div>
        )}
        {ready && suggested && (
          <button data-nav data-nav-default className="dest-option suggested" disabled={busy} onClick={() => queue(suggested)}>
            <Icon name="roms" size={26} />
            <div>
              <b>Copiar para roms/{system}</b>
              <small>Sugerido — pasta do emulador detectada</small>
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
            <b>Copiar para a pasta aberta no destino</b>
            <small>{destPath}</small>
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
