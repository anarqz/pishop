// Community installs and direct downloads, polled from the launcher: Transfers
// shows them next to torrents, the tab bar counts the active ones.

import { useEffect, useState } from 'react'
import { type CommunityInstall, type CommunityInstallPart, type DirectDownload, api } from './api'
import { tr } from './i18n'

export interface Extra {
  installs: CommunityInstall[]
  directs: DirectDownload[]
}

const EMPTY: Extra = { installs: [], directs: [] }

/** Community installs and direct downloads, refreshed every `ms`. */
export function useTransfersExtra(ms: number) {
  const [state, setState] = useState<Extra>(EMPTY)
  useEffect(() => {
    let alive = true
    let timer = 0
    const tick = async () => {
      const [installs, directs] = await Promise.all([api.communityInstalls().catch(() => null), api.directs().catch(() => null)])
      if (!alive) return
      setState(s => ({ installs: installs ?? s.installs, directs: directs ?? s.directs }))
      timer = window.setTimeout(tick, ms)
    }
    void tick()
    return () => {
      alive = false
      clearTimeout(timer)
    }
  }, [ms])
  return state
}

/** Still moving: downloading, extracting, running its script, adding the shortcut. */
export const installActive = (i: CommunityInstall) => !['installed', 'failed'].includes(i.stage) && !(i.paused && i.stage === 'downloading')

/** Waiting for the user to pick a file. */
export const needsFile = (i: CommunityInstall) => i.parts.some(p => p.state === 'waiting')

/** Torrents that belong to a community install (shown inside the game, not on their own). */
export const ownedTorrents = (installs: CommunityInstall[]) =>
  new Set(installs.flatMap(i => i.parts.map(p => p.torrent).filter((h): h is string => !!h)))

/** Where a part comes from, as a short label. */
export function partKind(p: Pick<CommunityInstallPart, 'kind' | 'provided'>) {
  if (p.provided) return tr('Your file')
  switch (p.kind) {
    case 'itch':
      return 'itch.io'
    case 'github':
      return 'GitHub'
    case 'torrent':
      return tr('Torrent')
    case 'user':
      return tr('Your file')
    default:
      return tr('Direct download')
  }
}

/** The step an install is on, for kickers and tiles. */
export function stageLabel(i: CommunityInstall) {
  switch (i.stage) {
    case 'downloading':
      if (needsFile(i)) return tr('Waiting for your file')
      if (i.parts.some(p => p.state === 'verifying')) return tr('Checking the files')
      return i.paused ? tr('Paused') : tr('Downloading')
    case 'extracting':
      return tr('Extracting')
    case 'preparing':
      return tr('Preparing the game')
    case 'shortcut':
      return tr('Adding to Steam')
    case 'installed':
      return tr('Installed')
    case 'failed':
      return tr('Needs attention')
  }
}

/** The file name at the end of a path. */
export const baseName = (p: string) => p.split('/').pop() ?? p
