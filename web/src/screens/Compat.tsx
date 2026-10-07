// Proton compatibility: ProtonDB's verdict plus recent player reports from
// Valve's Proton issue tracker, readable with the controller. ProtonDB's own
// page can be opened inside the panel (it scrolls by touch).

import { useEffect, useRef, useState } from 'react'
import { locale, tr, trn } from '../i18n'
import { focusFirst, input } from '../input'
import { Icon, Spinner, useHints } from '../ui'

export interface ProtonSummary {
  tier: string
  bestReportedTier?: string | null
  trendingTier?: string | null
  confidence?: string | null
  score?: number | null
  total?: number | null
}

export interface ProtonReports {
  issue: {
    number: number
    title: string
    url: string
    state: string
    comments: number
    updated_at: string
    labels: string[]
  } | null
  comments: Array<{ author: string; created_at: string; body: string; url: string }>
  others: Array<{ number: number; title: string; url: string; state: string }>
  appid?: string | null
  /** Saved reports, shown because GitHub refused or failed just now. */
  stale: boolean
}

async function get<T>(path: string): Promise<T> {
  const r = await fetch(path)
  const body = await r.json().catch(() => null)
  if (!r.ok) throw new Error(body?.error ?? `HTTP ${r.status}`)
  return body as T
}

/** appids arrive as numbers or strings. */
type AppId = string | number | null | undefined
const idOf = (appid: AppId) => (appid == null || appid === '' ? null : String(appid))

const query = (appid: AppId, name: string) => new URLSearchParams({ appid: idOf(appid) ?? '', name }).toString()

// One request per game per session: the badge and the panel share it.
const summaries = new Map<string, Promise<ProtonSummary | null>>()
function summaryFor(appid: AppId, name: string) {
  const key = idOf(appid) ?? name.toLowerCase()
  let p = summaries.get(key)
  if (!p) {
    p = get<ProtonSummary | null>(`/api/compat/protondb?${query(appid, name)}`)
    p.catch(() => summaries.delete(key))
    summaries.set(key, p)
  }
  return p
}

const TIER_COLOR: Record<string, string> = {
  platinum: '#b4c7dc',
  gold: '#cfb53b',
  silver: '#a6a6a6',
  bronze: '#cd7f32',
  borked: '#ff0000',
}
const tierColor = (t?: string | null) => TIER_COLOR[(t ?? '').toLowerCase()] ?? '#67707b'

function tierName(t?: string | null) {
  switch ((t ?? '').toLowerCase()) {
    case 'platinum':
      return tr('Platinum')
    case 'gold':
      return tr('Gold')
    case 'silver':
      return tr('Silver')
    case 'bronze':
      return tr('Bronze')
    case 'borked':
      return tr('Borked')
    case 'pending':
      return tr('Pending')
    default:
      return t ?? '—'
  }
}

function confidenceName(c?: string | null) {
  switch ((c ?? '').toLowerCase()) {
    case 'strong':
      return tr('Strong')
    case 'good':
      return tr('Good')
    case 'moderate':
      return tr('Moderate')
    case 'weak':
      return tr('Weak')
    case 'inadequate':
      return tr('Inadequate')
    default:
      return c ?? '—'
  }
}

/** "3 days ago" / "há 3 dias", in the UI language. */
function ago(iso: string) {
  const ms = new Date(iso).getTime()
  if (isNaN(ms)) return ''
  const s = (ms - Date.now()) / 1000
  const fmt = new Intl.RelativeTimeFormat(locale(), { numeric: 'auto' })
  const steps: Array<[Intl.RelativeTimeFormatUnit, number]> = [
    ['year', 31_536_000],
    ['month', 2_592_000],
    ['week', 604_800],
    ['day', 86_400],
    ['hour', 3_600],
    ['minute', 60],
  ]
  for (const [unit, secs] of steps) if (Math.abs(s) >= secs) return fmt.format(Math.round(s / secs), unit)
  return fmt.format(0, 'minute')
}

/**
 * ProtonDB tier chip ("● Gold · 2731 reports"). `inline` drops the chip box,
 * for use inside a button.
 */
export function ProtonBadge({ appid, name = '', inline }: { appid?: AppId; name?: string; inline?: boolean }) {
  const [s, setS] = useState<ProtonSummary | null | undefined>(undefined)
  const [failed, setFailed] = useState(false)
  const known = idOf(appid) != null || name.trim() !== ''
  useEffect(() => {
    if (!known) return
    let alive = true
    setS(undefined)
    setFailed(false)
    summaryFor(appid, name)
      .then(v => alive && setS(v))
      .catch(() => alive && setFailed(true))
    return () => {
      alive = false
    }
  }, [appid, name, known])
  if (failed || !known) return null
  const cls = `pdb-badge ${inline ? 'inline' : ''}`
  if (s === undefined)
    return (
      <span className={`${cls} loading`}>
        <span className="pdb-dot" />…
      </span>
    )
  if (s === null) return <span className={`${cls} none`}>{tr('No reports')}</span>
  return (
    <span className={cls} style={{ ['--tier' as string]: tierColor(s.tier) }}>
      <span className="pdb-dot" />
      {tierName(s.tier)}
      {!inline && s.total != null && <small>{trn(s.total, '{n} report', '{n} reports')}</small>}
    </span>
  )
}

/** Full-screen panel: ProtonDB summary, then player reports; B closes. */
export function CompatPanel({ appid, name, onClose }: { appid?: AppId; name: string; onClose: () => void }) {
  const [view, setView] = useState<'reports' | 'web'>('reports')
  const [summary, setSummary] = useState<ProtonSummary | null | undefined>(undefined)
  const [summaryError, setSummaryError] = useState<string | null>(null)
  const [reports, setReports] = useState<{ state: 'loading' | 'ok' | 'error'; r?: ProtonReports; msg?: string }>({ state: 'loading' })

  useEffect(() => {
    let alive = true
    summaryFor(appid, name)
      .then(v => alive && setSummary(v))
      .catch(e => alive && setSummaryError((e as Error).message))
    get<ProtonReports>(`/api/compat/issues?${query(appid, name)}`)
      .then(r => alive && setReports({ state: 'ok', r }))
      .catch(e => alive && setReports({ state: 'error', msg: (e as Error).message }))
    return () => {
      alive = false
    }
  }, [appid, name])

  const ctl = useRef({ view, setView, onClose })
  ctl.current = { view, setView, onClose }
  useEffect(
    () =>
      input.pushModal(() => {
        const c = ctl.current
        if (c.view === 'web') c.setView('reports')
        else c.onClose()
      }),
    [],
  )
  useEffect(() => {
    requestAnimationFrame(focusFirst)
  }, [view])
  useHints(
    view === 'web'
      ? [{ glyph: 'B', label: tr('Back to reports') }]
      : [
          { glyph: 'A', label: tr('Select') },
          { glyph: 'B', label: tr('Back') },
        ],
  )

  const resolved = idOf(appid) ?? reports.r?.appid ?? null
  const pdbUrl = resolved
    ? `https://www.protondb.com/app/${resolved}`
    : `https://www.protondb.com/search?q=${encodeURIComponent(name)}`
  const r = reports.r

  return (
    <div className="compat" data-nav-scope>
      <header className="compat-head">
        <div className="compat-title">
          <span className="hero-kicker">{tr('Proton compatibility')}</span>
          <h2>{name}</h2>
        </div>
        <div className="row">
          <button
            data-nav
            data-nav-default={view === 'reports' ? '' : undefined}
            className={`btn ${view === 'reports' ? 'primary' : ''}`}
            onClick={() => setView('reports')}
          >
            {tr('Player reports')}
          </button>
          <button
            data-nav
            data-nav-default={view === 'web' ? '' : undefined}
            className={`btn ${view === 'web' ? 'primary' : ''}`}
            onClick={() => setView('web')}
          >
            <Icon name="globe" size={18} /> {tr('Open ProtonDB')}
          </button>
        </div>
      </header>

      {view === 'web' ? (
        <div className="compat-web">
          <p className="compat-note">{tr('ProtonDB’s own page: scroll it by touch. B goes back to the reports.')}</p>
          <div className="compat-frame">
            <iframe src={pdbUrl} title="ProtonDB" tabIndex={-1} referrerPolicy="no-referrer" />
          </div>
        </div>
      ) : (
        <div className="compat-scroll">
          <section className="compat-summary" style={{ ['--tier' as string]: tierColor(summary?.tier) }}>
            {summary === undefined && !summaryError && (
              <div className="compat-msg">
                <Spinner /> {tr('Checking ProtonDB…')}
              </div>
            )}
            {summaryError && <div className="compat-msg error">{summaryError}</div>}
            {summary === null && (
              <div className="compat-msg">
                {resolved ? tr('ProtonDB has no reports for this game yet.') : tr("This game wasn't found on Steam, so ProtonDB can't rate it.")}
              </div>
            )}
            {summary && (
              <>
                <div className="compat-tier">
                  <span className="hero-kicker">ProtonDB</span>
                  <b>{tierName(summary.tier)}</b>
                </div>
                <dl className="compat-facts">
                  {summary.total != null && (
                    <div>
                      <dt>{tr('Reports')}</dt>
                      <dd>{summary.total.toLocaleString(locale())}</dd>
                    </div>
                  )}
                  {summary.trendingTier && (
                    <div>
                      <dt>{tr('Trending')}</dt>
                      <dd style={{ color: tierColor(summary.trendingTier) }}>{tierName(summary.trendingTier)}</dd>
                    </div>
                  )}
                  {summary.bestReportedTier && (
                    <div>
                      <dt>{tr('Best reported')}</dt>
                      <dd style={{ color: tierColor(summary.bestReportedTier) }}>{tierName(summary.bestReportedTier)}</dd>
                    </div>
                  )}
                  {summary.confidence && (
                    <div>
                      <dt>{tr('Confidence')}</dt>
                      <dd>{confidenceName(summary.confidence)}</dd>
                    </div>
                  )}
                </dl>
              </>
            )}
          </section>

          <section className="compat-reports">
            <h3 className="section-title">{tr('From the Proton issue tracker')}</h3>
            {reports.state === 'loading' && (
              <div className="compat-msg">
                <Spinner /> {tr('Reading reports…')}
              </div>
            )}
            {reports.state === 'error' && <div className="compat-msg error">{reports.msg}</div>}
            {r?.stale && <p className="compat-note">{tr('GitHub is busy right now; showing reports saved earlier.')}</p>}
            {reports.state === 'ok' && !r?.issue && (
              <div className="compat-msg">{tr('No report thread for this game on the Proton tracker.')}</div>
            )}
            {r?.issue && (
              <div className="compat-issue">
                <b className="compat-issue-title">{r.issue.title}</b>
                <div className="compat-tags">
                  <span className={r.issue.state === 'open' ? 'open' : 'closed'}>{r.issue.state === 'open' ? tr('Open thread') : tr('Closed thread')}</span>
                  <span>{trn(r.issue.comments, '{n} comment', '{n} comments')}</span>
                  {r.issue.labels.slice(0, 4).map(l => (
                    <span key={l}>{l}</span>
                  ))}
                </div>
                {r.comments.length > 0 && <small className="compat-note">{tr('Newest first.')}</small>}
              </div>
            )}
            <div className="compat-comments">
              {r?.comments.map((c, i) => (
                <article key={c.url || i} data-nav tabIndex={0} className="compat-comment">
                  <header>
                    <b>{c.author}</b>
                    <span>{ago(c.created_at)}</span>
                  </header>
                  <p>{c.body}</p>
                </article>
              ))}
            </div>
            {!!r?.others.length && (
              <div className="compat-others">
                <h3 className="section-title">{tr('Other threads')}</h3>
                <ul>
                  {r.others.map(o => (
                    <li key={o.number}>
                      <span className={`dot ${o.state}`} /> {o.title}
                    </li>
                  ))}
                </ul>
              </div>
            )}
          </section>
        </div>
      )}
    </div>
  )
}
