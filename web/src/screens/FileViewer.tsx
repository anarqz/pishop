// Reads a text file from Explore: Markdown (a README.md) formatted, other text
// (.txt, .ini, .log, .json…) as it is. Files come from anywhere — a network
// share included — and this page can reach piShop's whole API, so Markdown's
// raw HTML shows as text, and its links and images load nothing.

import MarkdownIt from 'markdown-it'
import { useEffect, useMemo, useRef, useState } from 'react'
import { api, formatBytes } from '../api'
import { tr } from '../i18n'
import { input } from '../input'
import { Icon, Spinner, useHints } from '../ui'

const MARKDOWN = /\.(md|markdown|mdown)$/i
const TEXT = /\.(txt|text|nfo|diz|ini|cfg|conf|config|log|json|jsonc|xml|ya?ml|toml|csv|tsv|sh|bash|bat|cmd|ps1|lua|py|js|ts|css|html?|reg|url|desktop|properties|srt|ass|cue|m3u8?|gitignore|license)$/i
const TEXT_NAMES = /^(readme|license|licence|copying|changelog|changes|authors|credits|notice|install)$/i

/** Files A opens in the viewer instead of marking them. */
export const previewable = (name: string) => MARKDOWN.test(name) || TEXT.test(name) || TEXT_NAMES.test(name)

const md = new MarkdownIt({ html: false, linkify: true })
// Images load nothing: their description stands in.
md.renderer.rules.image = (tokens, idx) => `<span class="md-img">${md.utils.escapeHtml(tokens[idx].content || tr('image'))}</span>`
// Links are shown, not followed: the full-screen browser has no way back.
md.renderer.rules.link_open = (tokens, idx) => `<span class="md-link" title="${md.utils.escapeHtml(tokens[idx].attrGet('href') ?? '')}">`
md.renderer.rules.link_close = () => '</span>'

const STEP = 90

export default function FileViewer({
  name,
  path,
  source,
  onClose,
}: {
  name: string
  path: string
  /** A game source's id (network storage); null for this device. */
  source: string | null
  onClose: () => void
}) {
  const [file, setFile] = useState<{ text: string; size: number; truncated: boolean } | null>(null)
  const [error, setError] = useState<string | null>(null)
  const body = useRef<HTMLDivElement>(null)

  useEffect(() => {
    let alive = true
    ;(source ? api.readRemote(source, path) : api.readLocal(path))
      .then(f => alive && setFile(f))
      .catch(e => alive && setError((e as Error).message))
    return () => {
      alive = false
    }
  }, [source, path])

  // The controller reads: D-pad and right stick scroll, L2/R2 a page, B closes.
  const close = useRef(onClose)
  close.current = onClose
  useEffect(() => {
    const by = (dy: number) => body.current?.scrollBy({ top: dy, behavior: 'smooth' })
    const page = () => (body.current?.clientHeight ?? 400) * 0.85
    const off = input.pushHandler(a => {
      switch (a) {
        case 'back':
          close.current()
          return true
        case 'up':
          by(-STEP)
          return true
        case 'down':
          by(STEP)
          return true
        case 'lt':
          by(-page())
          return true
        case 'rt':
          by(page())
          return true
        default:
          return 'nav'
      }
    })
    const offScroll = input.pushScroll(dy => {
      if (body.current) body.current.scrollTop += dy
      return true
    })
    return () => {
      off()
      offScroll()
    }
  }, [])
  useHints([
    { glyph: 'DPAD', label: tr('Scroll') },
    { glyph: ['L2', 'R2'], label: tr('Page') },
    { glyph: 'B', label: tr('Back') },
  ])

  const markdown = MARKDOWN.test(name)
  const html = useMemo(() => (file && markdown ? md.render(file.text) : ''), [file, markdown])
  const where = path.replace(/^\/home\/[^/]+/, '~')

  return (
    <div className="viewer" data-nav-scope>
      <header className="viewer-head">
        <Icon name="file" size={26} />
        <div>
          <h2>{name}</h2>
          <small>
            {where}
            {file ? ` · ${formatBytes(file.size)}` : ''}
          </small>
        </div>
      </header>
      <div className="viewer-body" ref={body}>
        {error ? (
          <p className="error">{error}</p>
        ) : !file ? (
          <p className="muted">
            <Spinner /> {tr('Loading…')}
          </p>
        ) : markdown ? (
          <div className="md" dangerouslySetInnerHTML={{ __html: html }} />
        ) : (
          <pre className="viewer-pre">{file.text}</pre>
        )}
        {file?.truncated && <p className="muted small">{tr('Showing the first 1 MB of the file.')}</p>}
      </div>
    </div>
  )
}
