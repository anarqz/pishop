#!/usr/bin/env bash
set -euo pipefail

echo "::step Checking Infinity Horizon"
for f in infinity.exe infinity-updater.exe hook/infinity.dll overlay/infinity-overlay.dll SHA256SUMS; do
  if [[ ! -f $f ]]; then
    echo "Infinity Horizon isn't complete in $PWD ($f is missing)." >&2
    exit 1
  fi
done

echo "::step Verifying files"
grep -v '\.sig$' SHA256SUMS | sha256sum -c --quiet -

echo "::step Pointing Infinity at Forza Horizon 2"
# Steam runs this before Proton (launch options), outside piShop's sandbox. It
# finds the Forza Horizon 2 recipe's folder (piShop puts recipes side by side)
# and writes the launcher's own settings for it, Azimuth's folder and the
# game's files, only while the player hasn't chosen them in the launcher.
cat > infinity-steamos.sh <<'WRAPPER'
#!/usr/bin/env bash
here=$(dirname "$(readlink -f "$0")")
state="${STEAM_COMPAT_DATA_PATH:-}/pfx/drive_c/users/steamuser/AppData/Local/ForzaInfinity/launcher/production"
win() { local p; p=$(readlink -f "$1"); printf 'Z:%s' "${p//\//\\}"; }
if [[ -n ${STEAM_COMPAT_DATA_PATH:-} ]]; then
  for fh2 in "$here/../Forza Horizon 2" /run/media/*/piShop/"Forza Horizon 2" /run/media/*/*/piShop/"Forza Horizon 2"; do
    [[ -d $fh2 ]] || continue
    azimuth=""
    for dir in "$fh2/Azimuth" "$fh2"; do
      if [[ -f $dir/azimuth.exe ]]; then azimuth=$dir; break; fi
    done
    manifest=$(find "$fh2/GameFiles" "$fh2/Game" -maxdepth 4 -type f -path '*/Mount/AppxManifest.xml' -print -quit 2>/dev/null)
    [[ -n $azimuth || -n $manifest ]] || continue
    mkdir -p "$state"
    if [[ -n $azimuth && ! -f $state/azimuth.json ]]; then
      path=$(win "$azimuth")
      printf '{"Path":"%s"}' "${path//\\/\\\\}" > "$state/azimuth.json"
    fi
    if [[ -n $manifest && ! -f $state/game-content-location.txt ]]; then
      win "$(dirname "$(dirname "$manifest")")" > "$state/game-content-location.txt"
    fi
    break
  done
fi
exec "$@"
WRAPPER
chmod +x infinity-steamos.sh

echo "::progress 100 Ready"
