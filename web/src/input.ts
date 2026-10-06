// Unified input layer: Gamepad API (Steam Input exposes the Deck controls as a
// standard-mapping virtual Xbox pad), keyboard (desktop / Steam keyboard
// templates) and spatial focus navigation over elements marked `data-nav`.
//
// Screens that need custom control (the file explorer, dialogs) push a
// handler. Handlers see actions first, top of the stack first:
//   true  → consumed;
//   false → try the next handler, then default behaviour;
//   'nav' → modal: only default spatial navigation runs (no lower handlers,
//           no global listeners such as tab switching).

export type Action =
  | 'up' | 'down' | 'left' | 'right'
  | 'confirm' | 'back' | 'x' | 'y'
  | 'lb' | 'rb' | 'lt' | 'rt'
  | 'menu' | 'view' | 'quit'

export type Handler = (a: Action) => boolean | 'nav'

type Listener<T> = (value: T) => void

const REPEAT_DELAY = 320
const REPEAT_RATE = 70
/** Held directions speed up after a while, for long lists. */
const FAST_AFTER = 1200
const FAST_RATE = 30
const STICK_THRESHOLD = 0.5
const QUIT_HOLD_MS = 1000
/** Right stick: dead zone and top speed (design px per second). */
const SCROLL_DEADZONE = 0.18
const SCROLL_MAX_SPEED = 1800

const DIRECTIONS: Array<[Action, (p: Gamepad) => boolean]> = [
  ['up', p => pressed(p, 12) || (p.axes[1] ?? 0) < -STICK_THRESHOLD],
  ['down', p => pressed(p, 13) || (p.axes[1] ?? 0) > STICK_THRESHOLD],
  ['left', p => pressed(p, 14) || (p.axes[0] ?? 0) < -STICK_THRESHOLD],
  ['right', p => pressed(p, 15) || (p.axes[0] ?? 0) > STICK_THRESHOLD],
]

/** Buttons that fire once per press (L2/R2 repeat, like directions). */
const EDGE_BUTTONS: Array<[number, Action]> = [
  [0, 'confirm'],
  [1, 'back'],
  [2, 'x'],
  [3, 'y'],
  [4, 'lb'],
  [5, 'rb'],
  [8, 'view'],
  [9, 'menu'],
]

function pressed(p: Gamepad, i: number) {
  return p.buttons[i]?.pressed ?? false
}

class InputManager {
  private actionListeners = new Set<Listener<Action>>()
  private handlers: Handler[] = []
  private scrollHooks: Array<(dy: number) => boolean> = []
  private lastPoll = 0
  private prevButtons = new Map<number, boolean[]>()
  private heldSince = new Map<string, number>()
  private lastRepeat = new Map<string, number>()
  private quitHoldStart = 0
  private quitFired = false
  private running = false

  start() {
    if (this.running) return
    this.running = true
    window.addEventListener('keydown', this.onKey)
    const loop = () => {
      this.poll()
      requestAnimationFrame(loop)
    }
    requestAnimationFrame(loop)
  }

  /** Global listeners (tab switching, quit) — after handlers and navigation. */
  onAction(fn: Listener<Action>) {
    this.actionListeners.add(fn)
    return () => void this.actionListeners.delete(fn)
  }

  pushHandler(fn: Handler) {
    this.handlers.push(fn)
    return () => {
      this.handlers = this.handlers.filter(h => h !== fn)
    }
  }

  /**
   * Claims right-stick scrolling (e.g. the explorer's virtual list moves its
   * cursor instead). `dy` is in design pixels for this frame.
   */
  pushScroll(fn: (dy: number) => boolean) {
    this.scrollHooks.push(fn)
    return () => {
      this.scrollHooks = this.scrollHooks.filter(h => h !== fn)
    }
  }

  private scroll(dy: number) {
    for (let i = this.scrollHooks.length - 1; i >= 0; i--) {
      if (this.scrollHooks[i](dy)) return
    }
    const target = scrollTargetForStick()
    if (target) target.scrollTop += dy
  }

  /**
   * Modal layer: B runs `onBack`, everything else is plain spatial navigation.
   * Full-screen pages pass `tabs` so L1/R1 still switch tabs from them.
   */
  pushModal(onBack: () => void, opts: { tabs?: boolean } = {}) {
    return this.pushHandler(a => {
      if (a === 'back') {
        onBack()
        return true
      }
      if (opts.tabs && (a === 'lb' || a === 'rb')) return false
      return 'nav'
    })
  }

  /** Fraction (0..1) of the View+Menu quit hold, for UI feedback. */
  quitProgress() {
    if (!this.quitHoldStart) return 0
    return Math.min(1, (performance.now() - this.quitHoldStart) / QUIT_HOLD_MS)
  }

  emit(a: Action) {
    if (a === 'quit') {
      this.actionListeners.forEach(fn => fn(a))
      return
    }
    let modal = false
    for (let i = this.handlers.length - 1; i >= 0; i--) {
      const r = this.handlers[i](a)
      if (r === true) return
      if (r === 'nav') {
        modal = true
        break
      }
    }
    if (this.handleNavigation(a)) return
    if (!modal) this.actionListeners.forEach(fn => fn(a))
  }

  private onKey = (e: KeyboardEvent) => {
    const typing = e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement
    const map: Record<string, Action | undefined> = {
      ArrowUp: 'up', ArrowDown: 'down', ArrowLeft: typing ? undefined : 'left',
      ArrowRight: typing ? undefined : 'right', Enter: typing ? undefined : 'confirm',
      Escape: 'back', Backspace: typing ? undefined : 'back',
      PageUp: 'lt', PageDown: 'rt',
    }
    if (!typing) {
      Object.assign(map, { x: 'x', y: 'y', q: 'lb', e: 'rb', m: 'menu', v: 'view' })
    }
    const a = map[e.key]
    if (!a) return
    e.preventDefault()
    this.emit(a)
  }

  private poll() {
    const pads = (navigator.getGamepads?.() ?? []).filter((p): p is Gamepad => !!p && p.connected)
    const now = performance.now()
    const dt = Math.min(0.05, (now - (this.lastPoll || now)) / 1000)
    this.lastPoll = now

    // Right stick vertical (axis 3): continuous, speed proportional to tilt.
    let stick = 0
    for (const p of pads) {
      const v = p.axes[3] ?? 0
      if (Math.abs(v) > Math.abs(stick)) stick = v
    }
    if (Math.abs(stick) > SCROLL_DEADZONE && dt > 0) {
      const mag = (Math.abs(stick) - SCROLL_DEADZONE) / (1 - SCROLL_DEADZONE)
      this.scroll(Math.sign(stick) * mag * mag * SCROLL_MAX_SPEED * dt)
    }

    let quitHeld = false
    for (const p of pads) {
      const prev = this.prevButtons.get(p.index) ?? []
      const combo = pressed(p, 8) && pressed(p, 9)
      for (const [i, action] of EDGE_BUTTONS) {
        if (!pressed(p, i) || prev[i]) continue
        // View/Menu presses that are part of the quit combo don't count.
        if ((action === 'view' || action === 'menu') && combo) continue
        this.emit(action)
      }
      const held: Array<[Action, boolean]> = [
        ...DIRECTIONS.map(([a, test]) => [a, test(p)] as [Action, boolean]),
        ['lt', pressed(p, 6)],
        ['rt', pressed(p, 7)],
      ]
      for (const [action, down] of held) {
        const key = `${p.index}:${action}`
        if (!down) {
          this.heldSince.delete(key)
          continue
        }
        const since = this.heldSince.get(key)
        if (since === undefined) {
          this.heldSince.set(key, now)
          this.lastRepeat.set(key, now)
          this.emit(action)
          continue
        }
        const rate = now - since > FAST_AFTER ? FAST_RATE : REPEAT_RATE
        if (now - since > REPEAT_DELAY && now - (this.lastRepeat.get(key) ?? 0) > rate) {
          this.lastRepeat.set(key, now)
          this.emit(action)
        }
      }
      if (combo) quitHeld = true
      this.prevButtons.set(p.index, p.buttons.map(b => b.pressed))
    }

    if (quitHeld) {
      if (!this.quitHoldStart) this.quitHoldStart = now
      if (!this.quitFired && now - this.quitHoldStart >= QUIT_HOLD_MS) {
        this.quitFired = true
        this.emit('quit')
      }
    } else {
      this.quitHoldStart = 0
      this.quitFired = false
    }
  }

  /** Returns true when the action was consumed by focus handling. */
  private handleNavigation(a: Action): boolean {
    if (a === 'confirm') {
      const el = document.activeElement as HTMLElement | null
      if (el && el !== document.body && el.matches('[data-nav]')) {
        if (el instanceof HTMLInputElement) showKeyboard(el)
        else el.click()
        return true
      }
      return false
    }
    if (a === 'up' || a === 'down' || a === 'left' || a === 'right') {
      return moveFocus(a)
    }
    return false
  }
}

/**
 * Focuses a text field and asks the launcher to open Steam's on-screen
 * keyboard. The browser runs without Steam's input-method module, so focus
 * changes never open (or reopen) the keyboard on their own.
 */
export function showKeyboard(el: HTMLInputElement | HTMLTextAreaElement) {
  el.focus()
  void fetch('/api/keyboard', { method: 'POST' }).catch(() => {})
}

function focusables(): HTMLElement[] {
  // The most recently mounted scope (e.g. an open dialog) traps navigation.
  const scopes = document.querySelectorAll<HTMLElement>('[data-nav-scope]')
  const scope = scopes[scopes.length - 1] ?? document.body
  return [...scope.querySelectorAll<HTMLElement>('[data-nav]')].filter(
    el => !(el as HTMLButtonElement).disabled && el.offsetParent !== null,
  )
}

/**
 * Remembers what has focus now; the returned function puts focus back there
 * (e.g. on the card that opened an overlay), or on the default if it's gone.
 */
export function keepFocus(): () => void {
  const el = document.activeElement instanceof HTMLElement ? document.activeElement : null
  return () =>
    requestAnimationFrame(() => {
      if (el?.isConnected && el.matches('[data-nav]')) {
        el.focus({ preventScroll: true })
        ensureVisible(el)
      } else focusFirst()
    })
}

export function focusFirst() {
  const els = focusables()
  const preferred = els.find(el => el.dataset.navDefault !== undefined) ?? els[0]
  if (!preferred) return
  preferred.focus({ preventScroll: true })
  ensureVisible(preferred)
}

function isScrollable(c: HTMLElement) {
  const oy = getComputedStyle(c).overflowY
  return (oy === 'auto' || oy === 'scroll') && c.scrollHeight > c.clientHeight + 1
}

function scrollParentX(el: HTMLElement): HTMLElement | null {
  for (let c = el.parentElement; c; c = c.parentElement) {
    const ox = getComputedStyle(c).overflowX
    if ((ox === 'auto' || ox === 'scroll') && c.scrollWidth > c.clientWidth + 1) return c
  }
  return null
}

/** Horizontal strips (Discover rows): same rules as vertical, on the x axis. */
function ensureVisibleX(el: HTMLElement) {
  const c = scrollParentX(el)
  if (!c) return
  const r = el.getBoundingClientRect()
  const cr = c.getBoundingClientRect()
  const z = c.clientWidth ? cr.width / c.clientWidth || 1 : 1
  const margin = 48 * z
  if (r.left < cr.left + margin) c.scrollLeft -= (cr.left + margin - r.left) / z
  else if (r.right > cr.right - margin) c.scrollLeft += (r.right - (cr.right - margin)) / z
}

function scrollParent(el: HTMLElement): HTMLElement | null {
  for (let c = el.parentElement; c; c = c.parentElement) {
    if (isScrollable(c)) return c
  }
  return null
}

/** What the right stick scrolls: the focused element's area, else the screen's. */
function scrollTargetForStick(): HTMLElement | null {
  const active = document.activeElement as HTMLElement | null
  const fromFocus = active && active !== document.body ? scrollParent(active) : null
  if (fromFocus) return fromFocus
  const scopes = document.querySelectorAll<HTMLElement>('[data-nav-scope]')
  const scope = scopes[scopes.length - 1]
  if (!scope) return null
  if (isScrollable(scope)) return scope
  return [...scope.querySelectorAll<HTMLElement>('*')].find(isScrollable) ?? null
}

/** Visual px per layout px of a scroll container (the UI root is zoomed). */
function zoomOf(c: HTMLElement) {
  return c.clientHeight ? c.getBoundingClientRect().height / c.clientHeight || 1 : 1
}

function inView(el: HTMLElement, c: HTMLElement) {
  const r = el.getBoundingClientRect()
  const cr = c.getBoundingClientRect()
  return r.bottom > cr.top + 4 && r.top < cr.bottom - 4
}

/**
 * Scrolls `el` into its container with a margin. At the extremes (nothing
 * focusable above/below it) the container goes all the way to the top/bottom,
 * so non-focusable content (headers, "load more" markers) shows up too.
 */
export function ensureVisible(el: HTMLElement) {
  ensureVisibleX(el)
  const c = scrollParent(el)
  if (!c) return
  const r = el.getBoundingClientRect()
  const others = [...c.querySelectorAll<HTMLElement>('[data-nav]')].filter(o => o !== el && o.offsetParent !== null)
  const noneAbove = !others.some(o => o.getBoundingClientRect().bottom <= r.top + 1)
  const noneBelow = !others.some(o => o.getBoundingClientRect().top >= r.bottom - 1)
  if (noneAbove) {
    c.scrollTop = 0
    return
  }
  if (noneBelow) {
    c.scrollTop = c.scrollHeight
    return
  }
  const cr = c.getBoundingClientRect()
  const z = zoomOf(c)
  const margin = 24 * z
  if (r.top < cr.top + margin) c.scrollTop -= (cr.top + margin - r.top) / z
  else if (r.bottom > cr.bottom - margin) c.scrollTop += (r.bottom - (cr.bottom - margin)) / z
}

/** Returns true when focus moved (or was placed for the first time). */
/** The closest element in a direction: distance along it, sideways offset weighs double. */
function nearest(current: HTMLElement, dir: 'up' | 'down' | 'left' | 'right', els: HTMLElement[]): HTMLElement | null {
  const r = current.getBoundingClientRect()
  const cx = r.left + r.width / 2
  const cy = r.top + r.height / 2
  let best: HTMLElement | null = null
  let bestScore = Infinity
  for (const el of els) {
    if (el === current) continue
    const o = el.getBoundingClientRect()
    const dx = o.left + o.width / 2 - cx
    const dy = o.top + o.height / 2 - cy
    const primary = dir === 'left' ? -dx : dir === 'right' ? dx : dir === 'up' ? -dy : dy
    const secondary = dir === 'left' || dir === 'right' ? Math.abs(dy) : Math.abs(dx)
    if (primary <= 1) continue
    const score = primary + secondary * 2
    if (score < bestScore) {
      bestScore = score
      best = el
    }
  }
  return best
}

function moveFocus(dir: 'up' | 'down' | 'left' | 'right'): boolean {
  const els = focusables()
  if (!els.length) return false
  const current = document.activeElement as HTMLElement | null
  if (!current || !els.includes(current)) {
    focusFirst()
    return true
  }
  // The right stick may have scrolled the focused element away: resume from
  // what is on screen instead of jumping back to it.
  const area = scrollParent(current)
  if (area && !inView(current, area)) {
    const cr = area.getBoundingClientRect()
    const mid = (cr.top + cr.bottom) / 2
    const visible = els.filter(e => area.contains(e) && inView(e, area))
    visible.sort((a, b) => Math.abs(a.getBoundingClientRect().top - mid) - Math.abs(b.getBoundingClientRect().top - mid))
    if (visible[0]) {
      visible[0].focus({ preventScroll: true })
      ensureVisible(visible[0])
      return true
    }
  }
  // Explicit links win over geometry (data-nav-up/down/left/right="selector"),
  // e.g. a centered search bar between left-aligned buttons and a grid.
  // "none" keeps focus where it is.
  const link = current.dataset[`nav${dir[0].toUpperCase()}${dir.slice(1)}`]
  if (link === 'none') return true
  if (link) {
    const target = document.querySelector<HTMLElement>(link)
    if (target && els.includes(target)) {
      target.focus({ preventScroll: true })
      ensureVisible(target)
      return true
    }
  }
  // A container can route a direction that leaves it:
  // data-nav-exit-left=".settings-tab.active" (or "none" to stay inside).
  const box = current.parentElement?.closest<HTMLElement>(`[data-nav-exit-${dir}]`)
  if (box) {
    let target = nearest(current, dir, els.filter(e => box.contains(e)))
    if (!target) {
      const exit = box.getAttribute(`data-nav-exit-${dir}`)!
      if (exit === 'none') return true
      const t = document.querySelector<HTMLElement>(exit)
      target = t && els.includes(t) ? t : null
    }
    if (target) {
      target.focus({ preventScroll: true })
      ensureVisible(target)
      return true
    }
  }
  const best = nearest(current, dir, els)
  if (best) {
    best.focus({ preventScroll: true })
    ensureVisible(best)
    return true
  }
  return false
}

export const input = new InputManager()
