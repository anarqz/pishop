# Contributing to piShop

Thanks for helping. The easiest and most useful way to contribute is adding a
game to the **Community** tab — this guide walks through it end to end. Code
contributions are welcome too (see [the end](#code)).

*Português: a forma mais fácil de contribuir é adicionar um jogo à aba
**Comunidade** — este guia mostra o passo a passo (em inglês). Dúvidas podem
ser abertas em português nas issues.*

---

## Adding a game to the Community tab

A community installer is a recipe for a game piShop can't set up by itself: a
console game that runs through a wrapper (Forza Horizon 2 + Azimuth), a
decompilation port that needs the player's own ROM (Mario Kart 64 +
SpaghettiKart), a fan port with its own launcher. Each recipe is a folder in
[`community/`](community/) with two files:

| File | What it says |
|---|---|
| `manifest.json` | The game, its **parts** (each file piShop gets: downloads and files the player picks), how it launches |
| `install.sh` | A bash script that arranges the parts into the game's folder |

When a player picks **Install**, piShop downloads every part (in Transfers),
checks them against the checksums you declared, extracts what's compressed,
runs your script **in a sandbox** (no network, home folder hidden, only the
game's folder writable), and adds the game to Steam with its artwork. Your
script never downloads anything and never touches Steam: piShop does.

The full format is in [`community/README.md`](community/README.md); the
examples below are the shortest path.

> **Using an AI assistant?** The repository has guides written for it:
> [`CLAUDE.md`](CLAUDE.md) (context and rules, also linked from
> [`AGENTS.md`](AGENTS.md) for other tools) and two skills —
> [`add-community-game`](.claude/skills/add-community-game/SKILL.md) for this
> section, [`pishop-feature`](.claude/skills/pishop-feature/SKILL.md) for code.
> Claude Code picks them up by itself (`/add-community-game`,
> `/pishop-feature`); with other assistants, point them at `AGENTS.md`. You
> still own the result: check the sources, checksums and the test before
> opening the pull request.

### 1. Before you start

- **A source you're allowed to point to.** The developer's own page (itch.io,
  GitHub releases), the project's official releases, or a well-known archive.
  Never link to file lockers, URL shorteners or mirrors of unknown origin.
- **Nothing copyrighted goes in the repository.** Game files the player must
  own (ROMs, disc images, BIOS) are `user` parts: the player picks their own
  copy and piShop checks it against the checksums you list. They are never
  downloads.
- **The game's SteamGridDB id**: search the game on
  [steamgriddb.com](https://www.steamgriddb.com); the id is the number in
  `steamgriddb.com/game/<id>`. It gives the game its artwork and files its
  troubleshooting reports. Use the original game's entry (e.g. *Mario Kart 64*,
  not the port).
- **A SteamOS device to test on** (Steam Deck, ROG Ally…) — strongly
  recommended: reviewers can read your recipe, but only a device proves it.

### 2. Create the folder

Fork the repository and create `community/<id>/`. The id is lowercase letters,
digits, `-` and `_`, usually `<port-or-wrapper>-<game>`:

```
community/spaghettikart-mario-kart-64/
  manifest.json
  install.sh
```

### 3. Write `manifest.json`

A complete example — a native Linux port built from the player's ROM:

```json
{
  "$schema": "../schema.json",
  "id": "spaghettikart-mario-kart-64",
  "name": "Mario Kart 64",
  "platform": "Nintendo 64",
  "revision": 1,
  "updated": "2026-10-12",
  "authors": ["your-github-user"],
  "summary": "One line for the catalog.",
  "description": "The guide: what this is, which version is supported, what to expect on a Steam Deck.",
  "steamos": ["A known issue on SteamOS: the symptom, then the fix."],
  "wrapper": { "name": "SpaghettiKart", "version": "1.0.0", "url": "https://github.com/HarbourMasters/SpaghettiKart" },
  "art": { "sgdb_id": 21164 },
  "parts": [
    {
      "id": "port",
      "name": "SpaghettiKart",
      "role": "launcher",
      "source": { "type": "github", "repo": "HarbourMasters/SpaghettiKart", "tag": "1.0.0", "asset": "^spaghetti-linux\\.zip$" },
      "size": 12395451,
      "checksums": { "sha256": "54e249bd4aa71e55c63239c9c1c0eb522b07497d85f2ede193907059e4de250c" },
      "extract": ""
    },
    {
      "id": "rom",
      "name": "Mario Kart 64 (USA) ROM",
      "role": "data",
      "source": { "type": "user", "hint": "Your own Mario Kart 64 (USA) dump, in .z64 format", "extensions": [".z64"] },
      "checksums": { "sha1": "579c48e211ae952530ffc8738709f078d5dd215e" }
    }
  ],
  "install": { "script": "install.sh" },
  "launch": { "exe": "spaghettikart.sh", "runtime": "native" }
}
```

Your editor validates it against [`community/schema.json`](community/schema.json)
through `$schema`. The fields that matter most:

- **`parts[].source`** — where a part comes from:

  | `type` | Use it for |
  |---|---|
  | `github` | A release asset: `repo`, `tag` (pin one), `asset` (a regex for its name) |
  | `itch` | A free or pay-what-you-want itch.io game: `page`, `file` (the upload's name), `upload_id` (optional) |
  | `http` | A direct link: `urls` (mirrors, tried in order and used in parallel), `file` |
  | `torrent` | `urls` of the .torrent (or a `magnet`) and `files`, a regex choosing which of its files |
  | `user` | A file the player must own: `hint` (what to pick), `extensions` |

- **`size` and `checksums`** — pin what you reviewed. A part that changes
  upstream then fails its check instead of running something nobody reviewed.
  `checksums` takes `sha1`, `sha256` and/or `md5`, each a string or a list of
  accepted values (list every ROM dump the port supports).
- **`extract`** — piShop extracts the part (zip, 7z, rar, iso, tar) into the
  game's folder under this subfolder (`""` is the folder itself). Your script
  doesn't need to.
- **`launch`** — `exe` is the file Steam runs, relative to the game's folder;
  `runtime` is `proton` (Windows) or `native` (Linux); `options` are Steam
  launch options; `proton` names a preferred Proton.
- **`patches`** — optional toggles players can switch in Games → Patches
  (environment variables for workarounds only some players need).
- **`revision`** — start at 1 and bump it on every change.

**Getting sizes and checksums:**

```bash
# A GitHub release asset: size and SHA-256 straight from GitHub
gh api repos/OWNER/REPO/releases/tags/TAG --jq '.assets[] | "\(.name) \(.size) \(.digest)"'

# archive.org publishes size, SHA-1 and MD5 of every file of an item
curl -s https://archive.org/metadata/ITEM | jq '.files[] | select(.name == "FILE") | {size, sha1, md5}'

# Anything you downloaded yourself
stat -c %s FILE && sha256sum FILE        # Linux
stat -f %z FILE && shasum -a 256 FILE    # macOS
```

For ROMs, take the checksums the port itself documents (its README, a
`config.yml`, a list of supported hashes) — they are what it will accept.

### 4. Write `install.sh`

It runs once every part is there and extracted, **inside the game's folder**:

```bash
#!/usr/bin/env bash
# What this does, in a few lines: reviewers read this first.
set -euo pipefail

echo "::step Adding your ROM"
cp -f "$PISHOP_PART_ROM" baserom.us.z64

echo "::step Writing the launcher"
cat > spaghettikart.sh <<'LAUNCHER'
#!/usr/bin/env bash
cd "$(dirname "$(readlink -f "$0")")" || exit 1
export SHIP_HOME="$PWD"
exec ./spaghetti.appimage "$@"
LAUNCHER
chmod +x spaghettikart.sh spaghetti.appimage
echo "::progress 100 Ready to play"
```

What it gets:

| | |
|---|---|
| the current folder, `$PISHOP_GAME_DIR` | the game's folder, the only place it can write |
| `$PISHOP_PART_<ID>` | each part's file, read-only (`id` upper-cased, `-` → `_`) |
| `$PISHOP_GAME`, `$PISHOP_EXE`, `$PISHOP_LANG` | the game's name, `launch.exe`, `en` or `pt` |
| `/tmp` | scratch space on disk, emptied afterwards |
| `/usr/bin` | SteamOS's tools: bash, coreutils, find, sed, awk, 7z, unrar, bsdtar, unzip, python3, sha256sum… |

Rules (CI rejects, reviewers flag):

- `#!/usr/bin/env bash` and `set -euo pipefail`; readable — no `eval`, no
  encoded blobs.
- **No network** (`curl`, `wget`…), **no `sudo`**, nothing that touches Steam
  or the system. Every download is a part.
- Write only inside the game's folder; never `~` or `$HOME`.
- **Safe to run again**: a retry runs it on what the last run left. Keep the
  player's settings files when they're already there.
- Say what you're doing: `::step <text>` names the step, `::progress <0-100>
  [text]` moves the bar. Errors go to stderr with a non-zero exit.

Patterns that work well, all in the existing recipes:

- **Pre-configure the game** so its first start doesn't ask for anything:
  `azimuth-forza-horizon-2` writes `azimuth.ini` pointing at the game files.
- **A launcher script** for native ports that keeps settings and saves inside
  the game's folder (`SHIP_HOME` for HarbourMasters ports), so moving the game
  to another library or the SD card keeps everything working.
- **Relative paths** in config files: piShop can move the whole folder.

### 5. Build the index

```bash
node scripts/community.mjs           # validates every manifest, writes community/index.json
node scripts/community.mjs --check   # what CI runs
```

Commit `community/index.json` together with your folder — never edit it by
hand. If [ShellCheck](https://www.shellcheck.net) is installed, run it too:
`shellcheck community/<id>/install.sh`.

### 6. Test it on a device

1. Copy your `community/index.json` to the device (e.g. `scp
   community/index.json deck@steamdeck:~/index.json`).
2. In piShop: **Settings → Services → Community catalog** → the file's full
   path (`/home/deck/index.json`) → Save.
3. Open **Community**, find your game, **Install**. Follow it in Transfers; if
   the script fails, its log is on the install's page.
4. Play it from Steam. Then clear the catalog address in Settings to go back to
   the official one.

### 7. Open the pull request

Title it `Community: <game>` and fill in:

- [ ] Where each part comes from, and why that source is trustworthy
- [ ] Sizes and checksums pinned (and where the checksums came from)
- [ ] `art.sgdb_id` is this game on SteamGridDB
- [ ] `node scripts/community.mjs --check` passes
- [ ] Tested on: *device*, *SteamOS version* — or "not tested on a device"

Every pull request touching `community/` gets CI (manifests, script policy,
ShellCheck, index up to date) and a **GitHub Copilot review** following
[`.github/instructions/community.instructions.md`](.github/instructions/community.instructions.md)
— read it to know exactly what gets flagged. A maintainer merges; the game
shows up in everyone's Community tab right away (no app update needed).

### 8. After it's merged

- **Problems** players hit are reported from the game's page in piShop
  (**Troubleshooting → Report a problem**), which opens an issue already filed
  under the game (`sgdb:<id>` label). Answering there helps the next player:
  the comments show up in piShop.
- **Updating** the recipe (a new port release, a new workaround): change it,
  bump `revision`, rebuild the index, open a pull request. Installs still
  downloading pick the new revision up by themselves.

---

## Code

- `launcher/` — Rust (axum, librqbit). `cargo test` runs the unit tests.
- `web/` — React + Vite. `pnpm build` type-checks and builds; every UI string
  goes through `tr()` and needs its Portuguese translation in
  `web/src/locales/pt/` (`pnpm i18n` lists what's missing).
- `scripts/build.sh` builds the SteamOS bundle (needs `zig` and
  `cargo-zigbuild`); `scripts/deploy.sh` copies it to a device over SSH.
- Releases are tags (`vX.Y.Z`); installs update themselves.

Working with an AI assistant: have it follow
[`.claude/skills/pishop-feature/SKILL.md`](.claude/skills/pishop-feature/SKILL.md)
(Claude Code: `/pishop-feature`).

Open an issue before a big change, so we can agree on the approach first.

## Reporting problems

[Open an issue](https://github.com/anarqz/pishop/issues) with your device,
SteamOS version and what happened. For a game from the Community tab, use its
page in piShop: **Troubleshooting → Report a problem**.
