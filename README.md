<p align="center">
  <img src="art/icon.png" width="96" alt="piShop" />
</p>

<h1 align="center">piShop</h1>

<p align="center">
  <b>Seu hub de jogos dentro do Modo de Jogo do SteamOS.</b><br/>
  <i>Your game hub, right inside SteamOS Gaming Mode.</i><br/><br/>
  <a href="https://anarqz.github.io/pishop/">Site</a> ·
  <a href="https://anarqz.github.io/pishop/?lang=en">Website (EN)</a> ·
  <a href="https://github.com/anarqz/pishop/releases">Releases</a>
</p>

<p align="center">
  <img src="docs/assets/screens/ally/discover.webp" width="860" alt="piShop — Descobrir (ROG Ally, 1920×1080)" />
</p>

---

## Instalação · Install

No **Modo Desktop**, abra o **Konsole** e cole:
*In **Desktop Mode**, open **Konsole** and paste:*

```bash
curl -fsSL https://anarqz.github.io/pishop/install.sh | bash
```

Volte ao **Modo de Jogo** → Biblioteca → **Não-Steam** → **piShop**.
Depois disso o piShop se atualiza sozinho a cada nova versão (rodar o comando de novo também atualiza).
*Back to **Gaming Mode** → Library → **Non-Steam** → **piShop**. From then on piShop updates itself with every new release (running the command again updates too).*

Desinstalar · Uninstall:

```bash
curl -fsSL https://anarqz.github.io/pishop/install.sh | bash -s -- --uninstall
```

Nada é instalado no sistema: tudo fica em `~/Applications/piShop` e `~/.local/share/piShop`.
*Nothing is installed system-wide: everything lives in `~/Applications/piShop` and `~/.local/share/piShop`.*

## O que é · What it is

Um app nativo para SteamOS (Steam Deck, ROG Ally, Legion Go…) feito para o controle:

- **Descobrir** — lançamentos e cracks recentes em visual Big Picture; página do jogo com sinopse (Steam / TheGamesDB), nota no ProtonDB e arte (SteamGridDB).
- **Loja** — busca no The Pirate Bay sem configurar nada, e nos indexadores do seu Prowlarr, se você tiver um; baixa em Downloads ou na pasta que você escolher. Se o jogo encontrado estiver errado, escolha outro (Steam ou SteamGridDB) e o download leva os dados dele.
- **Transferências** — cada download com a capa do jogo e um anel de progresso; dados da Steam, TheGamesDB, SteamGridDB e isitcracked, nessa ordem.
- **Instalação pelo controle** — extrai o download, roda o instalador pela Steam no Modo de Jogo (a transferência para enquanto isso) e aponta o atalho para o executável certo; depois os arquivos baixados podem sair com um botão.
- **Jogos** — todos os atalhos não-Steam, os instalados pelo piShop e os outros: Proton, componentes (Visual C++, DirectX, .NET… pelos instaladores da própria Steam ou pelo winetricks embutido), instaladores no prefixo, mover para outra biblioteca ou para dentro do prefixo, arte, patches (correções nas opções de inicialização) e remover o atalho, o prefixo ou os arquivos.
- **Arte oficial da Steam** — capa, banner, fundo, logo e ícone na maior resolução; SteamGridDB só para o que faltar — ou escolhida à mão.
- **VPN** — importe uma configuração WireGuard (.conf) ou OpenVPN (.ovpn) e ligue pelo controle (NetworkManager do SteamOS, sem root).
- **BitTorrent nativo** — TCP/uTP, DHT, retomada e limite de velocidade.
- **Explorar** — explorador de dois painéis: pastas e cartões, bibliotecas Steam, o prefixo de cada jogo e compartilhamentos de rede (SMB), com fila de cópias para o aparelho.
- **Espaço em disco** — livre e total, com um anel, em todo lugar onde aparece uma pasta ou um disco.
- **Interface SteamOS** — abas em L1/R1, legenda de botões, analógico direito para rolar, teclado da Steam sob demanda, escala automática (Deck / Ally / TV); ignora o controle enquanto um jogo está na tela.
- **Atualiza sozinho** — novas versões do GitHub são baixadas em segundo plano e entram na próxima vez que o app abre.
- **Inglês e português** — o idioma é escolhido em Configurações → Idioma; a configuração dos serviços pode ser exportada e importada (de um arquivo ou de um link: gist do GitHub, Pastebin) para compartilhar.

A lista completa, incluindo o que está em estudo, está no [site](https://anarqz.github.io/pishop/#features).

## Desenvolvimento · Development

| Parte | Stack |
|---|---|
| `launcher/` | Rust (axum, librqbit, smb — com patches em `launcher/vendor/`); também é o `cabextract`/`unzip` do winetricks |
| `web/` | React + Vite (PWA embutida no binário) |
| `docs/` | Site (GitHub Pages) e `install.sh` |
| `scripts/` | `build.sh` (bundle local), `deploy.sh` (copia para o aparelho via SSH) |

```bash
./scripts/build.sh                                   # dist/piShop (Chromium incluído)
DECK=deck@steamdeck.local DECK_PASS=… ./scripts/deploy.sh --install
```

Releases: `git tag vX.Y.Z && git push --tags` (ou *Actions → Release → Run workflow*). Quem instalou pelo `install.sh` recebe a versão nova sozinho.

## Aviso · Disclaimer

O piShop não hospeda, indexa nem distribui conteúdo. Ele se conecta a serviços que **você** configura (The Pirate Bay, Prowlarr, armazenamento de rede, APIs públicas). Use apenas com conteúdo que você tem o direito de baixar.
*piShop does not host, index or distribute any content. It connects to services **you** configure. Only use it with content you have the right to download.*
