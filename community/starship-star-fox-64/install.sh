#!/usr/bin/env bash
# Star Fox 64 through Starship (HarbourMasters), the Linux AppImage.
#
# piShop has already extracted the Starship Linux zip into this game's folder
# (the current folder, $PISHOP_GAME_DIR): starship.appimage, config.yml and
# assets/ (what it extracts the ROM with), mods/, readme.txt and
# gamecontrollerdb.txt. The ROM ($PISHOP_PART_ROM) was checked against the
# USA 1.0/1.1 dumps' SHA-1.
#
# Starship 2.0.0 reads config.yml and assets/ from its working folder and
# writes sf64.o2r there; the game reads it from $SHIP_HOME. The launcher below
# runs it from this folder with SHIP_HOME pointing here, so both agree and
# everything stays together when piShop moves the game.
#
# Safe to run again.
set -euo pipefail

echo "::step Checking Starship"
app=$(find . -maxdepth 2 -type f -iname '*.appimage' -print -quit)
if [[ -z $app ]]; then
  echo "No AppImage in the Starship download." >&2
  exit 1
fi
if [[ $app != ./starship.appimage ]]; then
  mv -f "$app" starship.appimage
fi
chmod +x starship.appimage
if [[ ! -f config.yml || ! -d assets ]]; then
  echo "config.yml or assets/ is missing: Starship can't build its assets without them." >&2
  exit 1
fi
mkdir -p mods

# The 2.0.0 zip's gamecontrollerdb.txt is a web page, not the mappings file:
# without it SDL's built-in mappings are used.
if [[ -f gamecontrollerdb.txt ]] && grep -qi '<html' gamecontrollerdb.txt; then
  echo "Removing the broken gamecontrollerdb.txt"
  rm -f gamecontrollerdb.txt
fi

echo "::step Adding your ROM"
cp -f "$PISHOP_PART_ROM" baserom.us.z64
chmod 0644 baserom.us.z64

echo "::step Writing the launcher"
cat > starship.sh <<'LAUNCHER'
#!/usr/bin/env bash
# Runs Starship from this folder, keeping its data here.
cd "$(dirname "$(readlink -f "$0")")" || exit 1
export SHIP_HOME="$PWD"
exec ./starship.appimage "$@"
LAUNCHER
chmod +x starship.sh
echo "::progress 100 Ready to play"
