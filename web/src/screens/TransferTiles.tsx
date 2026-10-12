// Transfers' other two kinds besides torrents: community installs (one tile
// per game, its parts inside) and direct downloads added by hand. Same shelf
// look as torrents: a cover with a ring, two lines under it, the focused one
// detailed in the panel on top.

import { type CommunityInstall, type DirectDownload, api, formatBytes, formatEta, imgUrl } from '../api'
import { baseName, installActive, needsFile, stageLabel } from '../community'
import { tr, trn } from '../i18n'
import { Dialog, Icon, toast } from '../ui'
import { Cover, type DlState, ProgressRing, hue } from './Torrents'

const pct = (f: number | null | undefined) => Math.floor(Math.max(0, Math.min(1, f ?? 0)) * 100)

// ---------- community installs ----------

/** The ring's look for an install's step. */
export function installState(i: CommunityInstall): DlState {
  if (i.stage === 'installed') return 'done'
  if (i.stage === 'failed') return 'error'
  if (i.stage === 'downloading' && (i.paused || needsFile(i))) return 'paused'
  if (i.stage === 'preparing' && i.progress == null) return 'checking'
  return 'active'
}

/** The two lines under an install's cover. */
function installLines(i: CommunityInstall): [string, string] {
  switch (i.stage) {
    case 'downloading': {
      const waiting = i.parts.find(p => p.state === 'waiting')
      if (waiting) return [tr('Waiting for your file'), waiting.name]
      const checking = i.parts.find(p => p.state === 'verifying')
      if (checking) return [tr('Checking the files'), `${checking.name} · ${pct(checking.verify)}%`]
      if (i.paused) return [tr('Paused'), tr('{done} of {total}', { done: formatBytes(i.done), total: formatBytes(i.total) })]
      const eta = i.speed > 0 ? formatEta((i.total - i.done) / i.speed) : ''
      return [`↓ ${formatBytes(i.speed)}/s${eta ? ` · ${eta}` : ''}`, tr('{done} of {total}', { done: formatBytes(i.done), total: formatBytes(i.total) })]
    }
    case 'extracting':
      return [tr('Extracting'), `${pct(i.progress)}%`]
    case 'preparing':
      return [i.step ?? tr('Preparing the game'), i.progress != null ? `${pct(i.progress)}%` : tr('Install script')]
    case 'shortcut':
      return [tr('Adding to Steam'), i.platform]
    case 'installed':
      return [tr('In Games'), tr('Community')]
    case 'failed':
      return [tr('Needs attention'), i.error ?? '']
  }
}

export function CommunityTile({ i, first, onOpen }: { i: CommunityInstall; first?: boolean; onOpen: () => void }) {
  const state = installState(i)
  const [line1, line2] = installLines(i)
  const progress = i.stage === 'downloading' ? (i.total ? i.done / i.total : 0) : (i.progress ?? (i.stage === 'installed' ? 1 : 0))
  return (
    <div
      data-nav
      data-nav-default={first ? '' : undefined}
      data-community-id={i.id}
      tabIndex={0}
      className={`dl-tile ${state}`}
      onClick={onOpen}
    >
      <div className="dl-cover" style={{ ['--h' as string]: hue(i.name) }}>
        <Cover name={i.name} cover={i.game?.cover} />
        <ProgressRing pct={pct(progress)} state={state} note={needsFile(i) ? tr('Your file') : undefined} />
        <span className="plat">{i.platform}</span>
        <span className="cm-badge" title={tr('Community')}>
          <Icon name="globe" size={15} />
        </span>
      </div>
      <div className="dl-info">
        <b className="dl-name">{i.name}</b>
        <span className={`dl-line ${state}`}>{line1}</span>
        <span className="dl-line sub">{line2}</span>
      </div>
    </div>
  )
}

/** The panel on top for a community install. */
export function CommunityHero({ i }: { i: CommunityInstall }) {
  const g = i.game
  const state = installState(i)
  const eta = i.stage === 'downloading' && i.speed > 0 && !i.paused ? formatEta((i.total - i.done) / i.speed) : null
  const chips = [i.platform, i.wrapper ? tr('via {name}', { name: i.wrapper.name }) : null, trn(i.parts.length, '{n} part', '{n} parts')].filter(Boolean)
  const facts: Array<[string, string]> = [
    [tr('Step'), stageLabel(i)],
    ...(i.stage === 'downloading' ? ([[tr('Downloaded'), tr('{done} of {total}', { done: formatBytes(i.done), total: formatBytes(i.total) })]] as Array<[string, string]>) : []),
    ...(i.stage === 'downloading' && !i.paused ? ([[tr('Download speed'), `${formatBytes(i.speed)}/s`]] as Array<[string, string]>) : []),
    ...(eta ? ([[tr('Time left'), eta]] as Array<[string, string]>) : []),
    ...(i.step && i.stage !== 'downloading' ? ([[tr('Now'), i.step]] as Array<[string, string]>) : []),
  ]
  return (
    <section className="dl-hero" key={`cm-${i.id}`}>
      {g?.hero && <div className="dl-hero-bg" style={{ backgroundImage: `url(${imgUrl(g.hero)})` }} />}
      <div className="dl-hero-shade" />
      <div className="dl-hero-content">
        <span className={`dl-kicker ${state}`}>
          {tr('Community')} · {stageLabel(i)}
        </span>
        <h2>{i.name}</h2>
        <div className="dl-chips">
          {chips.map(c => (
            <span key={String(c)}>{c}</span>
          ))}
        </div>
        {i.stage === 'failed' ? (
          <p className="dl-hero-error">{i.error}</p>
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

/**
 * Y on an install: before it's done, cancel it (keeping or deleting the
 * downloads); once installed, take it off Transfers, with or without the
 * downloaded files. The game stays in Games either way.
 */
export function CommunityRemoveDialog({ i, onClose }: { i: CommunityInstall; onClose: () => void }) {
  const installed = i.stage === 'installed'
  const size = i.parts.filter(p => !p.provided && p.kind !== 'user').reduce((s, p) => s + p.done, 0)
  const go = (del: boolean) => {
    onClose()
    api
      .communityRemove(i.id, del)
      .then(r =>
        toast(
          installed ? tr('Removed from Transfers') : tr('Installation canceled'),
          del ? tr('{size} freed', { size: formatBytes(r.freed || size) }) : i.name,
          'ok',
        ),
      )
      .catch(e => toast(tr('Something went wrong'), (e as Error).message, 'error'))
  }
  return (
    <Dialog title={installed ? tr('Remove {name} from Transfers?', { name: i.name }) : tr('Cancel installing {name}?', { name: i.name })} onClose={onClose}>
      <div className="menu-list">
        <button data-nav data-nav-default className="menu-item" onClick={() => go(false)}>
          {installed ? tr('Remove from Transfers') : tr('Cancel, keep the downloads')}
          <small>
            {installed
              ? tr('The game stays in Games; the downloaded files stay on the device')
              : tr('The game’s folder goes; what was downloaded stays in Downloads for another try')}
          </small>
        </button>
        <button data-nav className="menu-item danger" onClick={() => go(true)}>
          {installed ? tr('Also delete the downloaded files') : tr('Cancel and delete the downloads')}
          <small>
            {tr('Erases the {size} downloaded; files you picked yourself stay', { size: formatBytes(size) })}
          </small>
        </button>
        <button data-nav className="menu-item" onClick={onClose}>
          {tr('Back')}
        </button>
      </div>
    </Dialog>
  )
}

// ---------- direct downloads ----------

export function directState(d: DirectDownload): DlState {
  if (d.state === 'failed') return 'error'
  if (d.state === 'done') return 'done'
  if (d.state === 'paused') return 'paused'
  return d.total ? 'active' : 'checking'
}

function directLines(d: DirectDownload): [string, string] {
  const of = d.total ? tr('{done} of {total}', { done: formatBytes(d.done), total: formatBytes(d.total) }) : formatBytes(d.done)
  switch (d.state) {
    case 'running': {
      const eta = d.speed > 0 && d.total ? formatEta((d.total - d.done) / d.speed) : ''
      return [`↓ ${formatBytes(d.speed)}/s${eta ? ` · ${eta}` : ''}`, of]
    }
    case 'paused':
      return [tr('Paused'), of]
    case 'failed':
      return [tr('Download error'), d.error ?? '']
    case 'done':
      return [tr('Complete'), formatBytes(d.total)]
  }
}

export function DirectTile({ d, first, onOpen }: { d: DirectDownload; first?: boolean; onOpen: () => void }) {
  const name = baseName(d.path)
  const state = directState(d)
  const [line1, line2] = directLines(d)
  return (
    <div data-nav data-nav-default={first ? '' : undefined} data-direct-id={d.id} tabIndex={0} className={`dl-tile ${state}`} onClick={onOpen}>
      <div className="dl-cover direct" style={{ ['--h' as string]: hue(name) }}>
        <div className="dl-cover-ph">
          <Icon name="file" size={38} />
          <b>{name}</b>
        </div>
        <ProgressRing pct={pct(d.total ? d.done / d.total : 0)} state={state} />
      </div>
      <div className="dl-info">
        <b className="dl-name">{name}</b>
        <span className={`dl-line ${state}`}>{line1}</span>
        <span className="dl-line sub">{line2}</span>
      </div>
    </div>
  )
}

export function DirectHero({ d }: { d: DirectDownload }) {
  const state = directState(d)
  const eta = d.state === 'running' && d.speed > 0 && d.total ? formatEta((d.total - d.done) / d.speed) : null
  const facts: Array<[string, string]> = [
    [tr('Progress'), d.total ? `${pct(d.done / d.total)}%` : '—'],
    [tr('Downloaded'), d.total ? tr('{done} of {total}', { done: formatBytes(d.done), total: formatBytes(d.total) }) : formatBytes(d.done)],
    ...(d.state === 'running' ? ([[tr('Download speed'), `${formatBytes(d.speed)}/s`]] as Array<[string, string]>) : []),
    ...(eta ? ([[tr('Time left'), eta]] as Array<[string, string]>) : []),
  ]
  return (
    <section className="dl-hero" key={`d-${d.id}`}>
      <div className="dl-hero-shade" />
      <div className="dl-hero-content">
        <span className={`dl-kicker ${state}`}>{tr('Direct download')}</span>
        <h2>{baseName(d.path)}</h2>
        <span className="dl-byline">{d.path.replace(/^\/home\/[^/]+/, '~')}</span>
        {d.state === 'failed' ? (
          <p className="dl-hero-error">{d.error}</p>
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

/** A to open a direct download: show it, pause/resume, delete. */
export function DirectDialog({ d, onClose, onExplore }: { d: DirectDownload; onClose: () => void; onExplore?: (path: string) => void }) {
  const act = (action: 'pause' | 'resume' | 'remove' | 'delete', done?: string) => {
    onClose()
    api
      .directAction(d.id, action)
      .then(() => done && toast(done, baseName(d.path), 'ok'))
      .catch(e => toast(tr('Action failed'), (e as Error).message, 'error'))
  }
  const dir = d.path.replace(/\/[^/]+$/, '') || '/'
  return (
    <Dialog title={baseName(d.path)} onClose={onClose}>
      <div className="menu-list">
        {d.state === 'done' && onExplore && (
          <button data-nav data-nav-default className="menu-item" onClick={() => (onClose(), onExplore(dir))}>
            {tr('Show on Explore')}
            <small>{dir.replace(/^\/home\/[^/]+/, '~')}</small>
          </button>
        )}
        {d.state === 'running' && (
          <button data-nav data-nav-default className="menu-item" onClick={() => act('pause')}>
            {tr('Pause')}
          </button>
        )}
        {(d.state === 'paused' || d.state === 'failed') && (
          <button data-nav data-nav-default className="menu-item" onClick={() => act('resume')}>
            {d.state === 'failed' ? tr('Try again') : tr('Resume')}
            <small>{tr('Continues from where it stopped')}</small>
          </button>
        )}
        <button data-nav className="menu-item" onClick={() => act('remove', tr('Transfer deleted'))}>
          {tr('Delete the transfer')}
          <small>{tr('Removes it from the list; the downloaded files stay on the device')}</small>
        </button>
        <button data-nav className="menu-item danger" onClick={() => act('delete', tr('Transfer and data deleted'))}>
          {tr('Also delete the data')}
          <small>{tr('Erases the {size} already downloaded', { size: formatBytes(d.done) })}</small>
        </button>
        <button data-nav className="menu-item" onClick={onClose}>
          {tr('Cancel')}
        </button>
      </div>
    </Dialog>
  )
}

/** Active community installs and direct downloads, for the tab's badge. */
export function extraActive(installs: CommunityInstall[], directs: DirectDownload[]) {
  return installs.filter(installActive).length + directs.filter(d => !d.owner && d.state === 'running').length
}
