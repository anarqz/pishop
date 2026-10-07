#!/usr/bin/env bash
# piShop — instalador para SteamOS / installer for SteamOS
#
#   curl -fsSL https://anarqz.github.io/pishop/install.sh | bash
#
# Rodar de novo atualiza o piShop. / Running it again updates piShop.
# Opções / options:  curl -fsSL …/install.sh | bash -s -- <opção>
#   --uninstall      remove o piShop (mantém configurações) / remove piShop (keeps settings)
#   --purge          com --uninstall, apaga também configurações e cache / also wipe settings and cache
#   --no-steam       não mexe no atalho da Steam / don't touch the Steam shortcut
#   --version vX.Y   instala uma versão específica / install a specific version
#   --dir PATH       pasta de instalação / install folder (default ~/Applications/piShop)
set -euo pipefail

REPO="anarqz/pishop"
ASSET="piShop-linux-x86_64.tar.gz"
DEST="$HOME/Applications/piShop"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}/piShop"
VERSION=""
NO_STEAM=0
UNINSTALL=0
PURGE=0

# ---------- language & output ----------
case "${PISHOP_LANG:-${LC_ALL:-${LC_MESSAGES:-${LANG:-}}}}" in pt*) PT=1 ;; *) PT=0 ;; esac
if [ -t 1 ]; then B=$'\e[1m'; D=$'\e[2m'; G=$'\e[32m'; Y=$'\e[33m'; R=$'\e[31m'; C=$'\e[36m'; N=$'\e[0m'; else B= D= G= Y= R= C= N=; fi
t() { if [ "$PT" = 1 ]; then printf '%s' "$1"; else printf '%s' "$2"; fi; }
step() { printf '%s›%s %s\n' "$C" "$N" "$(t "$1" "$2")"; }
ok() { printf '%s✓%s %s\n' "$G" "$N" "$(t "$1" "$2")"; }
warn() { printf '%s!%s %s\n' "$Y" "$N" "$(t "$1" "$2")"; }
die() { printf '%s✗ %s%s\n' "$R" "$(t "$1" "$2")" "$N" >&2; exit 1; }

# Answers come from the terminal even when the script itself is piped in.
ask() { # ask "pergunta" "question" default(y|n)
  local def="$3" ans=""
  if [ -r /dev/tty ]; then
    printf '%s?%s %s %s ' "$Y" "$N" "$(t "$1" "$2")" "$([ "$def" = y ] && t '[S/n]' '[Y/n]' || t '[s/N]' '[y/N]')"
    read -r ans < /dev/tty || ans=""
  fi
  ans="${ans:-$def}"
  case "$ans" in [sSyY]*) return 0 ;; *) return 1 ;; esac
}

while [ $# -gt 0 ]; do
  case "$1" in
    --uninstall) UNINSTALL=1 ;;
    --purge) PURGE=1 ;;
    --no-steam) NO_STEAM=1 ;;
    --version) VERSION="${2:?}"; shift ;;
    --dir) DEST="${2:?}"; shift ;;
    *) die "opção desconhecida: $1" "unknown option: $1" ;;
  esac
  shift
done

banner() {
  printf '\n%s  π  piShop%s  %s\n\n' "$B" "$N" "$D$(t 'para SteamOS' 'for SteamOS')$N"
}

# ---------- helpers ----------
need() { command -v "$1" >/dev/null 2>&1 || die "comando necessário não encontrado: $1" "required command not found: $1"; }

quit_running() {
  if curl -fs -m 2 http://127.0.0.1:47800/api/health >/dev/null 2>&1; then
    step "Fechando o piShop aberto…" "Closing the running piShop…"
    curl -fs -m 3 -X POST http://127.0.0.1:47800/api/quit >/dev/null 2>&1 || true
    for _ in $(seq 1 30); do pgrep -f "$DEST/pishop" >/dev/null 2>&1 || return 0; sleep 0.2; done
    pkill -f "$DEST/pishop" 2>/dev/null || true
  fi
}

# Steam rewrites its shortcut list on exit, so it must be closed (or
# restarted afterwards) for the piShop shortcut to stick.
STEAM_CLOSED=0
steam_closed_or_warn() {
  pgrep -x steam >/dev/null 2>&1 || return 0
  if ask "A Steam está aberta. Fechar a Steam agora para registrar o atalho?" \
         "Steam is running. Close Steam now to register the shortcut?" y; then
    steam -shutdown >/dev/null 2>&1 || true
    for _ in $(seq 1 60); do
      if ! pgrep -x steam >/dev/null 2>&1; then
        ok "Steam fechada." "Steam closed."
        STEAM_CLOSED=1
        return 0
      fi
      sleep 0.5
    done
    warn "A Steam não fechou a tempo; reinicie-a depois." "Steam didn't close in time; restart it afterwards."
  else
    warn "Reinicie a Steam depois para o piShop aparecer." "Restart Steam afterwards for piShop to show up."
  fi
}

# Steam was closed for the shortcut: open it again, in its own session so it
# keeps running when this terminal closes (Desktop Mode doesn't bring it back).
reopen_steam() {
  [ "$STEAM_CLOSED" = 1 ] || return 0
  step "Abrindo a Steam de novo…" "Opening Steam again…"
  if command -v setsid >/dev/null 2>&1; then
    setsid -f steam >/dev/null 2>&1 < /dev/null || true
  else
    nohup steam >/dev/null 2>&1 < /dev/null &
  fi
  for _ in $(seq 1 30); do
    if pgrep -x steam >/dev/null 2>&1; then
      ok "Steam aberta." "Steam is open."
      return 0
    fi
    sleep 0.5
  done
  warn "A Steam ainda não abriu; abra-a pelo menu se ela não aparecer." "Steam hasn't opened yet; open it from the menu if it doesn't show up."
}

release_url() {
  if [ -n "$VERSION" ]; then
    printf 'https://github.com/%s/releases/download/%s/%s' "$REPO" "$VERSION" "$ASSET"
    return
  fi
  # Newest release, pre-releases included.
  curl -fsSL -m 20 "https://api.github.com/repos/$REPO/releases?per_page=10" | python3 -c '
import json, sys
asset = sys.argv[1]
for rel in json.load(sys.stdin):
    if rel.get("draft"):
        continue
    for a in rel.get("assets", []):
        if a["name"] == asset:
            print(a["browser_download_url"])
            sys.exit(0)
sys.exit(1)
' "$ASSET"
}

# ---------- uninstall ----------
if [ "$UNINSTALL" = 1 ]; then
  banner
  quit_running
  if [ "$NO_STEAM" = 0 ] && [ -x "$DEST/pishop" ]; then
    steam_closed_or_warn
    "$DEST/pishop" --uninstall-steam || warn "Não foi possível remover o atalho da Steam." "Couldn't remove the Steam shortcut."
    reopen_steam
  fi
  rm -rf "$DEST"
  ok "piShop removido de $DEST" "piShop removed from $DEST"
  if [ "$PURGE" = 1 ]; then
    rm -rf "$DATA"
    ok "Configurações e cache apagados." "Settings and cache wiped."
  else
    printf '%s\n' "$D$(t "Suas configurações continuam em $DATA (use --purge para apagar)." "Your settings remain in $DATA (use --purge to wipe them).")$N"
  fi
  exit 0
fi

# ---------- install / update ----------
banner
[ "$(uname -m)" = x86_64 ] || die "Apenas x86_64 é suportado." "Only x86_64 is supported."
if ! grep -qs '^ID=steamos' /etc/os-release; then
  warn "Este sistema não parece ser o SteamOS; continuando mesmo assim." "This doesn't look like SteamOS; continuing anyway."
fi
for c in curl tar python3 unzip pgrep; do need "$c"; done

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

avail_kb=$(df -Pk "$HOME" | awk 'NR==2 {print $4}')
[ "${avail_kb:-0}" -gt 1200000 ] || die "Espaço insuficiente em $HOME (precisa de ~1,2 GB)." "Not enough space in $HOME (needs ~1.2 GB)."

step "Procurando a versão mais recente…" "Looking up the latest version…"
URL="$(release_url)" || die "Nenhuma versão publicada encontrada em github.com/$REPO." "No published release found at github.com/$REPO."

step "Baixando o piShop…" "Downloading piShop…"
curl -fL --progress-bar -o "$TMP/$ASSET" "$URL"
if curl -fsSL -m 20 -o "$TMP/$ASSET.sha256" "$URL.sha256" 2>/dev/null; then
  (cd "$TMP" && sha256sum -c --status "$ASSET.sha256") || die "Download corrompido (sha256 não confere)." "Corrupted download (sha256 mismatch)."
fi
mkdir -p "$TMP/x"
tar -C "$TMP/x" -xzf "$TMP/$ASSET"
NEW="$TMP/x/piShop"
[ -x "$NEW/pishop" ] || die "Pacote inválido." "Invalid package."
REL_VERSION="$(cat "$NEW/VERSION" 2>/dev/null || echo '?')"
CHROME_VERSION="$(cat "$NEW/chromium.version")"

# Chromium (Chrome for Testing) comes straight from Google, once per version.
if [ -x "$DEST/chromium/chrome" ] && [ -f "$DEST/chromium/.version-$CHROME_VERSION" ]; then
  ok "Navegador embutido já está na versão $CHROME_VERSION." "Bundled browser already at $CHROME_VERSION."
  CHROME_NEW=""
else
  step "Baixando o navegador embutido (Chromium $CHROME_VERSION, ~190 MB)…" "Downloading the bundled browser (Chromium $CHROME_VERSION, ~190 MB)…"
  curl -fL --progress-bar -o "$TMP/chrome.zip" \
    "https://storage.googleapis.com/chrome-for-testing-public/$CHROME_VERSION/linux64/chrome-linux64.zip"
  unzip -q "$TMP/chrome.zip" -d "$TMP/c"
  CHROME_NEW="$TMP/c/chrome-linux64"
  touch "$CHROME_NEW/.version-$CHROME_VERSION"
fi

quit_running
step "Instalando em $DEST…" "Installing into $DEST…"
mkdir -p "$DEST"
# Keep .cache (downloaded covers) and an up-to-date chromium/ across updates.
rm -rf "$DEST/pishop" "$DEST/bin" "$DEST/art" "$DEST/VERSION" "$DEST/chromium.version"
if [ -n "$CHROME_NEW" ]; then
  rm -rf "$DEST/chromium"
  mv "$CHROME_NEW" "$DEST/chromium"
fi
cp -a "$NEW/." "$DEST/"
ok "piShop $REL_VERSION instalado." "piShop $REL_VERSION installed."

if [ "$NO_STEAM" = 0 ]; then
  steam_closed_or_warn
  if "$DEST/pishop" --install-steam; then
    ok "Atalho criado na Steam (Biblioteca → Não-Steam)." "Steam shortcut created (Library → Non-Steam)."
  else
    warn "Não foi possível criar o atalho; adicione $DEST/pishop como jogo não-Steam." \
         "Couldn't create the shortcut; add $DEST/pishop as a non-Steam game."
  fi
  reopen_steam
fi

printf '\n%s%s%s\n' "$B" "$(t 'Pronto!' 'All set!')" "$N"
printf '%s\n' "$(t '  Volte ao Modo de Jogo → Biblioteca → Não-Steam → piShop.' '  Go back to Gaming Mode → Library → Non-Steam → piShop.')"
printf '%s\n\n' "$D$(t '  Daqui em diante o piShop se atualiza sozinho (rodar este comando de novo também atualiza).' '  From now on piShop keeps itself up to date (running this command again updates too).')$N"
