# piShop — context for AI assistants

piShop is a game store and manager for **SteamOS Game Mode** (Steam Deck, ROG
Ally and other handhelds). It runs as a non-Steam game: a Rust launcher serves a
React UI and shows it fullscreen in a bundled Chromium, used entirely with the
controller. Users discover, download (BitTorrent and direct links), install
(Proton prefixes, components, patches) and manage games without leaving Game
Mode, and the **Community** tab installs games through recipes kept in this repo.

Human contributors read [CONTRIBUTING.md](CONTRIBUTING.md); this file is the
same ground for an AI assistant. Two task guides (skills) go step by step:

| Task | Skill |
|---|---|
| Add or update a game in the Community tab (`community/`) | [`.claude/skills/add-community-game/SKILL.md`](.claude/skills/add-community-game/SKILL.md) |
| Build or change a feature in the app (`launcher/`, `web/`) | [`.claude/skills/pishop-feature/SKILL.md`](.claude/skills/pishop-feature/SKILL.md) |

Load the matching skill before starting either kind of task.

## Repository

```
launcher/   Rust (edition 2024): axum API on 127.0.0.1:47800, librqbit torrents,
            direct downloads, Steam integration, installs; embeds web/dist
web/        React 19 + Vite + TypeScript: the UI (a PWA), controller-first
community/  Community recipes: <id>/manifest.json + install.sh; index.json is generated
scripts/    build.sh (SteamOS bundle), deploy.sh (copy to a device), community.mjs
            (validate + bundle the catalog), screenshots/site-shots (site images)
docs/       Website (GitHub Pages from main:/docs) and install.sh
.github/    CI (community check, releases), Copilot review rules for community/
```

## Commands

```bash
(cd web && pnpm install && pnpm build)     # translation check + type check + bundle
(cd web && pnpm i18n)                      # also lists translations no longer used
(cd launcher && cargo test)                # unit tests
node scripts/community.mjs                 # validate community/ and rewrite index.json
node scripts/community.mjs --check         # what CI runs on community/ changes

# Run locally (UI work; Steam features need SteamOS): build web first, then
(cd launcher && cargo run -- --no-browser) # API + UI on http://127.0.0.1:47800
(cd web && pnpm dev)                       # optional: Vite with hot reload, /api proxied
```

A second instance next to a running one needs **both** `PISHOP_PORT=<port>` and
`XDG_DATA_HOME=<scratch dir>`; sharing the data dir breaks the real one
(torrent engine port, settings). State lives in `$XDG_DATA_HOME/piShop/`
(`~/.local/share/piShop/` by default).

## Rules that apply to every change

- **Two languages.** UI text is English in the code — `tr('View game')`,
  `tr('{n} games', { n })` — with its Portuguese in `web/src/locales/pt/*.json`,
  keyed by the English text; `pnpm build` fails on a missing one. Launcher
  messages the user sees use `tr!("English", "Português")`. Launcher log lines
  (`log!`) are Portuguese, `area: message`. Code comments are English.
- **Controller first.** Everything must work with a gamepad: interactive
  elements carry `data-nav`, a screen marks its default focus, B goes back,
  the footer shows what the buttons do (`useHints`). L1/R1 always switch tabs.
- **No secrets, no copyrighted game data** in the repo, logs or screenshots.
  Service endpoints and API keys come from Settings, never from the code.
- **Games only.** piShop downloads and installs games; no media playback.
- **Game metadata priority**, field by field: Steam (with an app id) →
  TheGamesDB → SteamGridDB → isitcracked.
- **Moving a game moves its shortcut.** Anything that relocates game files also
  updates the Steam shortcut (target and "Start in", double-quoted) and
  registry paths, through the existing after-move step in `install.rs`.
- **Storage is always visible.** Wherever piShop names a place on disk, show its
  free space (the small ring used across the app).
- **Failures keep the input.** A prompt whose action fails stays open with the
  user's text and the reason under it (`TextPrompt` `busy`/`error`).
- **Match the code around you**: naming, comment density (comments explain
  *why*), file layout. Prefer extending an existing module over a new one, and
  the standard library or an existing dependency over a new crate/package.
- **Releases are the maintainer's.** Don't bump versions, tag, or edit
  `community/index.json` by hand (it's generated).

## Checks before calling it done

`pnpm build` and `cargo test` pass; `node scripts/community.mjs --check`
passes when `community/` changed; new UI strings have Portuguese; the feature
was tried — on a SteamOS device when it touches Steam, Proton or the
controller — or the pull request says what wasn't tried.
