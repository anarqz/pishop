// UI language: English (default) and Brazilian Portuguese.
//
// Strings are written in English right in the code — tr('View game') — and
// the Portuguese lives in src/locales/pt/*.json, keyed by that English text.
// `pnpm i18n` (also part of `pnpm build`) fails when a string has no
// translation. The launcher keeps the choice (Settings → Language) and uses it
// for Steam data and its own messages too.

import { type ReactNode, useSyncExternalStore } from 'react'
import PT from './locales/pt'

export type Lang = 'en' | 'pt'
export const LANGS: Array<[Lang, string]> = [
  ['en', 'English'],
  ['pt', 'Português (Brasil)'],
]

const CACHE = 'pishop.lang'
type Vars = Record<string, string | number>

function cached(): Lang {
  try {
    return localStorage.getItem(CACHE) === 'pt' ? 'pt' : 'en'
  } catch {
    return 'en'
  }
}

let lang: Lang = cached()
const subs = new Set<() => void>()
document.documentElement.lang = lang === 'pt' ? 'pt-BR' : 'en'

export const getLang = () => lang

/** Switches the UI language (the launcher's setting is saved separately). */
export function setLang(next: Lang) {
  if (next === lang) return
  lang = next
  try {
    localStorage.setItem(CACHE, next)
  } catch {
    // storage unavailable: the launcher still has the setting
  }
  document.documentElement.lang = next === 'pt' ? 'pt-BR' : 'en'
  subs.forEach(f => f())
}

/** Re-renders the caller when the language changes. */
export function useLang() {
  return useSyncExternalStore(
    f => {
      subs.add(f)
      return () => void subs.delete(f)
    },
    () => lang,
  )
}

/** BCP 47 locale for dates and numbers. */
export const locale = () => (lang === 'pt' ? 'pt-BR' : 'en-US')

/** Translates an English UI string; `{name}` placeholders come from `vars`. */
export function tr(en: string, vars?: Vars): string {
  const s = lang === 'pt' ? (PT[en] ?? en) : en
  return vars ? s.replace(/\{(\w+)\}/g, (m, k: string) => (k in vars ? String(vars[k]) : m)) : s
}

/** Singular or plural by `n` (available as `{n}`). */
export function trn(n: number, one: string, many: string, vars?: Vars): string {
  return tr(n === 1 ? one : many, { n, ...vars })
}

/** tr() for text with **bold** parts, as React nodes. */
export function trb(en: string, vars?: Vars): ReactNode[] {
  return tr(en, vars)
    .split(/\*\*(.+?)\*\*/g)
    .map((part, i) => (i % 2 ? <b key={i}>{part}</b> : part))
}
