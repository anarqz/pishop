// Torrent downloads as a shelf of game covers: the game the Store matched,
// a progress ring over the cover and the live numbers under it. The focused
// download fills the panel on top with the game's art and details.

import { useEffect, useState } from 'react'
import { type LibraryEntry, api, formatEta, imgUrl } from '../api'
import { tr, trn } from '../i18n'
import { type TorrentInfo, destroy, forget, formatBytes, useEngineState } from '../torrent'
import { Dialog, Icon, toast } from '../ui'

export { useEngineState }

/**
 * Store metadata for the torrents on screen: refetched when the set changes,
 * and every few seconds while a new download's game data is still resolving.
 */
export function useLibrary(torrents: TorrentInfo[]) {
  const [lib, setLib] = useState<Record<string, LibraryEntry>>({})
  const key = torrents
    .map(t => t.infoHash)
    .sort()
    .join(',')
  const pending = Object.values(lib).some(e => !e.resolved)
  useEffect(() => {
    const load = () =>
      void api
        .library()
        .then(setLib)
        .catch(() => {})
    load()
    if (!pending) return
    const id = setInterval(load, 2000)
    return () => clearInterval(id)
  }, [key, pending])
  return lib
}

export type DlState = 'active' | 'checking' | 'paused' | 'done' | 'error'

export function dlState(t: TorrentInfo): DlState {
  if (t.error) return 'error'
  if (t.finished) return 'done'
  if (t.state === 'paused') return 'paused'
  if (t.state === 'initializing' || !t.totalBytes) return 'checking'
  return 'active'
}

/** Finished and still uploading to the swarm. */
export const seeding = (t: TorrentInfo) => t.finished && !t.error && t.state === 'live'

/** X on a download: pause while downloading, stop sharing once finished, resume otherwise. */
export function toggleLabel(t: TorrentInfo) {
  if (t.error) return tr('Try again')
  if (t.state === 'paused') return tr('Resume')
  return t.finished ? tr('Stop') : tr('Pause')
}

const ORDER: Record<DlState, number> = { active: 0, checking: 1, paused: 2, error: 3, done: 4 }

/** Downloading first, then paused/failed, then finished; newest first within each. */
export function sortDownloads(ts: TorrentInfo[], lib: Record<string, LibraryEntry>) {
  return [...ts].sort(
    (a, b) =>
      ORDER[dlState(a)] - ORDER[dlState(b)] ||
      (lib[b.infoHash]?.added ?? 0) - (lib[a.infoHash]?.added ?? 0) ||
      b.id - a.id,
  )
}

function hue(s: string) {
  let h = 0
  for (const c of s) h = (h * 31 + c.charCodeAt(0)) % 360
  return h
}

const pctOf = (t: TorrentInfo) => Math.floor(Math.max(0, Math.min(1, t.progress)) * 100)

/** Circular progress with the percentage inside; spins while downloading. */
function Ring({ t, state }: { t: TorrentInfo; state: DlState }) {
  const r = 40
  const c = 2 * Math.PI * r
  const pct = pctOf(t)
  return (
    <div className={`ring ${state}`}>
      <svg viewBox="0 0 100 100" aria-hidden>
        <circle className="ring-track" cx="50" cy="50" r={r} />
        <circle
          className="ring-bar"
          cx="50"
          cy="50"
          r={r}
          strokeDasharray={c}
          strokeDashoffset={state === 'checking' && !pct ? c * 0.8 : c * (1 - pct / 100)}
        />
        {(state === 'active' || state === 'checking') && <circle className="ring-spin" cx="50" cy="50" r="47" />}
      </svg>
      <span className="ring-label">
        {state === 'done' ? (
          <Icon name="check" size={20} />
        ) : state === 'error' ? (
          '!'
        ) : state === 'checking' && !pct ? (
          '…'
        ) : (
          <>
            {pct}
            <small>%</small>
          </>
        )}
      </span>
      {state === 'paused' && <span className="ring-note">{tr('Paused')}</span>}
    </div>
  )
}

function Cover({ name, cover }: { name: string; cover?: string | null }) {
  const [failed, setFailed] = useState(false)
  return cover && !failed ? (
    <img src={imgUrl(cover)} alt="" draggable={false} onError={() => setFailed(true)} />
  ) : (
    <div className="dl-cover-ph">
      <Icon name="pad" size={38} />
      <b>{name}</b>
    </div>
  )
}

/** "{done} of {total}" for a torrent's bytes. */
const doneOfTotal = (t: TorrentInfo) =>
  tr('{done} of {total}', { done: formatBytes(t.progress * t.totalBytes), total: formatBytes(t.totalBytes) })

/** The two short lines under a cover. */
function tileLines(t: TorrentInfo, state: DlState): [string, string] {
  switch (state) {
    case 'active': {
      const done = t.progress * t.totalBytes
      const eta = t.downloadSpeed > 0 ? formatEta((t.totalBytes - done) / t.downloadSpeed) : ''
      return [`↓ ${formatBytes(t.downloadSpeed)}/s${eta ? ` · ${eta}` : ''}`, doneOfTotal(t)]
    }
    case 'checking':
      return [tr('Preparing…'), t.totalBytes ? formatBytes(t.totalBytes) : tr('Fetching the torrent')]
    case 'paused':
      return [tr('Paused'), doneOfTotal(t)]
    case 'error':
      return [tr('Download error'), t.error ?? '']
    case 'done':
      return [
        seeding(t) ? `${tr('Complete')} · ↑ ${formatBytes(t.uploadSpeed)}/s` : tr('Complete'),
        `${formatBytes(t.totalBytes)} · ${trn(t.files.length, '{n} file', '{n} files')}`,
      ]
  }
}

export function DownloadTile({
  t,
  entry,
  first,
  onOpen,
}: {
  t: TorrentInfo
  entry?: LibraryEntry
  first?: boolean
  onOpen: () => void
}) {
  const name = entry?.game.name ?? t.name
  const state = dlState(t)
  const installed = entry?.install?.stage === 'installed'
  const [line1, line2] = installed ? [tr('Installed'), tileLines(t, state)[1]] : tileLines(t, state)
  return (
    <div
      data-nav
      data-nav-default={first ? '' : undefined}
      data-torrent-id={t.id}
      tabIndex={0}
      className={`dl-tile ${state}`}
      onClick={onOpen}
    >
      <div className="dl-cover" style={{ ['--h' as string]: hue(name) }}>
        <Cover name={name} cover={entry?.game.cover} />
        <Ring t={t} state={state} />
        {entry?.game.platform && <span className="plat">{entry.game.platform}</span>}
      </div>
      <div className="dl-info">
        <b className="dl-name">{name}</b>
        <span className={`dl-line ${state}`}>{line1}</span>
        <span className="dl-line sub">{line2}</span>
      </div>
    </div>
  )
}

/** Kicker of the panel on top (translated at render time). */
function statusLabel(state: DlState) {
  switch (state) {
    case 'active':
      return tr('Downloading')
    case 'checking':
      return tr('Preparing')
    case 'paused':
      return tr('Paused')
    case 'done':
      return tr('Complete')
    case 'error':
      return tr('Error')
  }
}

/** Panel on top: the focused download with the game's art and details. */
export function DownloadHero({ t, entry }: { t: TorrentInfo; entry?: LibraryEntry }) {
  const g = entry?.game
  const name = g?.name ?? t.name
  const state = dlState(t)
  const done = t.progress * t.totalBytes
  const eta = state === 'active' && t.downloadSpeed > 0 ? formatEta((t.totalBytes - done) / t.downloadSpeed) : null
  const chips = [g?.year, g?.platform, ...(g?.genres ?? []).slice(0, 2), g?.scene_group, g?.drm && `DRM ${g.drm}`].filter(Boolean)
  const byline = [g?.developers?.[0], g?.publishers?.[0] !== g?.developers?.[0] ? g?.publishers?.[0] : null].filter(Boolean).join(' · ')
  const facts: Array<[string, string]> = [
    [tr('Progress'), `${pctOf(t)}%`],
    [tr('Downloaded'), doneOfTotal(t)],
    ...(state === 'active' ? ([[tr('Download speed'), `${formatBytes(t.downloadSpeed)}/s`]] as Array<[string, string]>) : []),
    ...(eta ? ([[tr('Time left'), eta]] as Array<[string, string]>) : []),
    ...(seeding(t) || t.uploadSpeed > 0 ? ([[tr('Upload speed'), `${formatBytes(t.uploadSpeed)}/s`]] as Array<[string, string]>) : []),
    [tr('Peers'), String(t.peers)],
  ]
  return (
    <section className="dl-hero" key={t.id}>
      {g?.hero && <div className="dl-hero-bg" style={{ backgroundImage: `url(${imgUrl(g.hero)})` }} />}
      <div className="dl-hero-shade" />
      <div className="dl-hero-content">
        <span className={`dl-kicker ${state}`}>
          {statusLabel(state)}
          {state === 'done' ? ` · ${seeding(t) ? tr('sharing') : tr('stopped')}` : ''}
        </span>
        <h2>{name}</h2>
        {byline && <span className="dl-byline">{byline}</span>}
        {chips.length > 0 && (
          <div className="dl-chips">
            {chips.map(c => (
              <span key={String(c)}>{c}</span>
            ))}
          </div>
        )}
        {state === 'error' ? (
          <p className="dl-hero-error">{t.error}</p>
        ) : (
          <dl className="dl-facts">
            {facts.map(([k, v]) => (
              <div key={k}>
                <dt>{k}</dt>
                <dd>{v}</dd>
              </div>
            ))}
          </dl>
        )}
      </div>
    </section>
  )
}

/** Y: remove the download, keeping the files or deleting them too. */
export function DeleteDialog({ t, entry, onClose }: { t: TorrentInfo; entry?: LibraryEntry; onClose: () => void }) {
  const name = entry?.game.name ?? t.name
  const run = (p: Promise<unknown>, done: string) => {
    onClose()
    p.then(() => api.forgetLibrary(t.infoHash).catch(() => {}))
      .then(() => toast(done, name, 'ok'))
      .catch(e => toast(tr("Couldn't delete the transfer"), String((e as Error).message), 'error'))
  }
  return (
    <Dialog title={tr('Delete {name}?', { name })} onClose={onClose}>
      <div className="menu-list">
        <button data-nav data-nav-default className="menu-item" onClick={() => run(forget(t.id), tr('Transfer deleted'))}>
          {tr('Delete the transfer')}
          <small>
            {t.finished
              ? tr('Removes it from the list; the downloaded files stay on the device')
              : tr('Stops the download and removes it from the list; what was downloaded stays on the device')}
          </small>
        </button>
        <button data-nav className="menu-item danger" onClick={() => run(destroy(t.id), tr('Transfer and data deleted'))}>
          {tr('Also delete the data')}
          <small>{tr('Erases the {size} already downloaded', { size: formatBytes(t.progress * t.totalBytes) })}</small>
        </button>
        <button data-nav className="menu-item" onClick={onClose}>
          {tr('Cancel')}
        </button>
      </div>
    </Dialog>
  )
}
