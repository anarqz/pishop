#!/usr/bin/env bash
# Mario Kart 64 through SpaghettiKart (HarbourMasters), the Linux AppImage.
#
# piShop has already extracted spaghetti-linux.zip into this game's folder
# (the current folder, $PISHOP_GAME_DIR): spaghetti.appimage and
# gamecontrollerdb.txt. The ROM ($PISHOP_PART_ROM) was checked against the
# USA cartridge's SHA-1.
#
# SpaghettiKart looks for the ROM in its working folder and builds mk64.o2r
# there at the first start; its settings, saves and mods go to $SHIP_HOME.
# The launcher below runs it from this folder with SHIP_HOME pointing here,
# so everything stays together when piShop moves the game.
#
# Safe to run again.
set -euo pipefail

echo "::step Checking SpaghettiKart"
app=$(find . -maxdepth 2 -type f -iname '*.appimage' -print -quit)
if [[ -z $app ]]; then
  echo "No AppImage in the SpaghettiKart download." >&2
  exit 1
fi
if [[ $app != ./spaghetti.appimage ]]; then
  mv -f "$app" spaghetti.appimage
fi
chmod +x spaghetti.appimage

echo "::step Adding your ROM"
cp -f "$PISHOP_PART_ROM" baserom.us.z64
chmod 0644 baserom.us.z64

echo "::step Writing the launcher"
cat > spaghettikart.sh <<'LAUNCHER'
#!/usr/bin/env bash
# Runs SpaghettiKart from this folder, keeping its data here.
cd "$(dirname "$(readlink -f "$0")")" || exit 1
export SHIP_HOME="$PWD"
exec ./spaghetti.appimage "$@"
LAUNCHER
chmod +x spaghettikart.sh
echo "::progress 100 Ready to play"
