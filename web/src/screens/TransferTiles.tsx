// Transfers' other two kinds besides torrents: community installs and direct
// downloads added by hand — their state, and the dialogs Y and A open. Their
// rows and banner are in TransferRows.tsx.

import { type CommunityInstall, type DirectDownload, api, formatBytes } from '../api'
import { baseName, installActive, needsFile } from '../community'
import { tr } from '../i18n'
import { Dialog, toast } from '../ui'
import type { DlState } from './Torrents'

// ---------- community installs ----------

/** An install's step as a transfer state (the row's colors, the list's order). */
export function installState(i: CommunityInstall): DlState {
  if (i.stage === 'installed') return 'done'
  if (i.stage === 'failed') return 'error'
  if (i.stage === 'downloading' && (i.paused || needsFile(i))) return 'paused'
  if (i.stage === 'preparing' && i.progress == null) return 'checking'
  return 'active'
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
