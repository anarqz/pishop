import { useEffect, useRef, useState } from 'react'
import { input } from '../input'
import {
  type TorrentInfo, addMagnet, destroy, forget, formatBytes, mainVideo, pause, resume, streamUrl,
  useEngineState,
} from '../torrent'
import { Icon, Progress, TextPrompt, toast, useHints } from '../ui'

export { useEngineState }

interface Playing {
  url: string
  name: string
}

/** Torrent downloads, shown as a section of the Transfers tab. */
export function TorrentSection({ downloadDir }: { downloadDir?: string }) {
  const engine = useEngineState()
  const [prompt, setPrompt] = useState(false)
  const [playing, setPlaying] = useState<Playing | null>(null)
  const run = (p: Promise<unknown>) => p.catch(e => toast('Ação falhou', String((e as Error).message), 'error'))

  return (
    <section className="torrent-section">
      <div className="torrents-head">
        <div>
          <h3 className="section-title">Downloads (torrent)</h3>
          <p className="settings-desc">
            BitTorrent nativo (TCP/uTP + DHT) · ↓ {formatBytes(engine.downloadSpeed)}/s · ↑{' '}
            {formatBytes(engine.uploadSpeed)}/s · {engine.peers} peers · {(downloadDir ?? '').replace(/^\/home\/[^/]+/, '~')}
          </p>
        </div>
        <div className="row">
          <button data-nav data-nav-default className="btn primary" onClick={() => setPrompt(true)}>
            Adicionar magnet link
          </button>

        </div>
      </div>
      {!engine.online && <p className="muted">Iniciando o motor de torrents…</p>}
      <div className="torrent-list">
        {engine.torrents.map(t => (
          <TorrentRow key={t.id} t={t} run={run} onPlay={setPlaying} />
        ))}
      </div>
      {prompt && (
        <TextPrompt
          title="Magnet link"
          placeholder="magnet:?xt=urn:btih:…"
          submitLabel="Adicionar"
          validate={v => v.startsWith('magnet:')}
          onCancel={() => setPrompt(false)}
          onSubmit={m => {
            setPrompt(false)
            run(addMagnet(m))
          }}
        />
      )}
      {playing && <Player playing={playing} onClose={() => setPlaying(null)} />}
    </section>
  )
}

function TorrentRow({ t, run, onPlay }: { t: TorrentInfo; run: (p: Promise<unknown>) => void; onPlay: (p: Playing) => void }) {
  const video = mainVideo(t)
  const paused = t.state === 'paused'
  const status = t.error
    ? `Erro: ${t.error}`
    : t.finished
      ? 'Concluído'
      : paused
        ? 'Pausado'
        : t.state === 'initializing'
          ? 'Verificando…'
          : `${(t.progress * 100).toFixed(1)}%`
  return (
    <div className="torrent-card">
      <div className="job-line">
        <b className="job-name">{t.name}</b>
        <span className={t.error ? 'error' : t.finished ? 'ok' : ''}>{status}</span>
      </div>
      <Progress value={t.progress} state={t.finished ? 'done' : t.error ? 'error' : paused ? 'paused' : undefined} />
      <div className="job-sub">
        {formatBytes(t.totalBytes)} · ↓ {formatBytes(t.downloadSpeed)}/s · ↑ {formatBytes(t.uploadSpeed)}/s · {t.peers} peers
        · {t.files.length} arquivos
      </div>
      <div className="row compact">
        {video && (
          <button data-nav className="btn small" onClick={() => onPlay({ url: streamUrl(t, video), name: video.name })}>
            Assistir
          </button>
        )}
        <button data-nav className="btn small" onClick={() => run(paused ? resume(t.id) : pause(t.id))}>
          {paused ? 'Retomar' : 'Pausar'}
        </button>
        <button data-nav className="btn small" onClick={() => run(forget(t.id))}>
          Remover
        </button>
        <button data-nav className="btn small danger" onClick={() => run(destroy(t.id))}>
          Apagar arquivos
        </button>
      </div>
    </div>
  )
}

function Player({ playing, onClose }: { playing: Playing; onClose: () => void }) {
  const ref = useRef<HTMLVideoElement>(null)
  const [paused, setPaused] = useState(false)
  const close = useRef(onClose)
  close.current = onClose
  useEffect(() => input.pushModal(() => close.current()), [])
  useEffect(() => {
    requestAnimationFrame(() => document.querySelector<HTMLElement>('.player-bar [data-nav-default]')?.focus())
  }, [])
  useHints([
    { glyph: 'A', label: 'Selecionar' },
    { glyph: 'B', label: 'Fechar' },
  ])
  const seek = (d: number) => {
    const v = ref.current
    if (v) v.currentTime = Math.max(0, v.currentTime + d)
  }
  const toggle = () => {
    const v = ref.current
    if (!v) return
    if (v.paused) void v.play()
    else v.pause()
  }
  return (
    <div className="player">
      <video ref={ref} src={playing.url} autoPlay onPlay={() => setPaused(false)} onPause={() => setPaused(true)} />
      <div className="player-bar" data-nav-scope>
        <Icon name="torrent" />
        <span className="player-name">{playing.name}</span>
        <button data-nav className="btn small" onClick={() => seek(-10)}>
          −10s
        </button>
        <button data-nav data-nav-default className="btn small primary" onClick={toggle}>
          {paused ? 'Play' : 'Pausa'}
        </button>
        <button data-nav className="btn small" onClick={() => seek(30)}>
          +30s
        </button>
        <button data-nav className="btn small" onClick={onClose}>
          Fechar
        </button>
      </div>
    </div>
  )
}
