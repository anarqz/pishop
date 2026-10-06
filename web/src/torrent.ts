import { useEffect, useState } from 'react'

// Client for the native BitTorrent engine (librqbit) embedded in the Rust
// launcher. It speaks regular TCP/uTP BitTorrent, DHT and trackers.

export const SINTEL_MAGNET =
  'magnet:?xt=urn:btih:08ada5a7a6183aae1e09d831df6748d566095a10&dn=Sintel' +
  '&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337&tr=udp%3A%2F%2Fexplodie.org%3A6969' +
  '&tr=udp%3A%2F%2Ftracker.empire-js.us%3A1337&tr=wss%3A%2F%2Ftracker.openwebtorrent.com'

const VIDEO_EXT = /\.(mp4|m4v|mkv|webm|mov|avi)$/i

let apiBase = 'http://127.0.0.1:47801'

export function setTorrentApi(base: string) {
  apiBase = base
}

export interface TorrentFile {
  index: number
  name: string
  length: number
}

export interface TorrentInfo {
  id: number
  infoHash: string
  name: string
  state: string
  progress: number
  totalBytes: number
  downloadSpeed: number
  uploadSpeed: number
  peers: number
  finished: boolean
  error: string | null
  files: TorrentFile[]
}

export interface EngineState {
  online: boolean
  downloadSpeed: number
  uploadSpeed: number
  peers: number
  torrents: TorrentInfo[]
}

const MIB = 1024 * 1024

async function call(path: string, init?: RequestInit) {
  const r = await fetch(`${apiBase}${path}`, init)
  if (!r.ok) throw new Error((await r.text()) || `HTTP ${r.status}`)
  return r.json()
}

async function details(id: number): Promise<TorrentFile[]> {
  const d = await call(`/torrents/${id}`)
  return (d.files ?? []).map((f: any, index: number) => ({ index, name: f.name, length: f.length }))
}

// File lists never change once metadata is known; cache them per torrent.
const filesCache = new Map<number, TorrentFile[]>()

async function fetchState(): Promise<EngineState> {
  const [list, stats] = await Promise.all([call('/torrents?with_stats=true'), call('/stats')])
  const torrents: TorrentInfo[] = await Promise.all(
    (list.torrents ?? []).map(async (t: any) => {
      const s = t.stats ?? {}
      const live = s.live ?? {}
      let files = filesCache.get(t.id)
      if (!files && t.name) {
        files = await details(t.id).catch(() => [])
        if (files.length) filesCache.set(t.id, files)
      }
      return {
        id: t.id,
        infoHash: t.info_hash,
        name: t.name ?? t.info_hash,
        state: s.state ?? 'initializing',
        progress: s.total_bytes ? s.progress_bytes / s.total_bytes : 0,
        totalBytes: s.total_bytes ?? 0,
        downloadSpeed: (live.download_speed?.mbps ?? 0) * MIB,
        uploadSpeed: (live.upload_speed?.mbps ?? 0) * MIB,
        peers: live.snapshot?.peer_stats?.live ?? 0,
        finished: !!s.finished,
        error: s.error ?? null,
        files: files ?? [],
      }
    }),
  )
  return {
    online: true,
    downloadSpeed: (stats.download_speed?.mbps ?? 0) * MIB,
    uploadSpeed: (stats.upload_speed?.mbps ?? 0) * MIB,
    peers: stats.peers?.live ?? 0,
    torrents,
  }
}

const OFFLINE: EngineState = { online: false, downloadSpeed: 0, uploadSpeed: 0, peers: 0, torrents: [] }

export function useEngineState(intervalMs = 1000) {
  const [state, setState] = useState<EngineState>(OFFLINE)
  useEffect(() => {
    let alive = true
    let timer = 0
    const tick = async () => {
      const next = await fetchState().catch(() => OFFLINE)
      if (!alive) return
      setState(next)
      timer = window.setTimeout(tick, intervalMs)
    }
    void tick()
    return () => {
      alive = false
      clearTimeout(timer)
    }
  }, [intervalMs])
  return state
}

export async function addMagnet(magnet: string) {
  await call('/torrents?overwrite=true', { method: 'POST', body: magnet })
}

export const pause = (id: number) => call(`/torrents/${id}/pause`, { method: 'POST' })
export const resume = (id: number) => call(`/torrents/${id}/start`, { method: 'POST' })
/** Removes from the list but keeps downloaded files. */
export const forget = (id: number) => {
  filesCache.delete(id)
  return call(`/torrents/${id}/forget`, { method: 'POST' })
}
/** Removes and deletes downloaded files. */
export const destroy = (id: number) => {
  filesCache.delete(id)
  return call(`/torrents/${id}/delete`, { method: 'POST' })
}

/** Largest video file, which is what "watch" should play. */
export function mainVideo(t: TorrentInfo): TorrentFile | null {
  const videos = t.files.filter(f => VIDEO_EXT.test(f.name))
  return videos.sort((a, b) => b.length - a.length)[0] ?? null
}

/** Ranged HTTP stream; plays while still downloading (pieces are prioritised). */
export function streamUrl(t: TorrentInfo, f: TorrentFile) {
  return `${apiBase}/torrents/${t.id}/stream/${f.index}/${encodeURIComponent(f.name)}`
}

export function formatBytes(n: number) {
  if (n < 1024) return `${n.toFixed(0)} B`
  const units = ['KB', 'MB', 'GB', 'TB']
  let v = n / 1024
  let i = 0
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024
    i++
  }
  return `${v.toFixed(1)} ${units[i]}`
}
