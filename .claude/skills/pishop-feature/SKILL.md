---
name: pishop-feature
description: Build, change or fix something in the piShop app — the Rust launcher (launcher/) or the React UI (web/): screens, API endpoints, settings, downloads, installs, Steam integration, translations. Use for any code change outside community/ recipes.
---

# Build a feature in piShop

Read `CLAUDE.md` first (repository map, commands, rules for every change).
This guide is how the app is put together and how a change goes from idea to
pull request.

## How piShop works

- **One process for Steam.** Steam launches `pishop` as a non-Steam game. The
  launcher starts an axum server on `127.0.0.1:47800` (`PISHOP_PORT` for
  development), opens the UI in a bundled Chromium (kiosk, fullscreen) and
  ties both lifetimes together (`main.rs`, `browser.rs`). Under gamescope only
  processes under Steam's reaper get a window, so anything with its own UI
  (an installer, a game) is started *through Steam*.
- **UI embedded in the binary.** `web/dist` is built by Vite and embedded with
  rust-embed (`server.rs`). The UI is a PWA; the launcher purges its service
  worker cache when the embedded UI changes.
- **API.** Every feature module exposes `pub fn router() -> Router`, merged in
  `api.rs`. Handlers answer JSON, or `{"error": "..."}` with a 4xx/5xx; most use
  `install::reply(anyhow::Result<T>)`.
- **State** lives in JSON files under `data_dir()` (`$XDG_DATA_HOME/piShop`):
  `settings.json`, `library.json`, `direct.json`, `community/installs.json`…
  Each module reads and writes its own file through small load/store helpers.
  The launcher log is `launcher.log` there.
- **Steam integration.** `steam.rs` + `vdf.rs` edit `shortcuts.vdf` directly
  (piShop's own registration, `--install-steam`); Steam only reads that file at
  startup, so this needs a Steam restart. Everything at runtime goes through
  `steamclient.rs`, which drives Steam's own client API live over its CEF
  debugging port (127.0.0.1:8080, enabled by
  `~/.local/share/Steam/.cef-enable-remote-debugging`): add shortcuts, set
  artwork and Proton, launch apps. `focus.rs` reads gamescope's focused app so
  input is ignored while a game is in front.
- **Device.** SteamOS Game Mode on handhelds with 1280×800 (Steam Deck) or
  1920×1080 (ROG Ally) screens. The UI is laid out in 1280-wide pixels and
  scaled (`web/src/scale.ts`). The controller is the only input most users have.

## Where things live

| Area | Launcher (`launcher/src/`) | UI (`web/src/`) |
|---|---|---|
| Discover | `discover.rs`, `steam_store.rs`, `tgdb.rs`, `trailer.rs` | `screens/Discover.tsx`, `GameDetail.tsx` |
| Community | `community.rs` | `screens/Community.tsx`, `CommunityInstall.tsx`, `community.ts` |
| Store | `catalog.rs` (search, SteamGridDB art, image proxy), `tpb.rs`, `titles.rs` | `screens/Store.tsx` |
| Transfers | `torrent.rs`, `direct.rs`, `jobs.rs` (copies), `library.rs` | `screens/Jobs.tsx`, `Torrents.tsx`, `TransferTiles.tsx`, `torrent.ts` |
| Install | `install.rs` (wizard, shortcut, after-move), `archive.rs`, `exeguess.rs`, `redist.rs`, `winetricks.rs`, `archivetools.rs` | `screens/Install.tsx` |
| Games | `games.rs`, `patches.rs`, `compat.rs`, `proton.rs`, `relocate.rs` | `screens/Games.tsx`, `GameSetup.tsx`, `Patches.tsx`, `Compat.tsx` |
| Explore / files | `localfs.rs`, `sources.rs`, `smbfs.rs` | `screens/Explorer.tsx`, `FilePicker.tsx`, `DirPicker.tsx`, `storage.tsx` |
| Settings | `settings.rs`, `services_file.rs`, `vpn.rs`, `update.rs` | `screens/Settings.tsx` |
| Shell | `main.rs`, `server.rs`, `browser.rs`, `focus.rs`, `log.rs` | `App.tsx` (tabs), `ui.tsx`, `input.ts`, `i18n.tsx`, `api.ts` |

Every file opens with a comment saying what it is; read it before changing the file.

## Workflow

1. **Pin down the behaviour** with the human: which tab, what the user does
   with the controller, what happens on failure. Check it against the product
   rules below; ask when it conflicts.
2. **Find the closest existing feature** and follow its pattern end to end
   (a new download source → `direct.rs`; a new game action → `patches.rs` +
   `Patches.tsx`; a new setting → `settings.rs` + `Settings.tsx`).
3. **Launcher**
   - Put the code in the module that owns the area (a new module only for a
     new area; then `mod` it in `main.rs` and `.merge(x::router())` in `api.rs`).
   - Errors: `anyhow` with context the user can act on; user-visible text via
     `tr!("English", "Português", args…)` (`crate::tr`).
   - Log what helps afterwards: `log!("area: ...")`, in Portuguese like the
     rest of `launcher.log`.
   - Network: the module's shared `reqwest` client, with connect/read timeouts;
     never an unbounded wait. Heavy CPU or blocking IO: `spawn_blocking`.
   - Long work (downloads, copies, installs) runs in the background with
     progress the UI polls, can be paused/cancelled, and resumes after a
     restart.
   - Unit tests in the `#[cfg(test)] mod tests` at the bottom of the file.
4. **UI**
   - Types and calls in `api.ts` (`call<T>(url)` / `post<T>(url, body)`).
   - Build screens from `ui.tsx` (`Dialog`, `TextPrompt` with `busy`/`error`,
     `toast`, `Progress`, `Spinner`, `Icon`, `useHints`) and `storage.tsx`
     (free space ring) before writing new widgets.
   - Styles go in `styles.css`, in 1280-wide design pixels, next to the
     screen's existing rules.
   - Translations: see below.
5. **Controller navigation** (`input.ts`) — required for every new element:
   - `data-nav` on everything focusable; `data-nav-default` on the element
     focused when the screen opens.
   - `data-nav-scope` on a dialog/panel to trap focus inside it (the latest
     mounted scope wins).
   - Geometry picks the neighbour; override with
     `data-nav-up|down|left|right="<css selector>"` (or `"none"`), and on a
     container `data-nav-exit-<dir>="<selector>"`. Give targets a
     `data-nav-id`.
   - Buttons other than A/B/D-pad: `input.pushHandler(a => …)` (return `true`
     when handled); modals: `input.pushModal(onBack)`. Show them in the footer
     with `useHints([...])`. L1/R1 always switch tabs — don't take them.
6. **Translations** — every `tr('…')` string gets a Portuguese entry in the
   area's file in `web/src/locales/pt/` (`common`, `discover`, `community`,
   `store`, `transfers`, `install`, `games`, `explore`, `settings`, `compat`),
   keyed by the exact English text, `{placeholders}` kept. Write both languages
   plainly; Portuguese is Brazilian.
7. **Checks**: `cd web && pnpm build` (translations + types + bundle),
   `pnpm i18n` (drop unused entries), `cd launcher && cargo test`.
8. **Try it** (next section), then open the pull request: what changed for
   the user, how it was tried, screenshots for UI changes. Don't bump
   versions or tag; maintainers release.

## Trying a change

- **Local (any Linux/macOS)** — UI and logic that don't need Steam:
  `(cd web && pnpm build) && (cd launcher && cargo run -- --no-browser)`, then
  open http://127.0.0.1:47800 in a browser. Keyboard = controller: arrows
  D-pad, Enter A, Escape/Backspace B, `x`/`y` X/Y, `q`/`e` L1/R1, PageUp/PageDown
  L2/R2, `m` ☰, `v` View. `pnpm dev` gives hot reload with `/api` proxied.
  A second instance needs its own `PISHOP_PORT` **and** `XDG_DATA_HOME`.
- **On a SteamOS device** — anything touching Steam, Proton, gamescope, the
  controller or real installs: `scripts/build.sh` (needs `pnpm`, `zig`,
  `cargo-zigbuild`, the `x86_64-unknown-linux-musl` target) then
  `DECK=deck@<host> scripts/deploy.sh` (`--install` the first time to add the
  Steam shortcut). Start piShop from Steam. Logs:
  `~/.local/share/piShop/launcher.log`. Screen capture over SSH:
  `XDG_RUNTIME_DIR=/run/user/1000 gamescopectl screenshot /tmp/shot.png`.
- Don't test against someone's real instance (port 47800 and their data dir)
  while they use it.

## Product rules

- **Games only**: downloading, installing and managing games. No media playback.
- **Tabs have roles.** Transfers handles downloads and the first install step
  (which makes the shortcut and prefix); everything about an installed game is
  in Games. Deleting a download never removes the installed game.
- **Metadata, field by field**: Steam (app id) → TheGamesDB → SteamGridDB →
  isitcracked (also the only source of crack date/group/DRM). Store what was
  matched (`library.rs`) instead of re-matching by name later.
- **Endpoints and API keys come from Settings** (exportable with Services →
  Export), never hardcoded.
- **Moving a game updates its shortcut**: target and "Start in" (double
  quotes), registry paths — through `install.rs`'s after-move step.
- **Storage status** wherever a place on disk is named.
- **A failed prompt keeps its text** and shows the reason under the field.
- **Long operations** show progress, can be cancelled, survive a restart, and
  never block the UI.
- **Nothing private on screen or in logs** that shouldn't be: passwords and
  keys are never echoed back (the API returns `has_password`, not the value).

## Code style

Match the file you're in: naming, structure, comment density. Comments say
*why* in full sentences; doc comments on public items. Rust: edition 2024,
`anyhow`, no `unwrap()` on anything from the user, disk or network. TypeScript:
strict, function components and hooks, no new UI library. Add a dependency only
when the standard library or an existing one can't do it, and say why in
`Cargo.toml`/the PR.

## Before you report done

- [ ] Works with the controller alone (focus order, default focus, B backs out, footer hints)
- [ ] Every new UI string has Portuguese; `pnpm build` and `pnpm i18n` clean
- [ ] `cargo test` passes; new logic has a unit test where it can
- [ ] Failure paths: network down, disk full, file missing — the user sees why and can retry
- [ ] Tried locally, and on a SteamOS device when it touches Steam/Proton/controller — or the PR says what wasn't
- [ ] No version bump, no secrets, no unrelated changes
