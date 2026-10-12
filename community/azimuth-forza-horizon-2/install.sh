#!/usr/bin/env bash
# Forza Horizon 2 (Xbox One, 1.0.0.64) through the Azimuth launcher.
#
# piShop has already extracted, into this game's folder (the current folder,
# $PISHOP_GAME_DIR):
#   ./azimuth.exe, ./data/   ← Azimuth (azimuth-windows.zip)
#   ./Game/                  ← the game's RAR
#   ./Mods/ExtraOptions/     ← RokkuDayo's Extra Options mod
#
# This finds the game's package folder (the one holding Mount/AppxManifest.xml)
# and points Azimuth at it in azimuth.ini, so the launcher doesn't ask for the
# folder at the first start. The path is relative to azimuth.exe, so the game
# keeps working when piShop moves the whole folder to another library.
#
# Safe to run again: an azimuth.ini already there keeps the player's settings
# and only gets its game_folder line updated.
set -euo pipefail

echo "::step Checking Azimuth"
if [[ ! -f azimuth.exe || ! -d data/bin ]]; then
  echo "Azimuth isn't complete in $PWD (azimuth.exe and data/bin are needed)." >&2
  exit 1
fi

echo "::step Adding Extra Options"
# Its Azimuth build goes into data/bin, replacing Azimuth's azimuth-tls.dll.
mod=Mods/ExtraOptions/Azimuth
if [[ ! -f $mod/azimuth-tls.dll ]]; then
  echo "Extra Options isn't complete in $PWD/$mod." >&2
  exit 1
fi
for f in "$mod"/*; do
  name=${f##*/}
  # The player's own settings stay when this runs again.
  if [[ $name == *.ini && -f data/bin/$name ]]; then
    echo "Keeping data/bin/$name"
    continue
  fi
  cp -f "$f" "data/bin/$name"
  echo "data/bin/$name"
done

echo "::step Finding the game's files"
manifest=$(find Game -maxdepth 4 -type f -path '*/Mount/AppxManifest.xml' -print -quit)
if [[ -z "$manifest" ]]; then
  echo "No Mount/AppxManifest.xml under Game/: the archive isn't Forza Horizon 2's package." >&2
  exit 1
fi
package=$(dirname "$(dirname "$manifest")")
echo "Game package: $package"
for need in era.vbi EmbeddedXvd; do
  if [[ ! -e "$package/$need" ]]; then
    echo "Warning: $package/$need is missing; Azimuth may say the game is incomplete." >&2
  fi
done

echo "::step Pointing Azimuth at the game"
# Windows separators, relative to azimuth.exe: Game\Anthem_1.0.0.64_x64__…
folder=${package//\//\\}

if [[ -f azimuth.ini ]]; then
  found=0
  while IFS= read -r line || [[ -n $line ]]; do
    if [[ $line == game_folder=* ]]; then
      printf 'game_folder=%s\n' "$folder"
      found=1
    else
      printf '%s\n' "$line"
    fi
  done < azimuth.ini > azimuth.ini.new
  if (( ! found )); then
    printf '[launcher]\ngame_folder=%s\n' "$folder" >> azimuth.ini.new
  fi
  mv azimuth.ini.new azimuth.ini
else
  # Azimuth's own template (0.9.7) with its defaults; only game_folder is set.
  cat > azimuth.ini <<INI
# Azimuth's settings, as key=value lines, which the launcher writes as the
# player changes them. Lines that start with # are not read.
[launcher]
# The game's folder, the one holding Mount: a full path, or one relative to
# the folder of azimuth.exe. The launcher asks for it at the first start and
# saves the folder picked.
game_folder=$folder
# Lines rendered (720, 1080, 1440, 2160, ...), or screen.
resolution=1080
# off, msaax2 or msaax4 (the console's).
antialiasing=msaax4
# low (a steadier frame rate, less power), default (the console's) or
# high (the game engine's higher settings where it has them).
graphics=default
# The channels the game is told the sound output has: system (its own:
# stereo, 5.1 or 7.1), stereo, or multichannel (5.1 or 7.1, which the
# system mixes down for an output with fewer).
audio=system
# 30 or 60.
frame_rate=30
# fixed (the console's): each frame advances the game a fixed step, so it
# slows down while frames run late; real-time: each frame advances the time
# it took, so the game keeps its speed and late frames stutter.
frame_pacing=fixed
# on: frames wait for the display's refresh; off: they show at once, with
# tearing, still paced to the frame rate.
vsync=on
# windowed or fullscreen.
display=fullscreen
# A locale name (en-US, de-DE, pl-PL, ...), or empty for the system's.
language=
# The player's name in the game: 1 to 15 letters, digits and single spaces,
# starting with a letter.
player_name=Player 1
INI
fi
echo "game_folder=$folder"
echo "::progress 100 Ready to play"
