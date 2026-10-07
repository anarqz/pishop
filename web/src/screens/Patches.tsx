// Games → Patches: a panel on the right with quick workarounds to try when a
// game won't run on Linux. Each patch is on or off as the shortcut's launch
// options say (edits made in Steam show here too); switching one rewrites
// only its own part of them, and it applies the next time the game starts.

import { useEffect, useRef, useState } from 'react'
import { type PatchList, api } from '../api'
import { tr } from '../i18n'
import { focusFirst, input } from '../input'
import { Spinner, toast, useHints } from '../ui'

export default function PatchesPanel({ appid, name, onClose }: { appid: number; name: string; onClose: () => void }) {
  const [state, setState] = useState<PatchList | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState<string | null>(null)

  useEffect(() => {
    api
      .patches(appid)
      .then(setState)
      .catch(e => setError((e as Error).message))
  }, [appid])

  const close = useRef(onClose)
  close.current = onClose
  useEffect(() => input.pushModal(() => close.current()), [])
  useEffect(() => {
    if (state || error) requestAnimationFrame(focusFirst)
  }, [!!state, !!error])
  useHints([
    { glyph: 'A', label: tr('Turn on / off') },
    { glyph: 'B', label: tr('Close') },
  ])

  const toggle = (id: string, title: string, on: boolean) => {
    setBusy(id)
    api
      .setPatch(appid, id, on)
      .then(s => {
        setState(s)
        toast(on ? tr('Patch applied') : tr('Patch removed'), title, 'ok')
      })
      .catch(e => toast(tr('Something went wrong'), (e as Error).message, 'error'))
      .finally(() => setBusy(null))
  }

  return (
    <div className="aside-backdrop" onClick={onClose}>
      <aside className="aside" data-nav-scope onClick={e => e.stopPropagation()}>
        <span className="hero-kicker">{tr('Patches')}</span>
        <h2>{name}</h2>
        <p className="muted">
          {tr('Quick workarounds to try when a game won’t run on Linux. Each one changes the game’s launch options; turning it off undoes just that.')}
        </p>
        <div className="patch-opts">
          <small>{tr('Launch options')}</small>
          <code>{state ? state.launch_options || tr('(none)') : '…'}</code>
        </div>
        {error && <p className="inst-error">{error}</p>}
        {!state && !error && (
          <p className="muted">
            <Spinner /> {tr('Loading…')}
          </p>
        )}
        {state?.busy && <p className="inst-error">{tr('An installer is using this shortcut: patches wait for it to close.')}</p>}
        <div className="patch-list">
          {state?.patches.map((p, i) => (
            <button
              key={p.id}
              data-nav
              data-nav-default={i === 0 ? '' : undefined}
              className={`patch ${p.applied ? 'on' : ''}`}
              disabled={!!busy || state.busy}
              onClick={() => toggle(p.id, p.title, !p.applied)}
            >
              <span className="patch-switch" aria-hidden>
                <span />
              </span>
              <span className="patch-main">
                <b>{p.title}</b>
                <small>{p.about}</small>
              </span>
              <span className="patch-state">{busy === p.id ? <Spinner /> : p.applied ? tr('Applied') : tr('Not applied')}</span>
            </button>
          ))}
        </div>
        <p className="muted small">{tr('Changes apply the next time the game starts.')}</p>
      </aside>
    </div>
  )
}
