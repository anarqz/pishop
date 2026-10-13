---
applyTo: "community/**"
---

# Reviewing community installers

Files under `community/` are recipes that piShop runs on players' devices
(SteamOS handhelds): it downloads the parts a `manifest.json` declares, then
runs the game's `install.sh` inside a bubblewrap sandbox (no network, home
hidden, only the game's folder writable). The format and the rules are in
`community/README.md`. Review every pull request touching this folder as a
security and quality gate: the sandbox is the last line of defense, this
review is the first. Be strict and specific; quote the line.

## Block the pull request (request changes) when

**The script**
- Uses the network in any way: `curl`, `wget`, `aria2c`, `nc`/`ncat`/`socat`,
  `ssh`/`scp`/`rsync`, `/dev/tcp`, `python -c` with sockets or urllib, `git clone`.
  Every download must be a part in `manifest.json`.
- Asks for privileges (`sudo`, `pkexec`, `doas`, `su`) or touches the system:
  `steam`, `systemctl`, `flatpak`, `pacman`, `steamos-*`, `/etc`, `/usr`, `/boot`.
- Writes, deletes, moves or `chmod`s anything outside the current folder
  (`$PISHOP_GAME_DIR`): absolute paths, `..` escapes, `~`, `$HOME`,
  `rm -rf` on a variable that could be empty or unquoted.
- Is obfuscated or hard to read: `eval`, `base64 -d`, `xxd -r`, hex/escaped
  payloads, embedded binaries, minified one-liners, `source` of a downloaded file.
- Runs or installs something that wasn't declared as a part (a binary written
  by the script and then executed, a pip/npm install, a downloaded script).
- Lacks `#!/usr/bin/env bash` + `set -euo pipefail`, or isn't safe to run twice
  (a retry runs it on what the last run left: it must not fail or duplicate,
  and must keep the player's existing settings files).

**The manifest**
- A part's source isn't the publisher's own or a well-known archive for it
  (itch.io page of the developer, the project's GitHub releases, archive.org,
  the official site). Flag link shorteners, file lockers, IP addresses,
  `http://`, and mirrors that don't match the part's name.
- An `http` part without `size` and a checksum, or a `user` part without
  checksums of the accepted dumps (piShop verifies them before the script runs).
- `launch.exe` or `extract` points outside the game's folder.
- `updatable: true` on anything but the port, wrapper or mod the recipe runs
  (never game data), or on a part whose script names its files by exact
  version (an update brings a newer name: find them by pattern).
- `patches` set environment variables unrelated to a documented workaround,
  or `launch.options` that run a command other than the game
  (`… ; something`, `bash -c`, a wrapper that isn't a known tool).
- The `id` doesn't match the folder, `revision` wasn't bumped on a change, or
  `art.sgdb_id` isn't the SteamGridDB id of this exact game
  (`https://www.steamgriddb.com/game/<id>` should show it).
- `community/index.json` wasn't regenerated (`node scripts/community.mjs`), or
  was edited by hand.

**Content**
- The pull request ships copyrighted game data, ROMs, BIOS files or keys in the
  repository, or a part links to a commercial game's files where the
  manifest's description doesn't make the provenance clear. Files players must
  own (ROMs, discs) are `user` parts with checksums — never downloads.

## Also comment on (suggestions)

- `::step` messages that don't say what's happening, or long silent steps
  (suggest `::progress`).
- `steamos` notes: each should state the symptom and the fix; workarounds that
  only some players need belong in `patches`.
- Description: what the game is, which version is supported, what to expect on
  a Steam Deck; no marketing copy.
- ShellCheck-style issues: unquoted variables, `cd` without `|| exit`,
  parsing `ls`, `[ ]` instead of `[[ ]]`.

Summarize at the top: the parts and where each comes from, what the script
writes, and whether it is safe to merge.
