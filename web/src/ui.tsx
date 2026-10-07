// Shared SteamOS-styled building blocks: button glyphs, the contextual footer
// legend, dialogs, the top-pinned text prompt, toasts and icons.

import { type ReactNode, useEffect, useRef, useState, useSyncExternalStore } from 'react'
import { tr } from './i18n'
import { focusFirst, input, showKeyboard } from './input'

// ---------- Footer legend ----------

export interface Hint {
  /** Two glyphs for a pair of buttons with one meaning (e.g. L2/R2). */
  glyph: GlyphName | [GlyphName, GlyphName]
  label: string
}

type GlyphName = 'A' | 'B' | 'X' | 'Y' | 'L1' | 'R1' | 'L2' | 'R2' | 'MENU' | 'VIEW' | 'DPAD'

let hintStack: { id: number; hints: Hint[] }[] = []
let hintSubs = new Set<() => void>()
let hintSeq = 0
const notifyHints = () => hintSubs.forEach(f => f())

/** Sets the footer legend while the calling component is mounted (last wins). */
export function useHints(hints: Hint[] | null) {
  const key = hints ? JSON.stringify(hints) : ''
  useEffect(() => {
    if (!hints) return
    const id = ++hintSeq
    hintStack = [...hintStack, { id, hints }]
    notifyHints()
    return () => {
      hintStack = hintStack.filter(h => h.id !== id)
      notifyHints()
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key])
}

function useHintStack() {
  return useSyncExternalStore(
    f => {
      hintSubs.add(f)
      return () => void hintSubs.delete(f)
    },
    () => hintStack,
  )
}

export function Glyph({ name }: { name: GlyphName }) {
  const wide = name.length > 1
  return <span className={`glyph ${wide ? 'wide' : ''} g-${name.toLowerCase()}`}>{glyphText(name)}</span>
}

function glyphText(name: GlyphName) {
  switch (name) {
    case 'MENU':
      return '☰'
    case 'VIEW':
      return '⧉'
    case 'DPAD':
      return '✥'
    default:
      return name
  }
}

export function Footer({ quitProgress }: { quitProgress: number }) {
  const stack = useHintStack()
  const hints = stack[stack.length - 1]?.hints ?? []
  return (
    <footer className="footer">
      {quitProgress > 0 && (
        <div className="quitbar">
          <div style={{ width: `${quitProgress * 100}%` }} />
        </div>
      )}
      <div className="footer-left">
        <Glyph name="VIEW" />
        <span>+</span>
        <Glyph name="MENU" />
        <span className="footer-label">{tr('Hold to quit')}</span>
      </div>
      <div className="footer-right">
        {hints.map(h => (
          <span key={String(h.glyph) + h.label} className="hint">
            {(Array.isArray(h.glyph) ? h.glyph : [h.glyph]).map(g => (
              <Glyph key={g} name={g} />
            ))}
            <span className="footer-label">{h.label}</span>
          </span>
        ))}
      </div>
    </footer>
  )
}

// ---------- Toasts ----------

interface Toast {
  id: number
  title: string
  body?: string
  kind: 'info' | 'error' | 'ok'
}
let toasts: Toast[] = []
const toastSubs = new Set<() => void>()
let toastSeq = 0

export function toast(title: string, body?: string, kind: Toast['kind'] = 'info') {
  const id = ++toastSeq
  toasts = [...toasts.slice(-2), { id, title, body, kind }]
  toastSubs.forEach(f => f())
  setTimeout(() => {
    toasts = toasts.filter(t => t.id !== id)
    toastSubs.forEach(f => f())
  }, 3500)
}

export function Toasts() {
  const list = useSyncExternalStore(
    f => {
      toastSubs.add(f)
      return () => void toastSubs.delete(f)
    },
    () => toasts,
  )
  return (
    <div className="toasts">
      {list.map(t => (
        <div key={t.id} className={`toast toast-${t.kind}`}>
          <b>{t.title}</b>
          {t.body && <span>{t.body}</span>}
        </div>
      ))}
    </div>
  )
}

// ---------- Dialog ----------

/**
 * Centered modal. Navigation is trapped inside (spatial focus) and B closes.
 * `top` pins it to the upper half, clear of Steam's on-screen keyboard.
 */
export function Dialog({
  title,
  children,
  onClose,
  top,
  wide,
  hints,
}: {
  title: string
  children: ReactNode
  onClose: () => void
  top?: boolean
  wide?: boolean
  hints?: Hint[]
}) {
  const close = useRef(onClose)
  close.current = onClose
  useEffect(() => input.pushModal(() => close.current()), [])
  useEffect(() => {
    requestAnimationFrame(focusFirst)
  }, [])
  useHints(hints ?? [{ glyph: 'A', label: tr('Select') }, { glyph: 'B', label: tr('Back') }])
  return (
    <div className={`dialog-backdrop ${top ? 'top' : ''}`}>
      <div className={`dialog ${wide ? 'wide' : ''}`} data-nav-scope>
        <h2 className="dialog-title">{title}</h2>
        {children}
      </div>
    </div>
  )
}

/** One-line text entry pinned to the top; the keyboard opens right away. */
export function TextPrompt({
  title,
  placeholder,
  initial = '',
  submitLabel = tr('Confirm'),
  validate,
  onSubmit,
  onCancel,
  extra,
  busy,
  error,
  prefix,
  hint,
}: {
  title: string
  placeholder?: string
  initial?: string
  submitLabel?: string
  validate?: (v: string) => boolean
  onSubmit: (v: string) => void
  onCancel: () => void
  /** A second way to submit (e.g. "Search without cache"). */
  extra?: { label: string; onSubmit: (v: string) => void }
  /** Submitted and waiting: the text stays, the buttons wait. */
  busy?: boolean
  /** Why the last submit failed, under the field while it holds that text (the text stays to fix). */
  error?: string | null
  /**
   * Fixed text inside the field, before what's typed (e.g. the start of a
   * link, so only its end needs typing), for the text so far; null shows
   * none. Only shown: onSubmit gets what was typed.
   */
  prefix?: (v: string) => string | null
  /** A line under the field on what to type (an error takes its place). */
  hint?: string
}) {
  const [value, setValue] = useState(initial)
  // What was last submitted: an error is about that text, not an edit of it.
  const [sent, setSent] = useState<string | null>(null)
  const field = useRef<HTMLInputElement>(null)
  const ok = validate ? validate(value.trim()) : true
  const pre = prefix?.(value.trim()) ?? null
  const failed = error && sent === value.trim() ? error : null
  const submit = (to: (v: string) => void) => {
    setSent(value.trim())
    to(value.trim())
  }
  useEffect(() => {
    const id = setTimeout(() => field.current && showKeyboard(field.current), 80)
    return () => clearTimeout(id)
  }, [])
  return (
    <Dialog title={title} onClose={onCancel} top>
      <label className={`field-box ${pre ? 'prefixed' : ''}`}>
        {pre && <span className="field-prefix">{pre}</span>}
        <input
          ref={field}
          data-nav
          className="field mono"
          placeholder={placeholder}
          value={value}
          spellCheck={false}
          autoComplete="off"
          onChange={e => setValue(e.target.value)}
          onKeyDown={e => {
            if (e.key === 'Enter' && ok && !busy) submit(onSubmit)
          }}
        />
      </label>
      {failed ? <p className="prompt-error">{failed}</p> : hint && <p className="prompt-hint">{hint}</p>}
      <div className="dialog-actions">
        <button data-nav className="btn" onClick={onCancel}>
          {tr('Cancel')}
        </button>
        {extra && (
          <button data-nav className="btn" disabled={!ok || busy} onClick={() => submit(extra.onSubmit)}>
            {extra.label}
          </button>
        )}
        <button data-nav className="btn primary" disabled={!ok || busy} onClick={() => submit(onSubmit)}>
          {busy && <Spinner />}
          {submitLabel}
        </button>
      </div>
    </Dialog>
  )
}

// ---------- Misc ----------

export function Progress({ value, state }: { value: number; state?: 'done' | 'error' | 'paused' }) {
  return (
    <div className={`progress ${state ?? ''}`}>
      <div style={{ width: `${Math.max(0, Math.min(1, value)) * 100}%` }} />
    </div>
  )
}

export function Spinner() {
  return <span className="spinner" />
}

const ICONS: Record<string, string> = {
  folder: 'M3 6.5A1.5 1.5 0 0 1 4.5 5h4.2l2 2h8.8A1.5 1.5 0 0 1 21 8.5v9a1.5 1.5 0 0 1-1.5 1.5h-15A1.5 1.5 0 0 1 3 17.5z',
  file: 'M6 3h8l4 4v14H6zM14 3v4h4',
  roms: 'M6 9h12a3 3 0 0 1 3 3v2a3 3 0 0 1-3 3h-1.5l-1.8-2H9.3l-1.8 2H6a3 3 0 0 1-3-3v-2a3 3 0 0 1 3-3zM8 11v3M6.5 12.5h3M15.5 12h.01M17.5 13.5h.01',
  sd: 'M7 3h7l4 4v14H7zM10 3v4M12.5 3v4M15 4v3',
  download: 'M12 4v11M7 10l5 5 5-5M5 20h14',
  home: 'M4 11l8-7 8 7v9h-5v-6H9v6H4z',
  network: 'M4 5h16v6H4zM4 13h16v6H4zM7.5 8h.01M7.5 16h.01',
  check: 'M5 12.5l4.5 4.5L19 7.5',
  up: 'M12 19V5M6 11l6-6 6 6',
  search: 'M10.5 4a6.5 6.5 0 1 1 0 13 6.5 6.5 0 0 1 0-13zM15.5 15.5L20 20',
  plus: 'M12 5v14M5 12h14',
  torrent: 'M12 3v10M8 9l4 4 4-4M4 15v4h16v-4',
  gear: 'M12 8.5a3.5 3.5 0 1 1 0 7 3.5 3.5 0 0 1 0-7zM12 2.5v3M12 18.5v3M2.5 12h3M18.5 12h3M5.3 5.3l2.1 2.1M16.6 16.6l2.1 2.1M5.3 18.7l2.1-2.1M16.6 7.4l2.1-2.1',
  pad: 'M7 8h10a4 4 0 0 1 4 4v1a4 4 0 0 1-7 2.6h-4A4 4 0 0 1 3 13v-1a4 4 0 0 1 4-4zM7.5 10.5v3M6 12h3M15.5 11h.01M17.5 13h.01',
  info: 'M12 3a9 9 0 1 1 0 18 9 9 0 0 1 0-18zM12 11v6M12 7.5h.01',
  display: 'M3 5h18v11H3zM8 20h8M12 16v4',
  shield: 'M12 3l7.5 3v5.5c0 4.5-3.2 8-7.5 9.5-4.3-1.5-7.5-5-7.5-9.5V6zM9 12l2 2 4-4',
  library: 'M5 4h3v16H5zM10 4h3v16h-3zM15.4 4.6l2.9-.8 3.6 14.6-2.9.7z',
  prefix: 'M4 7.5l8-4 8 4v9l-8 4-8-4zM4 7.5l8 4 8-4M12 11.5v9',
  trash: 'M4 7h16M10 11v6M14 11v6M6 7l1 13h10l1-13M9 7V4h6v3',
  refresh: 'M19.5 12a7.5 7.5 0 1 1-2.2-5.3M19.5 4v5h-5',
  image: 'M4 5h16v14H4zM4 15.5l4.5-4.5 5 5 2.5-2.5 4 4M15.5 9h.01',
  play: 'M8 5v14l11-7z',
  globe: 'M12 3a9 9 0 1 1 0 18 9 9 0 0 1 0-18zM3.5 9h17M3.5 15h17M12 3c2.4 2.5 3.6 5.5 3.6 9s-1.2 6.5-3.6 9c-2.4-2.5-3.6-5.5-3.6-9s1.2-6.5 3.6-9z',
}

export function Icon({ name, size = 22 }: { name: keyof typeof ICONS | string; size?: number }) {
  return (
    <svg className="icon" width={size} height={size} viewBox="0 0 24 24" aria-hidden>
      <path d={ICONS[name] ?? ICONS.file} />
    </svg>
  )
}
