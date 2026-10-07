#!/usr/bin/env bash
# Copies dist/piShop to the Deck and (with --install) registers the Steam shortcut.
#   DECK=deck@192.168.68.93 DECK_PASS=... scripts/deploy.sh [--install]
set -euo pipefail
cd "$(dirname "$0")/.."

DECK="${DECK:-deck@192.168.68.93}"
DEST="${DEST:-Applications/piShop}"
SSH=(ssh)
RSYNC_SSH="ssh"
if [[ -n "${DECK_PASS:-}" ]]; then
  export SSHPASS="$DECK_PASS"
  SSH=(sshpass -e ssh)
  RSYNC_SSH="sshpass -e ssh"
fi

"${SSH[@]}" "$DECK" "mkdir -p ~/$DEST"
# .cache holds downloaded covers; --delete must not wipe it. A device installed
# from a release keeps its VERSION (so it still updates itself to the next
# release) and its browser's version marker (so Chromium isn't fetched again).
rsync -az --delete --exclude ".cache" --exclude "VERSION" --exclude "chromium.version" --exclude "chromium/.version-*" \
  -e "$RSYNC_SSH" dist/piShop/ "$DECK:$DEST/"
echo "copiado para $DECK:~/$DEST"

if [[ "${1:-}" == "--install" ]]; then
  "${SSH[@]}" "$DECK" "~/$DEST/pishop --install-steam"
fi
