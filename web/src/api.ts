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

/** Launcher-wide preferences (Settings). */
export interface AppSettings {
  lang: 'en' | 'pt'
}

/** A services file found on the device, ready to import. */
export interface ServicesFile {
  path: string
  name: string
  /** Unix seconds. */
  modified: number
  /** Which services it sets (e.g. ["Prowlarr", "TheGamesDB"]). */
  services: string[]
}

/** Install wizard (launcher's install.rs). */
export type InstallStage = 'ready' | 'installing' | 'installed'
export interface InstallState {
  stage: InstallStage
  appid: number | null
  tool: string
  target: string
  installer: string | null
  exe: string | null
  started: number
  runner?: string
  game_dir?: string | null
}
export interface ExeCandidate {
  path: string
  name: string
  size: number
  score: number
  root?: string
  /** The executable the installer registered as the game's icon. */
  registered?: boolean
}
export interface InstallTool {
  name: string
  display: string
  installed: boolean
}
/** Moving a game's files into its prefix. */
export interface MoveJob {
  state: 'running' | 'done' | 'failed' | 'canceled'
  done: number
  total: number
  to: string
  error: string | null
}
/** Where an installed game can move: its prefix, or a Steam library. */
export interface MoveTarget {
  /** "prefix" or the library's path */
  id: string
  kind: 'prefix' | 'library'
  label: string
  to: string
  windows: string
  /** The game is there now. */
  here: boolean
  same_disk: boolean
  free: number | null
  blocked: string | null
}
/** Where each piece of a shortcut's artwork came from. */
export type ArtSource = 'steam' | 'steamgriddb' | 'piShop' | null
export interface ArtResult {
  cover: ArtSource
  wide: ArtSource
  hero: ArtSource
  logo: ArtSource
  icon: ArtSource
}
export interface ArtJob {
  running: boolean
  done: number
  total: number
  current: string | null
}
/** An installed game: its Steam shortcut, folders, prefix and Proton. */
export interface InstallInfo {
  appid: number
  steam_api: boolean
  /** As Steam has it now (paths quoted the way Steam stores them). */
  shortcut: { exe: string; start_dir: string; launch_options: string; tool: string } | null
  tool: string
  tools: InstallTool[]
  exe: { path: string; windows: string } | null
  game_dir: { path: string; windows: string; size: number; disk: string } | null
  prefix: { path: string; exists: boolean; disk: string; free: number | null }
  in_prefix: boolean
  targets: MoveTarget[]
  moving: MoveJob | null
  /** An installer from the download running in the prefix right now. */
  borrowed: string | null
  running: boolean
  /** The game, an installer, winetricks or a move is using the prefix. */
  busy: boolean
}
/** Components for a prefix: Steam's redistributables and winetricks verbs. */
export interface ComponentsStatus {
  /** winetricks is bundled */
  available: boolean
  prefix: boolean
  steam: Array<{ id: string; title: string; installed: boolean }>
  verbs: Array<{ verb: string; offline: boolean; installed: boolean }>
  running: boolean
  /** What the current (or last) run is about. */
  current: string
  log: string[]
  exit: number | null
}
export interface InstallOptions {
  game: LibraryGame
  content: string[]
  /** The download's own folder. */
  download_dir: string
  installers: Array<{ path: string; name: string; kind: string | null }>
  portable: ExeCandidate[]
  libraries: Array<{ path: string; label: string; free: number | null; total: number | null }>
  default_library: string | null
  tools: InstallTool[]
  default_tool: string | null
  steam_api: boolean
  debugging_enabled: boolean
  state: InstallState | null
}
export interface InstallStatus {
  state: InstallState | null
  running?: boolean
  candidates?: ExeCandidate[]
}

/** Archives inside a download (launcher's archive.rs). */
export interface ArchiveSet {
  path: string
  name: string
  kind: string
  parts: number
  size: number
  /** False when a part is missing from the numbering (still downloading?). */
  complete?: boolean
}
export interface ArchiveJob {
  id: number
  archive: string
  dest: string
  state: 'running' | 'done' | 'failed' | 'canceled'
  progress: number
  error?: string | null
}

/** The game a Store download belongs to, as Transfers shows it. */
export interface LibraryGame {
  name: string
  cover?: string | null
  hero?: string | null
  year?: number | null
  platform?: string | null
  crack_date?: string | null
  scene_group?: string | null
  drm?: string | null
  steam_appid?: string | null
  overview?: string | null
  genres?: string[]
  developers?: string[]
  publishers?: string[]
  release_date?: string | null
  /** Services the data came from, in priority order. */
  sources?: string[]
}

/**
 * What the Store showed for the search, sent with a download; the launcher
 * adds Steam and TheGamesDB on top (Steam → TGDB → SteamGridDB → isitcracked).
 */
export interface GameHint {
  name: string
  sgdb: { name: string; cover: string; hero: string | null; year: number | null } | null
  crack: DiscoverGame | null
}

export interface LibraryEntry {
  info_hash: string
  game: LibraryGame
  release: string
  indexer: string
  size: number
  dest: string | null
  /** Unix seconds. */
  added: number
  /** False while Steam/TheGamesDB are still being asked. */
  resolved: boolean
  install?: InstallState | null
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
  tpb_url: string
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
  saveServices: (
    u: Partial<Record<'prowlarr_url' | 'prowlarr_key' | 'tgdb_key' | 'iic_url' | 'iic_key' | 'iic_cdn' | 'tpb_url', string>>,
  ) =>
    post<ServicesConfig>('/api/catalog/config', u),
  testService: (service: 'tgdb' | 'iic' | 'tpb', u: { url?: string; key?: string; cdn?: string }) =>
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
  /** Results from every indexer that answered, plus a note per one that failed. */
  search: (q: string, kind: 'console' | 'pc') =>
    call<{ results: Release[]; warnings: string[] }>(`/api/catalog/search?q=${encodeURIComponent(q)}&kind=${kind}`),
  art: (name: string) => call<Art | null>(`/api/catalog/art?name=${encodeURIComponent(name)}`),
  /** `art: false`: the caller already has the matched game (skip the lookup). */
  release: (id: string, art = true) => call<ReleaseDetails>(`/api/catalog/release/${id}${art ? '' : '?art=false'}`),
  releaseFiles: (id: string) =>
    call<{ name: string; files: { name: string; length: number }[] }>(`/api/catalog/release/${id}/files`),
  download: (id: string, dest?: string, hint?: GameHint) =>
    post<{ torrent_id: number; name: string; info_hash: string }>(`/api/catalog/release/${id}/download`, {
      dest: dest ?? null,
      hint: hint ?? null,
    }),
  settings: () => call<AppSettings>('/api/settings'),
  saveSettings: (s: Partial<AppSettings>) => post<AppSettings>('/api/settings', s),
  /** Settings → Services: share the services setup (API keys included) as a file. */
  exportServices: () => post<{ path: string }>('/api/services/export'),
  importCandidates: () => call<ServicesFile[]>('/api/services/import'),
  importServices: (path: string) => post<{ imported: string[] }>('/api/services/import', { path }),
  installOptions: (hash: string) => call<InstallOptions>(`/api/install/${hash}`),
  installStart: (hash: string, r: { installer: string; library: string; tool: string }) =>
    post<InstallState>(`/api/install/${hash}/start`, r),
  installStatus: (hash: string) => call<InstallStatus>(`/api/install/${hash}/status`),
  installFinish: (hash: string, exe: string) => post<InstallState>(`/api/install/${hash}/finish`, { exe }),
  installPortable: (hash: string, exe: string, tool: string) => post<InstallState>(`/api/install/${hash}/portable`, { exe, tool }),
  installPlay: (hash: string) => post<{ appid: number }>(`/api/install/${hash}/play`),
  installReset: (hash: string) => post<object>(`/api/install/${hash}/reset`),
  installInfo: (hash: string) => call<InstallInfo>(`/api/install/${hash}/info`),
  installTool: (hash: string, tool: string) => post<InstallState>(`/api/install/${hash}/tool`, { tool }),
  installMove: (hash: string, to: string) => post<{ started: boolean }>(`/api/install/${hash}/move`, { to }),
  installArtwork: (hash: string) => post<ArtResult>(`/api/install/${hash}/artwork`),
  libraryArtwork: () => post<ArtJob>('/api/library/artwork'),
  libraryArtworkStatus: () => call<ArtJob>('/api/library/artwork'),
  installMoveStatus: (hash: string) => call<MoveJob | null>(`/api/install/${hash}/move`),
  installMoveCancel: (hash: string) => post<object>(`/api/install/${hash}/move/cancel`),
  components: (hash: string) => call<ComponentsStatus>(`/api/install/${hash}/components`),
  componentsRun: (hash: string, r: { steam: string[]; verbs: string[] }) => post<{ running: boolean }>(`/api/install/${hash}/components`, r),
  componentsCancel: (hash: string) => post<object>(`/api/install/${hash}/components/cancel`),
  installRun: (hash: string, path: string) => post<{ running: boolean }>(`/api/install/${hash}/run`, { path }),
  enableSteamApi: () => post<{ restarting: boolean }>('/api/steam/client-api'),
  archiveInspect: (path: string) =>
    call<{ archives: ArchiveSet[]; exe_count: number; has_installer: boolean }>(`/api/archive/inspect?path=${encodeURIComponent(path)}`),
  archiveExtract: (path: string, deleteAfter: boolean, dest?: string) =>
    post<{ id: number }>('/api/archive/extract', { path, delete_after: deleteAfter, dest: dest ?? null }),
  archiveJobs: () => call<ArchiveJob[]>('/api/archive/jobs'),
  archiveCancel: (id: number) => post<object>(`/api/archive/jobs/${id}/cancel`),
  /** Settings → Downloads: overall torrent speed cap (bytes/s, null = none). */
  torrentLimits: () => call<{ download_bps: number | null }>('/api/torrent/limits'),
  setTorrentLimits: (download_bps: number | null) => post<{ download_bps: number | null }>('/api/torrent/limits', { download_bps }),
  /** Game metadata of Store downloads, by info hash. */
  library: () => call<Record<string, LibraryEntry>>('/api/library'),
  forgetLibrary: (infoHash: string) => call<void>(`/api/library/${infoHash}`, { method: 'DELETE' }),
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
