#!/usr/bin/env bash
# Builds a self-contained piShop bundle for SteamOS (x86_64) into dist/piShop.
set -euo pipefail
cd "$(dirname "$0")/.."

CHROME_VERSION="${CHROME_VERSION:-154.0.8037.92}"
OUT=dist/piShop
CACHE=.cache

echo "› web"
(cd web && pnpm install --frozen-lockfile >/dev/null && pnpm build >/dev/null)

echo "› launcher"
rustup target add x86_64-unknown-linux-musl >/dev/null 2>&1 || true
# zig links the C bits (ring) for a fully static musl binary.
command -v cargo-zigbuild >/dev/null || { echo "precisa de: brew install zig && cargo install cargo-zigbuild"; exit 1; }
(cd launcher && cargo zigbuild --release --target x86_64-unknown-linux-musl --quiet)

echo "› chromium $CHROME_VERSION"
mkdir -p "$CACHE"
if [[ ! -f "$CACHE/chrome-linux64/.version-$CHROME_VERSION" ]]; then
  curl -fSL -o "$CACHE/chrome-linux64.zip" \
    "https://storage.googleapis.com/chrome-for-testing-public/$CHROME_VERSION/linux64/chrome-linux64.zip"
  rm -rf "$CACHE/chrome-linux64"
  (cd "$CACHE" && unzip -q chrome-linux64.zip && rm chrome-linux64.zip)
  touch "$CACHE/chrome-linux64/.version-$CHROME_VERSION"
fi

echo "› yt-dlp (trailers)"
# Standalone Linux build (PyInstaller); refreshed when older than 30 days.
if [[ ! -f "$CACHE/yt-dlp_linux" ]] || [[ -n "$(find "$CACHE/yt-dlp_linux" -mtime +30)" ]]; then
  curl -fsSL -o "$CACHE/yt-dlp_linux" https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux
fi

echo "› bundle"
mkdir -p "$OUT/art"
rsync -a --delete --exclude '.version-*' "$CACHE/chrome-linux64/" "$OUT/chromium/"
cp launcher/target/x86_64-unknown-linux-musl/release/pishop "$OUT/pishop"
cp art/*.png "$OUT/art/"
mkdir -p "$OUT/bin"
cp "$CACHE/yt-dlp_linux" "$OUT/bin/yt-dlp"
chmod +x "$OUT/bin/yt-dlp"
chmod +x "$OUT/pishop"
du -sh "$OUT"
