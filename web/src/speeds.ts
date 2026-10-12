// Download speed history of each transfer, for the graph on Transfers' banner
// (like Steam's Downloads page). It's sampled from the polls that already run
// for the tab bar's badge (every few seconds) and for Transfers itself (every
// second), so a download has a history before Transfers is even opened. Kept
// in memory: it starts over when piShop does.

import { useSyncExternalStore } from 'react'

/** How far back the graph goes. */
export const WINDOW_MS = 10 * 60_000
/** Both polls report the same moment: one sample per this much time. */
const MIN_GAP_MS = 700

/** When, and bytes per second then. */
export type Sample = [at: number, bps: number]

const series = new Map<string, Sample[]>()
let version = 0
const subs = new Set<() => void>()

/** History keys: the same transfer keeps its key while piShop runs. */
export const speedKey = {
  torrent: (infoHash: string) => `t:${infoHash}`,
  community: (id: string) => `c:${id}`,
  direct: (id: number) => `d:${id}`,
}

/** Adds a sample; call `speedsChanged()` once after a batch. */
export function recordSpeed(key: string, bps: number, at = Date.now()) {
  let s = series.get(key)
  if (!s) series.set(key, (s = []))
  const last = s[s.length - 1]
  if (last && at - last[0] < MIN_GAP_MS) return
  s.push([at, Math.max(0, bps)])
  const cutoff = at - WINDOW_MS
  while (s.length && s[0][0] < cutoff) s.shift()
}

export function speedsChanged() {
  version++
  subs.forEach(f => f())
}

/** A transfer's samples, oldest first; re-renders the caller on new ones. */
export function useSpeedHistory(key: string | null): Sample[] {
  useSyncExternalStore(
    f => {
      subs.add(f)
      return () => void subs.delete(f)
    },
    () => version,
  )
  return key ? (series.get(key) ?? []) : []
}
