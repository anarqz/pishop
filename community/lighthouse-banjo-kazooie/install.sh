#!/usr/bin/env bash
# Banjo-Kazooie through Lighthouse (HarbourMasters), the Linux AppImage.
#
# piShop has already extracted the Lighthouse Linux zip into this game's
# folder (the current folder, $PISHOP_GAME_DIR): lighthouse.appimage,
# gamecontrollerdb.txt and readme.txt. The ROM ($PISHOP_PART_ROM) was checked
# against the supported dumps' SHA-1.
#
# Lighthouse keeps its settings, saves, mods and bk.o2r (built from the ROM
# at the first start) in $SHIP_HOME, and looks for the ROM there. The
# launcher below points SHIP_HOME at this folder, so everything stays
# together when piShop moves the game.
#
# Safe to run again.
set -euo pipefail

echo "::step Checking Lighthouse"
app=$(find . -maxdepth 2 -type f -iname '*.appimage' -print -quit)
if [[ -z $app ]]; then
  echo "No AppImage in the Lighthouse download." >&2
  exit 1
fi
if [[ $app != ./lighthouse.appimage ]]; then
  mv -f "$app" lighthouse.appimage
fi
chmod +x lighthouse.appimage

echo "::step Adding your ROM"
cp -f "$PISHOP_PART_ROM" baserom.us.z64
chmod 0644 baserom.us.z64

echo "::step Writing the launcher"
cat > lighthouse.sh <<'LAUNCHER'
#!/usr/bin/env bash
# Runs Lighthouse from this folder, keeping its data here.
cd "$(dirname "$(readlink -f "$0")")" || exit 1
export SHIP_HOME="$PWD"
exec ./lighthouse.appimage "$@"
LAUNCHER
chmod +x lighthouse.sh
echo "::progress 100 Ready to play"
