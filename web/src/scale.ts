// The UI is laid out in 1280-wide pixels, the width SteamOS designs its own
// interface for. On wider screens (the 1920×1080 handhelds, docked TVs) the
// root is zoomed so everything keeps Steam's proportions. The user can pin a
// fixed scale in Settings → Tela.

export const DESIGN_WIDTH = 1280
/** Smallest layout height the screens still fit in (16:9 at 1280 → 720). */
const MIN_HEIGHT = 600
const STORAGE_KEY = 'pishop.uiScale'

export type ScaleSetting = 'auto' | number
export const SCALE_CHOICES: ScaleSetting[] = ['auto', 1.25, 1.5, 1.75]

export function autoScale() {
  return Math.min(window.innerWidth / DESIGN_WIDTH, window.innerHeight / MIN_HEIGHT)
}

export function getScaleSetting(): ScaleSetting {
  try {
    const v = localStorage.getItem(STORAGE_KEY)
    const n = v ? Number(v) : NaN
    return n > 0 ? n : 'auto'
  } catch {
    return 'auto'
  }
}

function currentScale() {
  const s = getScaleSetting()
  // A fixed scale may never push the layout below the minimum size.
  return s === 'auto' ? autoScale() : Math.min(s, autoScale())
}

let rootEl: HTMLElement | null = null

function apply() {
  if (!rootEl) return
  const s = currentScale()
  // Layout size in design pixels; zoom maps it back onto the real window.
  rootEl.style.width = `${window.innerWidth / s}px`
  rootEl.style.height = `${window.innerHeight / s}px`
  rootEl.style.zoom = String(s)
}

export function setScaleSetting(v: ScaleSetting) {
  try {
    if (v === 'auto') localStorage.removeItem(STORAGE_KEY)
    else localStorage.setItem(STORAGE_KEY, String(v))
  } catch {
    // storage unavailable: still applies for this session below
  }
  apply()
  window.dispatchEvent(new Event('pishop-scale'))
}

export function installScaling(root: HTMLElement) {
  rootEl = root
  apply()
  window.addEventListener('resize', apply)
}
