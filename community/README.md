# Community installers · Instaladores da comunidade

Games piShop can't set up by itself — a console game behind a wrapper, a
decompilation that needs your own ROM, a port with its own launcher — set up
by a recipe someone wrote here. piShop's **Community** tab lists them; picking
**Install** downloads every part (Transfers), checks them, extracts what's
compressed, runs the game's install script in a sandbox and adds the game to
Steam, ready to play.

*Jogos que o piShop não sabe preparar sozinho, com uma receita escrita aqui. A
aba **Comunidade** lista os jogos; **Instalar** baixa as partes, confere,
extrai, roda o script num sandbox e adiciona o jogo à Steam.*

Every game is one folder:

```
community/
  azimuth-forza-horizon-2/
    manifest.json   what the game is, its parts, how it launches
    install.sh      arranges the parts into the game's folder
  index.json        everything bundled (generated — don't edit)
```

**New here? Start with the step-by-step guide in [CONTRIBUTING.md](../CONTRIBUTING.md);
this page is the reference for every field.**

After changing anything, run `node scripts/community.mjs` and commit
`index.json` with your change. CI runs `--check`, ShellCheck and the policy
below; Copilot reviews the pull request against
[`.github/instructions/community.instructions.md`](../.github/instructions/community.instructions.md);
a maintainer merges.

## manifest.json

```jsonc
{
  "$schema": "../schema.json",
  "id": "azimuth-forza-horizon-2",      // = the folder name: a-z 0-9 - _
  "name": "Forza Horizon 2",
  "platform": "Xbox One",
  "revision": 1,                        // bump on every change
  "updated": "2026-10-12",
  "authors": ["your-github-user"],
  "summary": "One line for the catalog.",
  "description": "The guide: what this is, what to expect.",
  "steamos": ["Known issues on SteamOS and their workarounds."],
  "wrapper": { "name": "Azimuth", "version": "0.9.7", "url": "https://…" },
  "art": { "sgdb_id": 5246839 },        // the game on SteamGridDB (required)
  "requirements": { "ram_gb": 16, "note": "720p60 on a Steam Deck." },
  "parts": [ … ],
  "install": { "script": "install.sh", "disk": 45000000000 },
  "launch": { "exe": "azimuth.exe", "runtime": "proton", "options": "" },
  "patches": [ … ]
}
```

- **`art.sgdb_id`** — the game's id on [SteamGridDB](https://www.steamgriddb.com)
  (the number in `steamgriddb.com/game/<id>`). piShop takes the artwork from it,
  and it is how troubleshooting issues find the game (see below).
  Optional: `steam_appid`, `cover`, `hero` (image URLs).
- **`install.disk`** — about how much the installed game takes (bytes), for the
  space check. The downloads count separately (they go to `~/Downloads/piShop/Community`).
- **`launch.exe`** — the file Steam runs, relative to the game's folder.
  `runtime`: `proton` (Windows) or `native` (Linux; piShop makes it executable).
  `proton`: a Proton to prefer, e.g. `"proton_experimental"` (else the newest installed).
  `options`: launch options, e.g. `"DXVK_ASYNC=1 %command%"`.
- **`patches`** — toggles offered in Games → Patches for this game:
  `{ "id": "ignore-dragonrise", "title": "…", "about": "…", "env": { "SDL_GAMECONTROLLER_IGNORE_DEVICES": "0x0079/0x0006" } }`.
  Use them for workarounds that only some players need.

### Parts

Each part is one file piShop gets before the script runs. The script sees it as
`$PISHOP_PART_<ID>` (id upper-cased, `-` → `_`).

```jsonc
{
  "id": "game",
  "name": "Forza Horizon 2",
  "description": "The Xbox One game files, version 1.0.0.64.",
  "role": "game",                       // launcher | game | data | other
  "source": { … },
  "size": 41410957919,                  // bytes: checked, and lets downloads resume
  "checksums": { "sha1": "…", "md5": "…" },   // a string or a list of accepted values
  "extract": "Game"                     // extract into <game folder>/Game ("" = its root)
}
```

| `source.type` | Fields | Notes |
|---|---|---|
| `http` | `urls` (mirrors, in order), `file` | Direct download. Give `size` and a checksum. |
| `itch` | `page`, `file`, `upload_id` (optional) | Free and pay-what-you-want itch.io games. The id falls back to the file name. |
| `github` | `repo` (`owner/name`), `tag` (`latest`), `asset` (regex) | A release asset. |
| `torrent` | `urls` (.torrent mirrors) or `magnet`, `files` (regex) | Only the files matching `files` are downloaded. |
| `user` | `hint`, `extensions` | **Not public**: the player picks their own copy (a ROM, a game disc). Checksums of every accepted dump are required. |

A `user` part is how a decompilation gets the original game: the player points
at their ROM in the install dialog, piShop checks it against the checksums
(several are fine: `.z64` and `.n64` dumps differ) and the script copies what
it needs into the game's folder. For example:

```jsonc
{
  "id": "rom",
  "name": "GoldenEye 007 (USA) ROM",
  "role": "data",
  "source": { "type": "user", "hint": "Your own GoldenEye 007 (USA) cartridge dump, .z64", "extensions": [".z64"] },
  "checksums": { "sha1": "<the dump's SHA-1>" }
}
```

Archives in `extract` are extracted by piShop (zip, 7z, rar, iso, tar) with
progress; the script doesn't need to.

**`updatable: true`** (GitHub and itch.io parts only) lets players update that
part from the game's page (**Check for updates**). piShop reads its versions
itself: a GitHub part's newest stable release with an asset matching `asset`
(the release's tag, or its name when the tag isn't a version), an itch.io
part's "Version …" next to the file on the page, or its exact size. An update
downloads the newer file, checks it against GitHub's own SHA-256 when there is
one (the manifest's checksums belong to the reviewed version), extracts it
over the old files, and runs `install.sh` again — so the script must cope with
a newer file of the same kind (find it by pattern, not by an exact version
name). Mark the wrapper, port or mod a recipe is built on, never the game data.

## install.sh

Runs after every part is there and extracted, inside the game's folder.

```bash
#!/usr/bin/env bash
set -euo pipefail
echo "::step Copying the ROM"
cp "$PISHOP_PART_ROM" baserom.us.z64
echo "::progress 100 Ready"
```

**What it gets**

| | |
|---|---|
| current folder, `$PISHOP_GAME_DIR` | the game's folder — the only place it can write |
| `$PISHOP_PART_<ID>` | each part's file (read-only) |
| `$PISHOP_GAME`, `$PISHOP_EXE`, `$PISHOP_LANG` | the game's name, `launch.exe`, `en`/`pt` |
| `/tmp` | scratch space on disk, emptied afterwards |
| `/usr/bin` | SteamOS's tools: bash, coreutils, find, sed, awk, 7z, unrar, bsdtar, unzip, python3, sha256sum… |

**The sandbox** (bubblewrap): no network; the home folder, other disks and
`/run` are hidden; the system is read-only. Everything a game needs from the
web is a part.

**Output**: every line goes to the install log the player sees.
`::step <text>` says what it's doing; `::progress <0-100> [text]` moves the
bar. Exit non-zero with a message on stderr when something is wrong.

**Policy** (CI rejects, Copilot flags):

- Only `set -euo pipefail` bash, readable: no `eval`, no encoded blobs.
- No network tools (`curl`, `wget`, `nc`…), no `sudo`, no `steam`/`systemctl`/`flatpak`.
- Write only inside the game's folder; never `~`/`$HOME`.
- **Safe to run again**: a retry runs it on what the last run left. Keep the
  player's settings files when they're already there.
- Don't download or bundle anything that isn't declared as a part.

## Troubleshooting

The game's page in piShop has **Troubleshooting**: it lists the GitHub issues
labelled `sgdb:<art.sgdb_id>` with their comments, and **Report a problem**
opens the issue form already filled in for the game. The label is added by a
workflow from the form's *SteamGridDB ID* field, so every report finds its game.
Answer in the issue: the comments show up in piShop for the next player.

## Testing on a device

Point piShop at your local index: Settings → Services → Community catalog →
the full path of your `index.json` (e.g. `/home/deck/pishop/community/index.json`).
Empty goes back to piShop's repository.
