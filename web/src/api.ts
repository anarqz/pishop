// Typed client for the launcher's JSON API.

export interface Entry {
  name: string
  dir: boolean
  size: number
  mtime: number
}

export interface Source {
  id: string
  kind: 'smb'
  name: string
  host: string
  share: string
  username: string
  has_password: boolean
  base_path: string
}

export interface SourceInput {
  id: string
  kind: 'smb'
  name: string
  host: string
  share: string
  username: string
  password: string
  base_path: string
}

export interface Place {
  id: string
  label: string
  path: string
  icon: 'roms' | 'sd' | 'folder' | 'download' | 'home'
  free: number
  total: number
}

export type JobStatus = 'queued' | 'scanning' | 'running' | 'done' | 'failed' | 'canceled'

export interface Job {
  id: number
  source_id: string
  source_name: string
  src_path: string
  name: string
  dir: boolean
  dest_dir: string
  status: JobStatus
  total_bytes: number
  done_bytes: number
  files_total: number
  files_done: number
  current: string
  speed: number
  error: string | null
  created: number
  finished: number | null
}

export interface Parsed {
  name: string
  platform: string | null
  platform_label: string | null
  region: string | null
  tags: string[]
  version: string | null
  group: string | null
}

export interface Release {
  id: string
  title: string
  parsed: Parsed
  size: number
  seeders: number
  leechers: number
  grabs: number | null
  files: number | null
  indexer: string
  publish_date: string
  info_url: string | null
  info_hash: string | null
  categories: string[]
  kind: string
}

export interface Art {
  game_id: number
  name: string
  year: number | null
  cover: string
  cover_thumb: string
  score: number
}

export interface ReleaseDetails {
  release: Release
  art: Art | null
  hero: string | null
  description: string | null
}

export interface TgdbInfo {
  id: number
  title: string
  overview: string | null
  release_date: string | null
  players: number | null
  rating: string | null
  genres: string[]
  developers: string[]
  publishers: string[]
  platform: string | null
  youtube: string | null
}

export interface SteamInfo {
  appid: string
  name: string
  overview: string | null
  genres: string[]
  developers: string[]
  publishers: string[]
  release_date: string | null
  background: string | null
  screenshot: string | null
}

export interface GameMeta {
  art: Art | null
  hero: string | null
  steam?: SteamInfo | null
  tgdb: TgdbInfo | null
}

export interface Trailer {
  video_id: string
  url: string
  title: string
}

export interface ServicesConfig {
  prowlarr_url: string
  has_key: boolean
  has_tgdb_key: boolean
  iic_url: string
  iic_cdn: string
  has_iic_key: boolean
}

export interface DiscoverGame {
  id: string
  title: string
  steam_appid: string | null
  cover: string | null
  header: string | null
  crack_date: string | null
  release_date: string | null
  scene_group: string | null
  drm: string | null
}

async function call<T>(path: string, init?: RequestInit): Promise<T> {
  const r = await fetch(path, {
    ...init,
    headers: init?.body ? { 'content-type': 'application/json' } : undefined,
  })
  if (!r.ok) {
    let msg = `HTTP ${r.status}`
    try {
      msg = (await r.json()).error ?? msg
    } catch {
      // non-JSON error body
    }
    throw new Error(msg)
  }
  return r.status === 204 ? (undefined as T) : r.json()
}

const post = <T>(path: string, body?: unknown) =>
  call<T>(path, { method: 'POST', body: body === undefined ? undefined : JSON.stringify(body) })

export const api = {
  sources: () => call<Source[]>('/api/sources'),
  saveSource: (s: SourceInput) => post<Source>('/api/sources', s),
  testSource: (s: SourceInput) => post<{ ok: boolean; entries: number }>('/api/sources/test', s),
  deleteSource: (id: string) => call<void>(`/api/sources/${id}`, { method: 'DELETE' }),
  listRemote: (id: string, path: string) =>
    call<{ path: string; entries: Entry[] }>(`/api/sources/${id}/list?path=${encodeURIComponent(path)}`),
  places: () => call<Place[]>('/api/local/places'),
  listLocal: (path: string) =>
    call<{ path: string; entries: Entry[]; free: number | null; total: number | null }>(
      `/api/local/list?path=${encodeURIComponent(path)}`,
    ),
  mkdir: (path: string) => post<void>('/api/local/mkdir', { path }),
  jobs: () => call<Job[]>('/api/jobs'),
  enqueue: (source_id: string, dest: string, items: { path: string; name: string; dir: boolean }[]) =>
    post<{ ids: number[] }>('/api/jobs', { source_id, dest, items }),
  jobAction: (id: number, action: 'cancel' | 'retry' | 'remove') => post<void>(`/api/jobs/${id}/${action}`),
  clearJobs: () => post<void>('/api/jobs/clear'),
  openKeyboard: () => post<void>('/api/keyboard'),
  quit: () => post<void>('/api/quit'),
  discover: (offset: number, search: string, limit = 30) =>
    call<{ items: DiscoverGame[]; total: number; offset: number }>(
      `/api/discover?offset=${offset}&limit=${limit}${search ? `&search=${encodeURIComponent(search)}` : ''}`,
    ),
  catalogConfig: () => call<ServicesConfig>('/api/catalog/config'),
  /** Partial update: only the fields given change; blank secrets keep the stored ones. */
  saveServices: (u: Partial<Record<'prowlarr_url' | 'prowlarr_key' | 'tgdb_key' | 'iic_url' | 'iic_key' | 'iic_cdn', string>>) =>
    post<ServicesConfig>('/api/catalog/config', u),
  testService: (service: 'tgdb' | 'iic', u: { url?: string; key?: string; cdn?: string }) =>
    post<{ remaining?: number; total?: number }>('/api/services/test', { service, ...u }),
  gameInfo: (name: string, full = false, appid?: string | null) =>
    call<GameMeta>(
      `/api/game/info?name=${encodeURIComponent(name)}${full ? '&tgdb=true' : ''}${appid ? `&appid=${encodeURIComponent(appid)}` : ''}`,
    ),
  openSteamStore: (appid: string) => post<void>(`/api/steam/open?appid=${encodeURIComponent(appid)}`),
  trailer: (name: string) => call<Trailer | null>(`/api/game/trailer?name=${encodeURIComponent(name)}`),
  context: (q: string, noCrack = false) =>
    call<{ art: Art | null; hero: string | null; crack: DiscoverGame | null }>(
      `/api/catalog/context?q=${encodeURIComponent(q)}${noCrack ? '&no_crack=true' : ''}`,
    ),
  testCatalog: (prowlarr_url: string, prowlarr_key: string) =>
    post<{ version: string; indexers: string[] }>('/api/catalog/config/test', { prowlarr_url, prowlarr_key }),
  search: (q: string, kind: 'console' | 'pc') =>
    call<Release[]>(`/api/catalog/search?q=${encodeURIComponent(q)}&kind=${kind}`),
  art: (name: string) => call<Art | null>(`/api/catalog/art?name=${encodeURIComponent(name)}`),
  release: (id: string) => call<ReleaseDetails>(`/api/catalog/release/${id}`),
  releaseFiles: (id: string) =>
    call<{ name: string; files: { name: string; length: number }[] }>(`/api/catalog/release/${id}/files`),
  download: (id: string, dest?: string) =>
    post<{ torrent_id: number; name: string }>(`/api/catalog/release/${id}/download`, { dest: dest ?? null }),
}

export function formatBytes(n: number) {
  if (!n) return '0 B'
  if (n < 1024) return `${n.toFixed(0)} B`
  const units = ['KB', 'MB', 'GB', 'TB']
  let v = n / 1024
  let i = 0
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024
    i++
  }
  return `${v.toFixed(v >= 100 ? 0 : 1)} ${units[i]}`
}

export function formatEta(seconds: number) {
  if (!isFinite(seconds) || seconds <= 0) return ''
  if (seconds < 60) return `${Math.ceil(seconds)} s`
  const m = Math.floor(seconds / 60)
  if (m < 60) return `${m} min`
  return `${Math.floor(m / 60)} h ${m % 60} min`
}

export function joinPath(base: string, name: string) {
  if (!base) return name
  return base.endsWith('/') ? `${base}${name}` : `${base}/${name}`
}

export function parentPath(path: string) {
  const i = path.replace(/\/+$/, '').lastIndexOf('/')
  if (i < 0) return ''
  return i === 0 ? '/' : path.slice(0, i)
}

/** Artwork goes through the launcher's caching proxy. */
export function imgUrl(u: string) {
  return `/api/catalog/img?u=${encodeURIComponent(u)}`
}
