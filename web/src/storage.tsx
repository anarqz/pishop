// Storage status, shown wherever piShop talks about a place on disk: a card
// with the place and a small ring of how full its disk is, the free space
// and the total next to it.

import { useEffect, useState } from 'react'
import { type Space, api, formatBytes } from './api'
import { tr } from './i18n'
import { Icon } from './ui'

export const homePath = (p: string) => p.replace(/^\/home\/[^/]+/, '~')

/** How full a disk is: the used part as a ring, amber then red as it fills. */
export function StorageRing({ free, total }: { free?: number | null; total?: number | null }) {
  const known = !!total && free != null
  const used = known ? Math.min(1, Math.max(0, 1 - (free as number) / (total as number))) : 0
  const r = 15
  const c = 2 * Math.PI * r
  // Warn on what's left, not just the percentage: 13% of a 2 TB disk is plenty.
  const GB = 1024 ** 3
  const left = known ? (free as number) : Infinity
  const level = left < 5 * GB || used >= 0.98 ? 'full' : left < 25 * GB || used >= 0.93 ? 'low' : ''
  return (
    <span className={`sring ${level}`} aria-hidden>
      <svg viewBox="0 0 40 40">
        <circle className="sring-track" cx="20" cy="20" r={r} />
        {known && <circle className="sring-bar" cx="20" cy="20" r={r} strokeDasharray={c} strokeDashoffset={c * (1 - used)} />}
      </svg>
      <small>{known ? `${Math.round(used * 100)}%` : '?'}</small>
    </span>
  )
}

/** Free space with the total and the disk's name under it, and the ring. */
export function StorageStatus({ free, total, disk }: { free?: number | null; total?: number | null; disk?: string | null }) {
  return (
    <span className="sstatus">
      <span className="sstatus-text">
        <b>{free != null ? tr('{free} free', { free: formatBytes(free) }) : tr('Free space unknown')}</b>
        <small>{[total ? tr('of {total}', { total: formatBytes(total) }) : null, disk].filter(Boolean).join(' · ')}</small>
      </span>
      <StorageRing free={free} total={total} />
    </span>
  )
}

/**
 * A place on disk: icon, name, path (or a note) and its disk's storage
 * status on the right. A focusable button when `onClick` is given.
 */
export function StorageCard({
  icon = 'folder',
  title,
  path,
  note,
  free,
  total,
  disk,
  onClick,
  navDefault,
  disabled,
  warn,
  selected,
}: {
  icon?: string
  title: string
  path?: string | null
  note?: string | null
  free?: number | null
  total?: number | null
  disk?: string | null
  onClick?: () => void
  navDefault?: boolean
  disabled?: boolean
  /** Shown in red under the title (not enough space, blocked…). */
  warn?: string | null
  selected?: boolean
}) {
  const body = (
    <>
      <Icon name={selected ? 'check' : icon} size={26} />
      <span className="scard-main">
        <b>{title}</b>
        {path && <small className="mono">{homePath(path)}</small>}
        {note && <small>{note}</small>}
        {warn && <small className="scard-warn">{warn}</small>}
      </span>
      <StorageStatus free={free} total={total} disk={disk} />
    </>
  )
  if (!onClick) return <div className={`scard ${selected ? 'selected' : ''}`}>{body}</div>
  return (
    <button
      data-nav
      data-nav-default={navDefault ? '' : undefined}
      className={`scard ${selected ? 'selected' : ''}`}
      disabled={disabled}
      onClick={onClick}
    >
      {body}
    </button>
  )
}

/** Free/total of the disk a path is on, refetched when the path changes. */
export function useSpace(path: string | null | undefined) {
  const [space, setSpace] = useState<Space | null>(null)
  useEffect(() => {
    if (!path) {
      setSpace(null)
      return
    }
    let alive = true
    api
      .space(path)
      .then(s => alive && setSpace(s))
      .catch(() => alive && setSpace(null))
    return () => {
      alive = false
    }
  }, [path])
  return space
}
