// Transfers as one list, after Steam's Downloads page: a row per game (its
// art behind it, the cover, progress and live numbers) and the focused one in
// the banner on top, with its download speed over the last minutes. Torrents,
// community installs and links added by hand look alike here; each row's
// kicker says which it is.

import { useState } from 'react'
import { type CommunityInstall, type DirectDownload, type LibraryEntry, formatBytes, formatEta, imgUrl } from '../api'
import { baseName, needsFile, stageLabel } from '../community'
import { tr, trn } from '../i18n'
import { type Sample, WINDOW_MS, speedKey, useSpeedHistory } from '../speeds'
import type { TorrentInfo } from '../torrent'
import { Icon, Progress } from '../ui'
import { type DlState, dlState, hue, seeding } from './Torrents'
import { directState, installState } from './TransferTiles'

type Fact = [string, string]

/** One transfer, whatever its kind, as the list and the banner show it. */
export interface TransferView {
  /** Speed history key; also the row's key. */
  key: string
  kind: 'torrent' | 'community' | 'direct'
  id: number | string
  state: DlState
  /** "Torrent", "Community", "Direct download". */
  source: string
  /** "Downloading", "Paused"… */
  status: string
  name: string
  cover?: string | null
  hero?: string | null
  /** Shown instead of a cover when there's no art (a file added by link). */
  icon?: string
  platform?: string | null
  /** Under the name: the live numbers, or what's going on. */
  line: string
  sub: string
  /** 0–1; null when there's nothing to measure. */
  progress: number | null
  /** Right side, under the percentage: time left or size. */
  aside: string
  chips: string[]
  byline?: string
  facts: Fact[]
  error?: string | null
  /** Unix seconds: newest first among transfers in the same state. */
  added: number
}

const RANK: Record<DlState, number> = { active: 0, checking: 1, paused: 2, error: 3, done: 4 }

/** Downloading first, then preparing, paused, failed, finished; newest first in each. */
export const sortViews = (vs: TransferView[]) => [...vs].sort((a, b) => RANK[a.state] - RANK[b.state] || b.added - a.added)

const clamp = (f: number) => Math.max(0, Math.min(1, f))
const pct = (f: number) => `${Math.floor(clamp(f) * 100)}%`
const ofTotal = (done: number, total: number) => tr('{done} of {total}', { done: formatBytes(done), total: formatBytes(total) })
const home = (p: string) => p.replace(/^\/home\/[^/]+/, '~')

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

// ---------- the three kinds ----------

export function torrentView(t: TorrentInfo, entry?: LibraryEntry): TransferView {
  const g = entry?.game
  const name = g?.name ?? t.name
  const state = dlState(t)
  const done = t.progress * t.totalBytes
  const eta = state === 'active' && t.downloadSpeed > 0 ? formatEta((t.totalBytes - done) / t.downloadSpeed) : ''
  const lines = ((): [string, string] => {
    switch (state) {
      case 'active':
        return [`↓ ${formatBytes(t.downloadSpeed)}/s · ${trn(t.peers, '{n} peer', '{n} peers')}`, ofTotal(done, t.totalBytes)]
      case 'checking':
        return [tr('Preparing…'), t.totalBytes ? formatBytes(t.totalBytes) : tr('Fetching the torrent')]
      case 'paused':
        return [tr('Paused'), ofTotal(done, t.totalBytes)]
      case 'error':
        return [tr('Download error'), t.error ?? '']
      case 'done':
        return [
          seeding(t) ? `${tr('Complete')} · ↑ ${formatBytes(t.uploadSpeed)}/s` : tr('Complete'),
          `${formatBytes(t.totalBytes)} · ${trn(t.files.length, '{n} file', '{n} files')}`,
        ]
    }
  })()
  const installed = entry?.install?.stage === 'installed'
  return {
    key: speedKey.torrent(t.infoHash),
    kind: 'torrent',
    id: t.id,
    state,
    source: tr('Torrent'),
    status: statusLabel(state) + (state === 'done' ? ` · ${seeding(t) ? tr('sharing') : tr('stopped')}` : ''),
    name,
    cover: g?.cover,
    hero: g?.hero,
    platform: g?.platform,
    line: installed ? tr('In Games') : lines[0],
    sub: lines[1],
    progress: t.totalBytes ? t.progress : null,
    aside: eta ? tr('{eta} left', { eta }) : t.totalBytes ? formatBytes(t.totalBytes) : '',
    chips: [g?.year, g?.platform, ...(g?.genres ?? []).slice(0, 2), g?.scene_group, g?.drm && `DRM ${g.drm}`].filter(Boolean).map(String),
    byline: [g?.developers?.[0], g?.publishers?.[0] !== g?.developers?.[0] ? g?.publishers?.[0] : null].filter(Boolean).join(' · ') || undefined,
    facts: [
      [tr('Downloaded'), ofTotal(done, t.totalBytes)],
      ...(eta ? ([[tr('Time left'), eta]] as Fact[]) : []),
      ...(seeding(t) || t.uploadSpeed > 0 ? ([[tr('Upload speed'), `${formatBytes(t.uploadSpeed)}/s`]] as Fact[]) : []),
      [tr('Peers'), String(t.peers)],
    ],
    error: state === 'error' ? t.error : null,
    added: entry?.added ?? 0,
  }
}

export function installView(i: CommunityInstall): TransferView {
  const state = installState(i)
  const downloading = i.stage === 'downloading'
  const eta = downloading && i.speed > 0 && !i.paused ? formatEta((i.total - i.done) / i.speed) : ''
  const lines = ((): [string, string] => {
    switch (i.stage) {
      case 'downloading': {
        const waiting = i.parts.find(p => p.state === 'waiting')
        if (waiting) return [tr('Waiting for your file'), waiting.name]
        const checking = i.parts.find(p => p.state === 'verifying')
        if (checking) return [tr('Checking the files'), `${checking.name} · ${pct(checking.verify ?? 0)}`]
        if (i.paused) return [tr('Paused'), ofTotal(i.done, i.total)]
        return [`↓ ${formatBytes(i.speed)}/s`, ofTotal(i.done, i.total)]
      }
      case 'extracting':
        return [tr('Extracting'), pct(i.progress ?? 0)]
      case 'preparing':
        return [i.step ?? tr('Preparing the game'), i.progress != null ? pct(i.progress) : tr('Install script')]
      case 'shortcut':
        return [tr('Adding to Steam'), i.platform]
      case 'installed':
        return [tr('In Games'), formatBytes(i.total)]
      case 'failed':
        return [tr('Needs attention'), i.error ?? '']
    }
  })()
  return {
    key: speedKey.community(i.id),
    kind: 'community',
    id: i.id,
    state,
    source: tr('Community'),
    status: needsFile(i) ? tr('Your file') : stageLabel(i),
    name: i.name,
    cover: i.game?.cover,
    hero: i.game?.hero,
    platform: i.platform,
    line: lines[0],
    sub: lines[1],
    progress: downloading ? (i.total ? i.done / i.total : 0) : (i.progress ?? (i.stage === 'installed' ? 1 : null)),
    aside: eta ? tr('{eta} left', { eta }) : i.total ? formatBytes(i.total) : '',
    chips: [i.platform, i.wrapper ? tr('via {name}', { name: i.wrapper.name }) : null, trn(i.parts.length, '{n} part', '{n} parts')].filter(
      (c): c is string => !!c,
    ),
    facts: [
      [tr('Step'), stageLabel(i)],
      ...(downloading ? ([[tr('Downloaded'), ofTotal(i.done, i.total)]] as Fact[]) : []),
      ...(eta ? ([[tr('Time left'), eta]] as Fact[]) : []),
      ...(i.step && !downloading ? ([[tr('Now'), i.step]] as Fact[]) : []),
    ],
    error: i.stage === 'failed' ? i.error : null,
    added: i.added,
  }
}

export function directView(d: DirectDownload): TransferView {
  const state = directState(d)
  const eta = d.state === 'running' && d.speed > 0 && d.total ? formatEta((d.total - d.done) / d.speed) : ''
  const of = d.total ? ofTotal(d.done, d.total) : formatBytes(d.done)
  const lines: [string, string] =
    d.state === 'running'
      ? [`↓ ${formatBytes(d.speed)}/s`, of]
      : d.state === 'paused'
        ? [tr('Paused'), of]
        : d.state === 'failed'
          ? [tr('Download error'), d.error ?? '']
          : [tr('Complete'), formatBytes(d.total)]
  return {
    key: speedKey.direct(d.id),
    kind: 'direct',
    id: d.id,
    state,
    source: tr('Direct download'),
    status: statusLabel(state),
    name: baseName(d.path),
    icon: 'file',
    line: lines[0],
    sub: lines[1],
    progress: d.total ? d.done / d.total : null,
    aside: eta ? tr('{eta} left', { eta }) : d.total ? formatBytes(d.total) : '',
    chips: [],
    byline: home(d.path),
    facts: [[tr('Downloaded'), of], ...(eta ? ([[tr('Time left'), eta]] as Fact[]) : [])],
    error: d.state === 'failed' ? d.error : null,
    added: d.added,
  }
}

// ---------- the list ----------

const navAttr = (v: TransferView) =>
  v.kind === 'torrent' ? { 'data-torrent-id': v.id } : v.kind === 'community' ? { 'data-community-id': v.id } : { 'data-direct-id': v.id }

const barState = (s: DlState) => (s === 'done' ? 'done' : s === 'error' ? 'error' : s === 'paused' ? 'paused' : undefined)

function RowArt({ v }: { v: TransferView }) {
  const [failed, setFailed] = useState(false)
  return v.cover && !failed ? (
    <img src={imgUrl(v.cover)} alt="" draggable={false} onError={() => setFailed(true)} />
  ) : (
    <div className="tb-ph">
      <Icon name={v.icon ?? 'pad'} size={26} />
    </div>
  )
}

export function TransferRow({ v, first, onOpen }: { v: TransferView; first?: boolean; onOpen: () => void }) {
  return (
    <div
      data-nav
      data-nav-default={first ? '' : undefined}
      data-transfer-key={v.key}
      {...navAttr(v)}
      tabIndex={0}
      className={`tb-row ${v.state}`}
      style={{ ['--h' as string]: hue(v.name) }}
      onClick={onOpen}
    >
      {v.hero && <div className="tb-row-bg" style={{ backgroundImage: `url(${imgUrl(v.hero)})` }} />}
      <div className="tb-row-cover">
        <RowArt v={v} />
      </div>
      <div className="tb-row-main">
        <span className={`tb-kicker ${v.state}`}>
          {v.source} · {v.status}
          {v.platform ? ` · ${v.platform}` : ''}
        </span>
        <b className="tb-row-name">{v.name}</b>
        <span className="tb-row-lines">
          <span className={`dl-line ${v.state}`}>{v.line}</span>
          {v.sub && <span className="dl-line sub">{v.sub}</span>}
        </span>
        {v.progress != null && <Progress value={v.progress} state={barState(v.state)} />}
      </div>
      <div className="tb-row-aside">
        {v.progress != null && <b className={v.state}>{pct(v.progress)}</b>}
        <span>{v.aside}</span>
      </div>
    </div>
  )
}

// ---------- the banner ----------

export function TransferBanner({ v }: { v: TransferView }) {
  const samples = useSpeedHistory(v.key)
  const graph = samples.length >= 2 || v.state === 'active'
  return (
    <section className={`tb-banner ${graph ? 'with-graph' : ''}`} style={{ ['--h' as string]: hue(v.name) }}>
      {v.hero && <div className="tb-banner-bg" style={{ backgroundImage: `url(${imgUrl(v.hero)})` }} />}
      <div className="tb-banner-shade" />
      <div className="tb-banner-main">
        <span className={`dl-kicker ${v.state}`}>
          {v.source} · {v.status}
        </span>
        <h2>{v.name}</h2>
        {v.byline && <span className="dl-byline">{v.byline}</span>}
        {v.chips.length > 0 && (
          <div className="dl-chips">
            {v.chips.map(c => (
              <span key={c}>{c}</span>
            ))}
          </div>
        )}
        {v.error ? (
          <p className="dl-hero-error">{v.error}</p>
        ) : (
          <dl className="dl-facts">
            {v.facts.map(([k, val]) => (
              <div key={k}>
                <dt>{k}</dt>
                <dd>{val}</dd>
              </div>
            ))}
          </dl>
        )}
        {v.progress != null && (
          <div className="tb-banner-progress">
            <Progress value={v.progress} state={barState(v.state)} />
            <b>{pct(v.progress)}</b>
          </div>
        )}
      </div>
      {graph && <SpeedGraph samples={samples} />}
    </section>
  )
}

const GRAPH_W = 480
const GRAPH_H = 120
/** Samples further apart than this: piShop wasn't polling (a game in front); a gap. */
const GAP_MS = 15_000

/** The last minutes of download speed, like Steam's: current, peak and average. */
function SpeedGraph({ samples }: { samples: Sample[] }) {
  if (samples.length < 2) {
    return (
      <div className="tb-graph">
        <div className="tb-graph-head">
          <span>{tr('Download speed')}</span>
        </div>
        <div className="tb-graph-wait">{tr('Measuring the speed…')}</div>
      </div>
    )
  }
  const now = samples[samples.length - 1][0]
  // Grows with the history up to the window, so a new download isn't a sliver.
  const span = Math.min(WINDOW_MS, Math.max(60_000, now - samples[0][0]))
  const from = now - span
  const shown = samples.filter(s => s[0] >= from)
  const peak = Math.max(...shown.map(s => s[1]))
  const avg = shown.reduce((sum, s) => sum + s[1], 0) / shown.length
  const top = Math.max(peak, 64 * 1024) * 1.15
  const x = (t: number) => (((t - from) / span) * GRAPH_W).toFixed(1)
  const y = (b: number) => (GRAPH_H - (b / top) * GRAPH_H).toFixed(1)
  const runs: Sample[][] = []
  for (const s of shown) {
    const run = runs[runs.length - 1]
    if (!run || s[0] - run[run.length - 1][0] > GAP_MS) runs.push([s])
    else run.push(s)
  }
  const line = runs.map(r => r.map((s, i) => `${i ? 'L' : 'M'}${x(s[0])},${y(s[1])}`).join('')).join('')
  const area = runs.map(r => `M${x(r[0][0])},${GRAPH_H}${r.map(s => `L${x(s[0])},${y(s[1])}`).join('')}L${x(r[r.length - 1][0])},${GRAPH_H}Z`).join('')
  const minutes = Math.round(span / 60_000)
  return (
    <div className="tb-graph">
      <div className="tb-graph-head">
        <span>{tr('Download speed')}</span>
        <span>{trn(minutes, 'last minute', 'last {n} min')}</span>
      </div>
      <svg viewBox={`0 0 ${GRAPH_W} ${GRAPH_H}`} preserveAspectRatio="none" aria-hidden>
        <defs>
          <linearGradient id="tb-fill" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor="#1a9fff" stopOpacity=".55" />
            <stop offset="100%" stopColor="#1a9fff" stopOpacity="0" />
          </linearGradient>
        </defs>
        <line className="tb-graph-peak" x1="0" x2={GRAPH_W} y1={y(peak)} y2={y(peak)} />
        <path d={area} fill="url(#tb-fill)" />
        <path d={line} className="tb-graph-line" />
      </svg>
      <dl className="tb-graph-stats">
        <div>
          <dt>{tr('Current')}</dt>
          <dd className="now">{formatBytes(samples[samples.length - 1][1])}/s</dd>
        </div>
        <div>
          <dt>{tr('Peak')}</dt>
          <dd>{formatBytes(peak)}/s</dd>
        </div>
        <div>
          <dt>{tr('Average')}</dt>
          <dd>{formatBytes(avg)}/s</dd>
        </div>
      </dl>
    </div>
  )
}
