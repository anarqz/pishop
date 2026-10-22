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

echo "::progress 100 Ready"
