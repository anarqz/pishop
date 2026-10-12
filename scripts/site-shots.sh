#!/usr/bin/env bash
# Recaptures the site's screenshots (docs/assets/screens: Portuguese at the
# root, English under en/) from a piShop built for this Mac, with the
# services set up in .cache/landing (API keys, a NAS for Explore, EmuDeck
# folders) and demo data for Community installs, Transfers, Games and VPN.
#
#   (cd launcher && cargo build --release) && (cd web && pnpm build)  # first
#   scripts/site-shots.sh [pt|en]                                    # both by default
#
# .cache/landing/replace.json (optional): extra text rewrites for the shots,
# e.g. {"192.168.68.91": "nas.local"}. The capture HOME always reads as ~.
set -euo pipefail
cd "$(dirname "$0")/.."
L=.cache/landing
cp launcher/target/release/pishop "$L/pishop"
extra=$( [[ -f $L/replace.json ]] && cat "$L/replace.json" || echo '{}' )
replace=$(node -e 'const [home, extra] = process.argv.slice(1); console.log(JSON.stringify({ [home]: "~", ...JSON.parse(extra) }))' "$PWD/$L/home" "$extra")
for lang in ${1:-pt en}; do
  out=docs/assets/screens
  [[ $lang == en ]] && out=docs/assets/screens/en
  PISHOP_PORT=47950 HOME="$PWD/$L/home" XDG_DATA_HOME="$PWD/$L/data" "$L/pishop" --no-browser > "$L/instance-$lang.log" 2>&1 &
  pid=$!
  sleep 4
  SHOTS_DEMO=1 SHOTS_LANG=$lang SHOTS_REPLACE="$replace" node scripts/screenshots.mjs http://127.0.0.1:47950/ "$out" | tee "$L/shots-$lang.log"
  curl -s -XPOST http://127.0.0.1:47950/api/quit > /dev/null || true
  wait "$pid" || true
done
