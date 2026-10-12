#!/usr/bin/env bash
# The Legend of Zelda: Ocarina of Time through Ship of Harkinian
# (HarbourMasters), the Linux AppImage.
#
# piShop has downloaded the AppImage ($PISHOP_PART_SOH) and checked the ROM
# ($PISHOP_PART_ROM) against the supported dumps' SHA-1 (N64, GameCube and
# Master Quest versions).
#
# Ship of Harkinian keeps its settings, saves, mods and oot.o2r / oot-mq.o2r
# (built from the ROM) in $SHIP_HOME. The launcher below points SHIP_HOME at
# this folder, so everything stays together when piShop moves the game, and
# until an archive exists it hands the ROM over on the command line: Ship of
# Harkinian then builds it right away, without its file picker.
#
# Safe to run again.
set -euo pipefail

echo "::step Adding Ship of Harkinian"
cp -f "$PISHOP_PART_SOH" soh.appimage
chmod +x soh.appimage

echo "::step Adding your ROM"
cp -f "$PISHOP_PART_ROM" baserom.z64
chmod 0644 baserom.z64

echo "::step Writing the launcher"
cat > soh.sh <<'LAUNCHER'
#!/usr/bin/env bash
# Runs Ship of Harkinian from this folder, keeping its data here. The first
# time (no oot.o2r / oot-mq.o2r yet) it gets the ROM to build them from.
cd "$(dirname "$(readlink -f "$0")")" || exit 1
export SHIP_HOME="$PWD"
if [[ ! -e oot.o2r && ! -e oot-mq.o2r && -f baserom.z64 ]]; then
  exec ./soh.appimage "$PWD/baserom.z64"
fi
exec ./soh.appimage "$@"
LAUNCHER
chmod +x soh.sh
echo "::progress 100 Ready to play"
