---
name: add-community-game
description: Add a game to piShop's Community tab, or update an existing recipe — a folder community/<id>/ with manifest.json and install.sh. Use when asked to add a port, decompilation, wrapper or fan project to Community, to pin a new release of one, or to fix a community install script.
---

# Add a game to the Community tab

A community recipe tells piShop how to set up a game it can't set up by itself.
piShop downloads every **part**, checks it against the declared size and
checksums, extracts archives, runs the recipe's `install.sh` **in a sandbox**
(no network, home hidden, only the game folder writable) and adds the game to
Steam with SteamGridDB artwork. The script never downloads and never touches
Steam.

References in the repo — read the ones you need, don't guess fields:

- `community/README.md`: every manifest field and the script contract (reference).
- `community/schema.json`: JSON Schema of `manifest.json`.
- `CONTRIBUTING.md`: the same steps for humans.
- `.github/instructions/community.instructions.md`: what the Copilot review flags.
- `scripts/community.mjs`: the validator, including the script policy CI enforces.

## 0. Settle with the human first

Ask instead of assuming — these can't be fixed later by the reviewer:

- **The game and the project** (repo or page) to install, and the version.
- **The source of every file.** Only the developer's own releases (GitHub
  releases, itch.io page) or another source the contributor has the right to
  point to. No file lockers, shorteners or mirrors of unknown origin.
- **What the player must own.** ROMs, disc images, BIOS, game files are
  `user` parts: the player picks their own copy, piShop checks it. They are
  never downloads and never go in the repo.
- **Whether they can test on a SteamOS device** (Steam Deck, ROG Ally…).

## 1. Research the project

Find out, from the project's own README, releases and code:

- The **Linux build** for SteamOS (an AppImage or a Linux zip runs `native`;
  a Windows build runs under `proton`). Prefer what the project recommends for
  Steam Deck.
- Where it keeps **settings and saves** (an env var such as `SHIP_HOME`, the
  current folder, `~/.local/share/...`) — the recipe should keep them inside
  the game folder so piShop can move it.
- What it needs from the player (which ROM, which region/revision, which
  format: `.z64`/`.n64`/`.v64`) and the **checksums it accepts**.
- What happens at first start (asks for a file, builds assets from the ROM).

```bash
# Releases and their assets (size, and SHA-256 when GitHub has it as "digest")
gh api repos/OWNER/REPO/releases --jq '.[] | "\(.tag_name)  \(.published_at)"' | head
gh api repos/OWNER/REPO/releases/tags/TAG --jq '.assets[] | "\(.name)  \(.size)  \(.digest)"'

# No digest? Download it once and hash it
curl -fL -o /tmp/asset "https://github.com/OWNER/REPO/releases/download/TAG/NAME"
stat -c %s /tmp/asset 2>/dev/null || stat -f %z /tmp/asset; shasum -a 256 /tmp/asset

# archive.org publishes size, SHA-1 and MD5 for each file of an item
curl -s https://archive.org/metadata/ITEM | jq '.files[] | select(.name=="FILE") | {size, sha1, md5}'
```

**SteamGridDB id** (`art.sgdb_id`, required: artwork and troubleshooting
reports hang on it). Use the *original game's* entry, not the port's:

```bash
curl -s -X POST https://www.steamgriddb.com/api/public/search/main/games \
  -H 'content-type: application/json' \
  -d '{"asset_type":"grid","term":"Mario Kart 64","offset":0,"filters":{}}' \
  | jq -r '.data.games[] | "\(.game.id)  \(.game.name)  verified=\(.game.verified)"' | head
```

Never invent a checksum, size or id. If one can't be verified from a primary
source, say so to the human and leave the recipe unfinished.

## 2. Start from the closest recipe

```
community/<id>/          id: lowercase a-z 0-9 - _, usually <port-or-wrapper>-<game>
  manifest.json
  install.sh
```

| Pattern | Copy from |
|---|---|
| Native Linux port built from the player's ROM (AppImage + `user` ROM, launcher script) | `spaghettikart-mario-kart-64`, `starship-star-fox-64`, `lighthouse-banjo-kazooie`, `shipwright-ocarina-of-time` |
| Windows game run through a wrapper under Proton (several parts, config written ahead, a patch toggle) | `azimuth-forza-horizon-2` |

## 3. manifest.json

Required: `id` (= folder name), `name`, `platform`, `revision` (1, bumped on
every change), `authors` (GitHub users), `summary` (one line), `description`
(the guide: what it is, which version, what to expect on a Deck), `art`
(`sgdb_id`; optional `steam_appid`, `cover`, `hero`), `parts`, `install`,
`launch`. Optional: `updated` (YYYY-MM-DD), `steamos` (known issues + fixes),
`wrapper` (`name`, `version`, `url`), `requirements` (`ram_gb`, `note`),
`patches`. Keep `"$schema": "../schema.json"` at the top.

Each part:

```jsonc
{
  "id": "rom",                 // the script sees it as $PISHOP_PART_ROM (upper-case, - → _)
  "name": "Mario Kart 64 (USA) ROM",
  "role": "data",              // launcher | game | data | other
  "source": { "type": "user", "hint": "Your own Mario Kart 64 (USA) dump, .z64", "extensions": [".z64"] },
  "size": 12345,               // bytes; omit for user parts
  "checksums": { "sha1": ["<dump 1>", "<dump 2>"] },  // sha1 / sha256 / md5: a value or a list
  "extract": ""                // archives: extract into <game>/<this> ("" = the game folder)
}
```

| `source.type` | Fields |
|---|---|
| `github` | `repo` (`owner/name`), `tag` (**pin one**; avoid `latest`), `asset` (regex for the asset name) |
| `itch` | `page`, `file` (upload name), `upload_id` (optional) — free / pay-what-you-want games |
| `http` | `urls` (mirrors, tried in order and used in parallel), `file` |
| `torrent` | `urls` of the .torrent or a `magnet`, `files` (regex choosing files inside) |
| `user` | `hint` (what to pick), `extensions` — checksums of every accepted dump are required |

`"updatable": true` (GitHub and itch.io parts only) lets players update that
part from the game's page: piShop reads the newer version itself (newest
release matching `asset`; itch.io's "Version …" or the file's size), downloads
it, extracts it over the old files and runs `install.sh` again. Use it for the
port, wrapper or mod the recipe is built on — not game data — and make the
script find that part's files by pattern (`*.appimage`), never by an exact
version name.

Pin `size` and a checksum for every downloaded part: a file that changes
upstream then fails its check instead of running something nobody reviewed.

`install`: `{ "script": "install.sh", "disk": <bytes the installed game takes, estimate> }`.
`launch`: `exe` (relative to the game folder), `runtime` (`native` | `proton`),
optional `options` (Steam launch options, `%command%`), `proton` (a preferred
Proton). `patches`: toggles in Games → Patches, e.g.
`{ "id": "…", "title": "…", "about": "…", "env": { "VAR": "value" } }` — for
workarounds only some players need.

## 4. install.sh

Runs once every part is in place and extracted, with the game folder as the
current directory.

| Available | |
|---|---|
| current folder, `$PISHOP_GAME_DIR` | the game folder — the only writable place |
| `$PISHOP_PART_<ID>` | each part's file, read-only |
| `$PISHOP_GAME`, `$PISHOP_EXE`, `$PISHOP_LANG` | game name, `launch.exe`, `en`/`pt` |
| `/tmp` | scratch on disk, emptied afterwards |
| `/usr/bin` | SteamOS tools: bash, coreutils, find, sed, awk, 7z, unrar, bsdtar, unzip, python3, sha256sum |

The validator rejects (`scripts/community.mjs` → `POLICY`):

- a first line other than `#!/usr/bin/env bash`, or no `set -euo pipefail`;
- `sudo`/`pkexec`/`doas`; network tools (`curl`, `wget`, `aria2c`, `nc`, `ssh`,
  `scp`, `rsync`, `/dev/tcp`); `eval`; decoding embedded data (`base64 -d`,
  `xxd -r`);
- `steam`, `systemctl`, `flatpak`, `pacman`, `steamos-session-select`;
- `~/`, `$HOME`; `rm -rf` of `/`, `~` or `$HOME`; `chmod 777`.

Write it so that:

- **It is safe to run again** — a retry runs on what the last run left. Keep
  the player's settings files when they already exist.
- It **says what it does**: `echo "::step <text>"` per step,
  `echo "::progress <0-100> [text]"` for long ones; errors go to stderr with a
  non-zero exit.
- It opens with a comment block a reviewer reads first: what was extracted
  where, what the script changes, why.
- Paths in config files are **relative** (piShop can move the whole folder).
- For native ports, a small launcher script in the game folder `cd`s there and
  points the port's data dir at it (see `lighthouse.sh` in
  `lighthouse-banjo-kazooie/install.sh`); `launch.exe` names that script.

## 5. Validate

```bash
node scripts/community.mjs            # validates everything, rewrites community/index.json
node scripts/community.mjs --check    # what CI runs: must pass
command -v shellcheck && shellcheck community/<id>/install.sh
```

Commit `community/index.json` with the folder; never edit it by hand. Fix
warnings too (e.g. a script without `::step`).

## 6. Test on a device (the human does this)

Hand the human these steps when they have a SteamOS device:

1. `scp community/index.json deck@<device>:~/index.json`
2. piShop → **Settings → Services → Community catalog** → `/home/deck/index.json` → Save.
3. **Community** → the game → **Install**; follow it in Transfers (a failing
   script's log is on the install page).
4. Play it from Steam. Afterwards clear the catalog address to go back to the
   official catalog.

If nobody can test, the pull request must say "not tested on a device".

## 7. Pull request

Title `Community: <game>`. Fill in the template (`.github/pull_request_template.md`):
where each part comes from and why that source is trustworthy, where the
checksums came from, the SteamGridDB id, `--check` passing, and what it was
tested on. CI runs the validator and ShellCheck; Copilot reviews against
`.github/instructions/community.instructions.md`; a maintainer merges, and the
game appears in every piShop's Community tab without an app update.

**Updating a recipe**: change it, bump `revision`, update `updated`, rebuild
the index. Installs still downloading pick up the new revision by themselves.

## Before you report done

- [ ] Every downloaded part: source confirmed with the human, `size` + checksum from a primary source
- [ ] `user` parts for anything the player must own; checksums for each accepted dump
- [ ] `art.sgdb_id` looked up (original game), not guessed
- [ ] Script: policy-clean, safe to re-run, `::step` messages, data kept in the game folder
- [ ] `node scripts/community.mjs --check` passes; `index.json` regenerated and staged
- [ ] Tested on a device — or clearly marked untested
