// Torrent downloads: the Store's metadata for them, their state as Transfers
// shows it, and Y's delete dialog. The rows and the banner are in
// TransferRows.tsx, shared with the other kinds of transfer.

import { useEffect, useState } from 'react'
import { type LibraryEntry, api } from '../api'
import { tr } from '../i18n'
import { type TorrentInfo, destroy, forget, formatBytes, useEngineState } from '../torrent'
import { Dialog, toast } from '../ui'

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

export function hue(s: string) {
  let h = 0
  for (const c of s) h = (h * 31 + c.charCodeAt(0)) % 360
  return h
}

/**
 * Y: remove the download, keeping the files or deleting them too. A game
 * installed from it stays in Games either way; its files are only deleted
 * when the game doesn't run from them (the launcher checks).
 */
export function DeleteDialog({ t, entry, onClose }: { t: TorrentInfo; entry?: LibraryEntry; onClose: () => void }) {
  const name = entry?.game.name ?? t.name
  const installed = !!entry?.install?.appid
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
        <button
          data-nav
          className="menu-item danger"
          onClick={() => run(installed ? api.installDeleteDownload(t.infoHash) : destroy(t.id), tr('Transfer and data deleted'))}
        >
          {tr('Also delete the data')}
          <small>
            {installed
              ? tr('Erases the {size} downloaded and what was extracted from it; the game stays in Games', {
                  size: formatBytes(t.progress * t.totalBytes),
                })
              : tr('Erases the {size} already downloaded', { size: formatBytes(t.progress * t.totalBytes) })}
          </small>
        </button>
        <button data-nav className="menu-item" onClick={onClose}>
          {tr('Cancel')}
        </button>
      </div>
    </Dialog>
  )
}
