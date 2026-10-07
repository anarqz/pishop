// piShop landing: i18n (pt-BR / en-US), screenshot gallery, installer
// terminal demo, copy buttons and the latest release tag.
;(() => {
  const $ = (sel, root = document) => root.querySelector(sel)
  const $$ = (sel, root = document) => [...root.querySelectorAll(sel)]
  const reduceMotion = matchMedia('(prefers-reduced-motion: reduce)').matches
  const store = {
    get: k => {
      try {
        return localStorage.getItem(k)
      } catch {
        return null
      }
    },
    set: (k, v) => {
      try {
        localStorage.setItem(k, v)
      } catch {}
    },
  }
  document.documentElement.classList.add('js')

  // ---------------------------------------------------------------- strings
  const I18N = {
    pt: {
      'meta.title': 'piShop — sua loja de jogos no SteamOS',
      'meta.description':
        'Descubra lançamentos, busque, baixe e instale seus jogos pelo controle — direto no Modo de Jogo do SteamOS. Instale com uma linha; depois ele se atualiza sozinho.',
      'nav.install': 'Instalar',
      'nav.screens': 'Telas',
      'nav.features': 'Recursos',
      'nav.roadmap': 'Em estudo',
      'hero.pill': 'Versão estável',
      'hero.title1': 'Sua loja de jogos',
      'hero.title2': 'dentro do Modo de Jogo.',
      'hero.lead':
        'Descubra lançamentos, busque, baixe, instale e cuide dos seus jogos pelo controle — com Proton, componentes, patches, arte oficial da Steam e VPN — sem sair do SteamOS e sem instalar nada no sistema.',
      'hero.hint': 'No <b>Modo Desktop</b>, abra o <b>Konsole</b>, cole e pressione <kbd>Enter</kbd>.',
      'hero.b1': 'Sem root nem senha',
      'hero.b2': 'Zero dependências',
      'hero.b3': '100% no controle',
      'hero.b4': 'Steam Deck · ROG Ally',
      'hero.altAlly': 'piShop no ROG Ally: aba Descobrir',
      'hero.altDeck': 'piShop no Steam Deck: página do jogo',
      'copy.btn': 'Copiar',
      'copy.label': 'Copiar comando',
      'copy.done': 'Copiado!',
      'copy.toast': 'Comando copiado — agora é só colar no Konsole.',
      'copy.fail': 'Não deu para copiar; selecione o comando e copie à mão.',
      'stats.s1': 'linha para instalar — depois ele se atualiza sozinho',
      'stats.s2': 'pacotes no sistema, nada de sudo',
      'stats.s3': 'jogável no controle',
      'stats.s4': 'mesma escala de interface da Steam',
      'install.kicker': 'Instalação',
      'install.title': 'Uma linha. Como o EmuDeck.',
      'install.lead':
        'O instalador baixa a versão mais recente e o navegador embutido, e cria o atalho na Steam com capa, banner e ícone. Depois disso o piShop se atualiza sozinho.',
      'install.s1t': 'Vá para o Modo Desktop',
      'install.s1': 'Botão Steam → Energia → Mudar para o Modo Desktop.',
      'install.s2t': 'Cole o comando no Konsole',
      'install.s2':
        'Copie o comando acima, cole no Konsole e pressione Enter. Se a Steam estiver aberta, o instalador pergunta antes de fechá-la.',
      'install.s3t': 'Volte ao Modo de Jogo',
      'install.s3': 'Biblioteca → Não-Steam → piShop. Pronto para jogar.',
      'install.update': 'Atualizar',
      'install.uninstall': 'Desinstalar',
      'install.replay': 'Repetir animação',
      'screens.kicker': 'Telas',
      'screens.title': 'Parece SteamOS. Porque foi feito para ele.',
      'screens.lead': 'Capturas reais do app na resolução de cada aparelho. A interface acompanha a escala da Steam sozinha.',
      'screens.prev': 'Tela anterior',
      'screens.next': 'Próxima tela',
      'screens.zoom': 'Ampliar captura',
      'features.kicker': 'Recursos',
      'features.title': 'Tudo o que um portátil com SteamOS pedia.',
      'features.lead': 'Um binário Rust estático, uma interface React e um navegador embutido — sem tocar no sistema.',
      'roadmap.kicker': 'Em estudo',
      'roadmap.title': 'O que vem por aí.',
      'roadmap.lead': 'Trabalho em andamento e ideias em pesquisa. Nada aqui tem data — mas é para onde o piShop está indo.',
      'tech.kicker': 'Por dentro',
      'tech.title': 'Nativo, leve e sem dependências.',
      'tech.lead':
        'Tudo fica em <code>~/Applications/piShop</code> e <code>~/.local/share/piShop</code>. As atualizações do SteamOS não mexem em nada.',
      'faq.title': 'Perguntas frequentes',
      'cta.title': 'Pronto para jogar?',
      'footer.disclaimer':
        'O piShop não hospeda, indexa nem distribui conteúdo: ele se conecta a serviços que você mesmo configura. Use apenas com conteúdo que você tem o direito de baixar. Steam, Steam Deck e SteamOS são marcas da Valve Corporation; ROG Ally é marca da ASUS. Projeto independente, sem afiliação.',
      'footer.releases': 'Versões',
      'footer.issues': 'Reportar um problema',
      'footer.script': 'Ver o script',
      'status.next': 'Próxima etapa',
      'status.wip': 'Em andamento',
      'status.research': 'Em estudo',
      'status.idea': 'Ideia',
    },
    en: {
      'meta.title': 'piShop — your game store on SteamOS',
      'meta.description':
        'Discover new releases, then search, download and install your games with the controller — right in SteamOS Gaming Mode. Install with one line; it keeps itself up to date.',
      'nav.install': 'Install',
      'nav.screens': 'Screens',
      'nav.features': 'Features',
      'nav.roadmap': 'Roadmap',
      'hero.pill': 'Stable release',
      'hero.title1': 'Your game store',
      'hero.title2': 'inside Gaming Mode.',
      'hero.lead':
        'Discover new releases, then search, download, install and manage your games with the controller — Proton, components, patches, official Steam art and a VPN included — without leaving SteamOS or installing anything on the system.',
      'hero.hint': 'In <b>Desktop Mode</b>, open <b>Konsole</b>, paste and press <kbd>Enter</kbd>.',
      'hero.b1': 'No root, no password',
      'hero.b2': 'Zero dependencies',
      'hero.b3': '100% controller',
      'hero.b4': 'Steam Deck · ROG Ally',
      'hero.altAlly': 'piShop on the ROG Ally: Discover tab',
      'hero.altDeck': 'piShop on the Steam Deck: game page',
      'copy.btn': 'Copy',
      'copy.label': 'Copy command',
      'copy.done': 'Copied!',
      'copy.toast': 'Command copied — now paste it into Konsole.',
      'copy.fail': "Couldn't copy; select the command and copy it by hand.",
      'stats.s1': 'line to install — then it updates itself',
      'stats.s2': 'system packages, no sudo',
      'stats.s3': 'playable with a controller',
      'stats.s4': 'same UI scale as Steam',
      'install.kicker': 'Install',
      'install.title': 'One line. Just like EmuDeck.',
      'install.lead':
        'The installer grabs the latest release and the bundled browser, and creates the Steam shortcut with capsule, banner and icon. From then on, piShop updates itself.',
      'install.s1t': 'Switch to Desktop Mode',
      'install.s1': 'Steam button → Power → Switch to Desktop.',
      'install.s2t': 'Paste the command into Konsole',
      'install.s2':
        'Copy the command above, paste it into Konsole and press Enter. If Steam is open, the installer asks before closing it.',
      'install.s3t': 'Back to Gaming Mode',
      'install.s3': 'Library → Non-Steam → piShop. Ready to play.',
      'install.update': 'Update',
      'install.uninstall': 'Uninstall',
      'install.replay': 'Replay animation',
      'screens.kicker': 'Screens',
      'screens.title': 'Looks like SteamOS. Because it was made for it.',
      'screens.lead':
        "Real captures of the app at each device's resolution. The interface follows Steam's scale on its own.",
      'screens.prev': 'Previous screen',
      'screens.next': 'Next screen',
      'screens.zoom': 'Enlarge capture',
      'features.kicker': 'Features',
      'features.title': 'Everything a SteamOS handheld was asking for.',
      'features.lead': 'A static Rust binary, a React interface and a bundled browser — without touching the system.',
      'roadmap.kicker': 'Roadmap',
      'roadmap.title': "What's next.",
      'roadmap.lead': "Work in progress and ideas being researched. Nothing here has a date — but it's where piShop is heading.",
      'tech.kicker': 'Under the hood',
      'tech.title': 'Native, light and <span class="nw">dependency-free</span>.',
      'tech.lead':
        "Everything lives in <code>~/Applications/piShop</code> and <code>~/.local/share/piShop</code>. SteamOS updates don't touch a thing.",
      'faq.title': 'Frequently asked questions',
      'cta.title': 'Ready to play?',
      'footer.disclaimer':
        "piShop doesn't host, index or distribute any content: it connects to services you configure yourself. Only use it with content you have the right to download. Steam, Steam Deck and SteamOS are trademarks of Valve Corporation; ROG Ally is a trademark of ASUS. Independent project, not affiliated.",
      'footer.releases': 'Releases',
      'footer.issues': 'Report an issue',
      'footer.script': 'View the script',
      'status.next': 'Up next',
      'status.wip': 'In progress',
      'status.research': 'Researching',
      'status.idea': 'Idea',
    },
  }

  // ---------------------------------------------------------------- content
  const ICONS = {
    sparkles: '<path d="M12 3.5l1.9 5 5 1.9-5 1.9-1.9 5-1.9-5-5-1.9 5-1.9z"/><path d="M19 15.5l.8 2 2 .8-2 .8-.8 2-.8-2-2-.8 2-.8z"/>',
    card: '<rect x="3" y="4.5" width="18" height="15" rx="2.5"/><rect x="6" y="8" width="5" height="7.5" rx="1"/><path d="M14 9h4M14 12.5h4M14 16h2.5"/>',
    play: '<rect x="2.5" y="5" width="19" height="14" rx="3"/><path d="M10 9.2v5.6l4.8-2.8z"/>',
    search: '<circle cx="11" cy="11" r="6.5"/><path d="M20 20l-4.2-4.2"/>',
    panes: '<rect x="2.5" y="4.5" width="8" height="15" rx="2"/><rect x="13.5" y="4.5" width="8" height="15" rx="2"/><path d="M5.5 9h2.5M5.5 12h2.5M16.5 9h2.5M16.5 12h2.5"/>',
    magnet: '<path d="M5 4h4v7a3 3 0 0 0 6 0V4h4v7a7 7 0 0 1-14 0z"/><path d="M5 8h4M15 8h4"/>',
    download: '<path d="M12 3.5v11M7.5 10l4.5 4.5 4.5-4.5"/><path d="M4 15.5v2A2.5 2.5 0 0 0 6.5 20h11a2.5 2.5 0 0 0 2.5-2.5v-2"/>',
    pad: '<path d="M7.5 7h9a5 5 0 0 1 4.8 6.4l-1 3.4a2.4 2.4 0 0 1-4.2.8L14.5 16h-5l-1.6 1.6a2.4 2.4 0 0 1-4.2-.8l-1-3.4A5 5 0 0 1 7.5 7z"/><path d="M8 10v3.5M6.25 11.75h3.5"/><path d="M15.5 10.5h.01M17.5 12.5h.01"/>',
    bolt: '<path d="M13 2.5L5 13.5h6l-1 8 8-11h-6z"/>',
    box: '<path d="M3.5 7.5L12 3l8.5 4.5v9L12 21l-8.5-4.5z"/><path d="M3.5 7.5L12 12l8.5-4.5M12 12v9"/>',
    shield: '<path d="M12 3l7.5 3v5.5c0 4.5-3.2 8-7.5 9.5-4.3-1.5-7.5-5-7.5-9.5V6z"/><rect x="9" y="11" width="6" height="5" rx="1"/><path d="M10.5 11V9.5a1.5 1.5 0 0 1 3 0V11"/>',
    globe: '<circle cx="12" cy="12" r="8.5"/><path d="M3.5 9.5h17M3.5 14.5h17"/><path d="M12 3.5c2.3 2.4 3.4 5.2 3.4 8.5s-1.1 6.1-3.4 8.5c-2.3-2.4-3.4-5.2-3.4-8.5S9.7 5.9 12 3.5z"/>',
    grid: '<rect x="3.5" y="3.5" width="7" height="7" rx="1.5"/><rect x="13.5" y="3.5" width="7" height="7" rx="1.5"/><rect x="3.5" y="13.5" width="7" height="7" rx="1.5"/><rect x="13.5" y="13.5" width="7" height="7" rx="1.5"/>',
    wrench: '<path d="M14.7 6.3a4 4 0 0 0-5.2 5.2l-5.8 5.8a1.6 1.6 0 0 0 2.3 2.3l5.8-5.8a4 4 0 0 0 5.2-5.2l-2.5 2.5-2.3-.4-.4-2.3z"/>',
    sliders: '<path d="M4 7h10M18 7h2M4 17h4M12 17h8"/><circle cx="16" cy="7" r="2"/><circle cx="10" cy="17" r="2"/>',
    layers: '<path d="M12 3.5l8.5 4.5-8.5 4.5-8.5-4.5z"/><path d="M3.5 12l8.5 4.5 8.5-4.5"/><path d="M3.5 16l8.5 4.5 8.5-4.5"/>',
    image: '<rect x="3" y="4.5" width="18" height="15" rx="2.5"/><circle cx="9" cy="10" r="2"/><path d="M21 16l-5.5-5.5L6 20"/>',
    vpn: '<path d="M12 3l7.5 3v5.5c0 4.5-3.2 8-7.5 9.5-4.3-1.5-7.5-5-7.5-9.5V6z"/><path d="M9 12l2 2 4-4"/>',
    refresh: '<path d="M20 11a8 8 0 0 0-14.3-4.9L4 8"/><path d="M4 4v4h4"/><path d="M4 13a8 8 0 0 0 14.3 4.9L20 16"/><path d="M20 20v-4h-4"/>',
  }
  const icon = name =>
    `<svg viewBox="0 0 24 24" width="22" height="22" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${ICONS[name]}</svg>`

  const FEATURES = [
    {
      icon: 'sparkles',
      shot: 'discover-grid',
      tags: ['isitcracked', 'SteamGridDB'],
      pt: [
        'Descobrir',
        'Os lançamentos mais recentes num visual Big Picture: os 5 cracks mais novos em destaque (troque com L2/R2), a busca no centro e a grade “Cracks recentes”, que vai carregando até o último jogo — com o status e o grupo de cada crack, e capas guardadas em cache no aparelho.',
      ],
      en: [
        'Discover',
        "The latest releases in a Big Picture look: the 5 newest cracks in the spotlight (flip with L2/R2), search front and center and a “Recent cracks” grid that keeps loading to the very last game — with each crack's status and group, and covers cached on the device.",
      ],
    },
    {
      icon: 'card',
      tags: ['Steam', 'TheGamesDB'],
      pt: [
        'Página do jogo',
        'Sinopse, gêneros, desenvolvedora e datas da loja da Steam (ou do TheGamesDB), a nota no ProtonDB, arte do SteamGridDB e um botão para abrir o jogo na Steam.',
      ],
      en: [
        'Game page',
        'Synopsis, genres, developer and dates from the Steam store (or TheGamesDB), its ProtonDB rating, SteamGridDB art and a button to open the game on Steam.',
      ],
    },
    {
      icon: 'download',
      shot: 'install',
      tags: ['Proton', 'Steam'],
      pt: [
        'Instale pelo controle',
        'Em Transferências, A abre o jogo baixado: arquivos compactados são extraídos, o instalador roda pela Steam no próprio Modo de Jogo (a transferência para enquanto isso) e, quando ele fecha, o piShop acha onde o jogo foi parar e aponta o atalho para o executável certo — Unreal, launchers e .NET incluídos. Daí o jogo vai para Jogos, e os arquivos baixados podem sair com um botão.',
      ],
      en: [
        'Install with the controller',
        "In Transfers, A opens the downloaded game: archives get extracted, the installer runs through Steam right in Gaming Mode (the transfer stops meanwhile), and when it closes piShop finds where the game went and points the shortcut at the right executable — Unreal, launchers and .NET included. The game then moves to Games, and the downloaded files can go with one button.",
      ],
    },
    {
      icon: 'grid',
      shot: 'games',
      tags: ['Steam', 'Proton'],
      pt: [
        'Jogos',
        'Todos os atalhos não-Steam num lugar só: os jogos instalados pelo piShop e todo o resto — emuladores, outros launchers, os que você adicionou à mão. A página de cada jogo tem Jogar, o Proton, os componentes, a pasta e o prefixo no Explorar e remover o atalho, o prefixo ou os arquivos.',
      ],
      en: [
        'Games',
        'Every non-Steam shortcut in one place: the games you installed through piShop and everything else — emulators, other launchers, the ones you added by hand. Each game\'s page has Play, its Proton, components, its folder and prefix in Explore, and removing the shortcut, the prefix or the files.',
      ],
    },
    {
      icon: 'sliders',
      shot: 'game-page',
      tags: ['Proton'],
      pt: [
        'Cada jogo do seu jeito',
        'Escolha o Proton, veja onde ficam o jogo e o prefixo — com o espaço livre de cada disco — e mova os arquivos para dentro do prefixo ou para outra biblioteca Steam (NVMe ou cartão SD): o atalho e o registro acompanham. Vale também para os atalhos que o piShop não criou.',
      ],
      en: [
        'Every game, your way',
        "Pick the Proton version, see where the game and its prefix are — with each disk's free space — and move its files into the prefix or to another Steam library (NVMe or SD card): the shortcut and the registry follow. Shortcuts piShop didn't create too.",
      ],
    },
    {
      icon: 'wrench',
      shot: 'patches',
      tags: ['Proton', 'Wine'],
      pt: [
        'Patches',
        'Correções rápidas para tentar quando um jogo não roda no Linux, ligadas e desligadas no jogo (Y): cada uma é uma mudança nas opções de inicialização — como o Sobrescrever o winmm de alguns repacks da FitGirl — e o piShop lê de volta se está ligada, mesmo quando você mexeu na Steam.',
      ],
      en: [
        'Patches',
        "Quick workarounds to try when a game won't run on Linux, switched on and off from the game (Y): each one is a change to its launch options — like Override winmm for some FitGirl repacks — and piShop reads back whether it's on, even when you set it in Steam.",
      ],
    },
    {
      icon: 'layers',
      shot: 'components',
      tags: ['Steam', 'winetricks'],
      pt: [
        'Componentes por jogo',
        'Visual C++, DirectX, .NET e PhysX direto dos instaladores da própria Steam, sem internet; o winetricks embutido cuida do resto. Ou rode, dentro do prefixo do jogo, os redistribuíveis que vieram no download.',
      ],
      en: [
        'Components per game',
        "Visual C++, DirectX, .NET and PhysX straight from Steam's own installers, offline; the bundled winetricks handles the rest. Or run the redistributables that came with the download inside the game's prefix.",
      ],
    },
    {
      icon: 'image',
      tags: ['Steam', 'SteamGridDB'],
      pt: [
        'Arte oficial da Steam',
        'Capa, banner, fundo, logo (na posição que a Steam usa) e ícone na maior resolução que a Steam publica; o SteamGridDB só completa o que faltar. Um botão atualiza a arte de todos os jogos — ou escolha à mão a arte oficial de um jogo da Steam ou qualquer jogo do SteamGridDB.',
      ],
      en: [
        'Official Steam art',
        "Cover, banner, hero, logo (placed where Steam puts it) and icon at the largest size Steam publishes; SteamGridDB only fills what's missing. One button refreshes every game's art — or pick it by hand: a Steam game's official art or any SteamGridDB game.",
      ],
    },
    {
      icon: 'search',
      shot: 'store',
      tags: ['The Pirate Bay', 'Prowlarr'],
      pt: [
        'Loja',
        'Busca no The Pirate Bay e nos indexadores do seu Prowlarr, ao mesmo tempo, em consoles ou PC — e funciona sem configurar nada, direto na API do próprio The Pirate Bay. Uma lista compacta com tamanho, data, seeders e leechers, ordenação e filtro por plataforma; o jogo aparece no cabeçalho com arte e informações, e os detalhes do torrent já vêm com ele. As buscas ficam em cache para poupar os limites dos indexadores. O download vai para Downloads ou para a pasta que você escolher, com o espaço livre de cada lugar à vista. Jogo errado no cabeçalho? Escolha outro — da Steam ou do SteamGridDB — e o download leva os dados dele. E dá para buscar sem cache, quando você acabou de adicionar uma fonte.',
      ],
      en: [
        'Store',
        "The Pirate Bay and your Prowlarr indexers, side by side, for consoles or PC — and it works with no setup at all, straight from The Pirate Bay's own API. A compact list with size, date, seeders and leechers, sorting and a platform filter; the game shows up in the header with art and details, and the release details carry it along. Searches are cached to spare your indexers' limits. Downloads go to Downloads or a folder you pick, with each place's free space in view. Wrong game in the header? Match another one — from Steam or SteamGridDB — and the download takes its details. You can also search without the cache, right after adding a source.",
      ],
    },
    {
      icon: 'panes',
      shot: 'explorer',
      tags: ['SMB2/3', 'EmuDeck'],
      pt: [
        'Explorar',
        'Um explorador de dois painéis feito para o controle, os dois começando na pasta pessoal: L2/R2 levam às suas pastas e cartões, às bibliotecas Steam, ao prefixo de cada jogo e aos seus compartilhamentos de rede. Marque com X, copie com Y — com o espaço livre de cada disco sempre à vista. Pensado para acervos com dezenas de milhares de jogos.',
      ],
      en: [
        'Explore',
        "A two-pane explorer built for the controller, both panes starting at home: L2/R2 jump to your folders and cards, Steam libraries, each game's prefix and your network shares. Mark with X, copy with Y — with each disk's free space always in view. Built for libraries with tens of thousands of games.",
      ],
    },
    {
      icon: 'magnet',
      tags: ['librqbit'],
      pt: [
        'BitTorrent nativo',
        'Motor próprio com TCP, uTP e DHT, links magnet, retomada automática e limite de velocidade em Configurações → Downloads. Os downloads vão para Downloads ou para a pasta que você escolher.',
      ],
      en: [
        'Native BitTorrent',
        'A built-in engine with TCP, uTP and DHT, magnet links, automatic resume and a speed cap in Settings → Downloads. Downloads go to Downloads or a folder you pick.',
      ],
    },
    {
      icon: 'download',
      shot: 'transfers',
      tags: ['Steam', 'TheGamesDB'],
      pt: [
        'Transferências',
        'Cada download aparece como o jogo: capa, anel de progresso e, embaixo, velocidade e tempo restante. Os dados vêm da Steam, do TheGamesDB, do SteamGridDB e do isitcracked, nessa ordem. Pause, pare ou exclua — com ou sem os arquivos; o jogo instalado continua em Jogos. As cópias da rede ficam na mesma tela.',
      ],
      en: [
        'Transfers',
        'Every download shows up as the game: cover, a progress ring and, below it, speed and time left. Game data comes from Steam, TheGamesDB, SteamGridDB and isitcracked, in that order. Pause, stop or delete — with or without the files; an installed game stays in Games. Network copies live on the same screen.',
      ],
    },
    {
      icon: 'globe',
      pt: [
        'Inglês e português',
        'A interface inteira em inglês ou em português, escolhida em Configurações → Idioma. As descrições dos jogos da Steam acompanham o idioma.',
      ],
      en: [
        'English and Portuguese',
        "The whole interface in English or Portuguese, picked in Settings → Language. Game descriptions from Steam follow along.",
      ],
    },
    {
      icon: 'pad',
      pt: [
        'Cara de SteamOS',
        'Abas em L1/R1, L2/R2 para trocar dentro da tela, legenda de botões, analógico direito para rolar, teclado da Steam só quando você pede e a mesma escala de interface da Steam — no Deck, no Ally ou na TV.',
      ],
      en: [
        'Feels like SteamOS',
        'L1/R1 tabs, L2/R2 to flip within a screen, a button legend, right stick to scroll, the Steam keyboard only when you ask for it and the same UI scale as Steam — on the Deck, the Ally or a TV.',
      ],
    },
    {
      icon: 'bolt',
      pt: [
        'Entra e sai na hora',
        'Abre como um jogo não-Steam e fecha limpo: segure View + Menu, ou aperte B até ele perguntar se quer sair — sem “Saindo do jogo…” travado, pop-ups ou barra de tradução. Com um jogo na tela, o piShop ignora o controle.',
      ],
      en: [
        'Instant in, instant out',
        'Opens as a non-Steam game and closes cleanly: hold View + Menu, or press B until it asks to quit — no stuck “Exiting game…”, pop-ups or translate bar. While a game is on screen, piShop ignores the controller.',
      ],
    },
    {
      icon: 'box',
      pt: [
        'Plug and play',
        'Um binário estático e um navegador embutido, tudo na sua pasta pessoal. Sem root, sem pacman, sem modo desenvolvedor — e as atualizações do SteamOS não apagam nada.',
      ],
      en: [
        'Plug and play',
        "A static binary and a bundled browser, all in your home folder. No root, no pacman, no developer mode — and SteamOS updates don't wipe anything.",
      ],
    },
    {
      icon: 'shield',
      pt: [
        'Suas chaves, seu aparelho',
        'The Pirate Bay, Prowlarr, TheGamesDB e isitcracked são configurados em Configurações → Serviços — e dá para exportar e importar essa configuração num arquivo, ou importar direto de um link (um gist do GitHub, um paste do Pastebin), para compartilhar. Nenhuma chave vem embutida, nada passa por servidores do piShop e não há telemetria.',
      ],
      en: [
        'Your keys, your device',
        "The Pirate Bay, Prowlarr, TheGamesDB and isitcracked are set up in Settings → Services — and that setup can be exported to a file and imported from one, or straight from a link (a GitHub gist, a Pastebin paste), to share. No keys are baked in, nothing goes through piShop servers and there's no telemetry.",
      ],
    },
    {
      icon: 'vpn',
      shot: 'vpn',
      tags: ['WireGuard', 'OpenVPN'],
      pt: [
        'VPN',
        'Importe a configuração WireGuard (.conf) ou OpenVPN (.ovpn) do seu provedor em Configurações → VPN e ligue ou desligue pelo controle. Usa o NetworkManager do próprio SteamOS — sem root e sem instalar nada — e, ligada, o aparelho inteiro passa por ela.',
      ],
      en: [
        'VPN',
        "Import your provider's WireGuard (.conf) or OpenVPN (.ovpn) config in Settings → VPN and switch it on or off with the controller. It uses SteamOS's own NetworkManager — no root, nothing to install — and while it's on, the whole device goes through it.",
      ],
    },
    {
      icon: 'refresh',
      pt: [
        'Atualiza sozinho',
        'O piShop procura novas versões no GitHub, baixa em segundo plano e se instala na próxima vez que abrir — ou na hora, em Configurações → Sobre. Configurações e capas ficam; o navegador só é baixado de novo quando muda de versão.',
      ],
      en: [
        'Updates itself',
        'piShop checks GitHub for new versions, downloads them in the background and installs them the next time it starts — or right away from Settings → About. Settings and covers stay; the browser is only downloaded again when its version changes.',
      ],
    },
  ]

  const ROADMAP = [
    {
      status: 'idea',
      pt: [
        'Mais fontes de jogos',
        'O armazenamento de rede (SMB) é só a primeira fonte do Explorar.',
        ['FTP / SFTP e servidores HTTP', 'Pendrives e HDs externos como origem'],
      ],
      en: [
        'More game sources',
        'Network storage (SMB) is just the first source in Explore.',
        ['FTP / SFTP and HTTP servers', 'USB drives and external disks as a source'],
      ],
    },
  ]

  const CMD = 'curl -fsSL https://anarqz.github.io/pishop/install.sh | bash'
  const FAQ = {
    pt: [
      [
        'Funciona no meu aparelho?',
        'Em qualquer PC x86_64 com SteamOS: Steam Deck (LCD e OLED), ROG Ally / Xbox Ally, Legion Go S e outros. O piShop é desenvolvido e testado num ROG Xbox Ally X e na resolução do Steam Deck. Sistemas parecidos, como o Bazzite, devem funcionar, mas não são testados.',
      ],
      [
        'Preciso de root, senha de sudo ou modo desenvolvedor?',
        'Não. Tudo fica na sua pasta pessoal: o app em <code>~/Applications/piShop</code> e as configurações em <code>~/.local/share/piShop</code>. Nada de pacman, nada na partição do sistema — por isso as atualizações do SteamOS não apagam o piShop.',
      ],
      [
        'Como atualizo?',
        'Não precisa: o piShop procura novas versões sozinho, baixa em segundo plano e se instala na próxima vez que abrir (ou na hora, em Configurações → Sobre). Rodar o comando de novo também atualiza. Suas configurações e o cache de capas ficam; o navegador só é baixado outra vez quando muda de versão.',
      ],
      [
        'Como desinstalo?',
        `<code>${CMD} -s -- --uninstall</code> remove o app e o atalho da Steam e mantém suas configurações. Adicione <code>--purge</code> para apagar tudo.`,
      ],
      [
        'O que preciso configurar?',
        'Nada para começar: a Loja já busca no The Pirate Bay. O Prowlarr (endereço e chave de API) é opcional e soma os seus indexadores; o Descobrir usa a API do isitcracked e os detalhes dos jogos podem usar o TheGamesDB — sem eles, o piShop avisa e segue funcionando. Tudo fica em Configurações → Serviços — e dá para importar a configuração de alguém de um arquivo ou de um link (gist do GitHub, Pastebin). A loja da Steam e o SteamGridDB não pedem chave.',
      ],
      [
        'Tem VPN?',
        'Tem: importe o arquivo WireGuard (.conf) ou OpenVPN (.ovpn) do seu provedor em Configurações → VPN e ligue pelo controle. Ela usa o NetworkManager do próprio SteamOS, sem root nem pacotes extras, e enquanto está ligada o aparelho inteiro — downloads, Steam e jogos online — passa por ela.',
      ],
      [
        'E os jogos que eu adicionei de outro jeito?',
        'A aba Jogos mostra todos os atalhos não-Steam — emuladores, outros launchers, os que você adicionou à mão — e deixa trocar o Proton, os componentes, a arte e os patches, mover ou remover. O piShop só trata uma pasta como sendo do jogo quando é seguro: nunca a pasta pessoal, uma pasta compartilhada ou uma de onde outro atalho roda.',
      ],
      [
        'Por que o instalador baixa um navegador?',
        'A interface do piShop é uma PWA mostrada em tela cheia por um Chromium dedicado (Chrome for Testing, baixado direto do Google). Ele fica isolado na pasta do app — sem pop-ups, barra de tradução ou notificações — e é baixado uma vez só (~190 MB).',
      ],
      [
        'Minhas chaves e dados vão para algum lugar?',
        'Não. As chaves ficam só no seu aparelho, e o piShop não tem servidor próprio nem telemetria: ele conversa direto com os serviços que você configurou.',
      ],
      [
        'Isso é legal?',
        'O piShop é um cliente, como um navegador ou um app de torrent: ele não hospeda, não indexa e não distribui conteúdo. O que aparece nele depende das fontes que você configura — use apenas com conteúdo que você tem o direito de baixar.',
      ],
      [
        'Achei um bug. Como ajudo?',
        'Abra uma <a href="https://github.com/anarqz/pishop/issues">issue no GitHub</a> com o modelo do aparelho e o que aconteceu. Pull requests são bem-vindos.',
      ],
    ],
    en: [
      [
        'Does it work on my device?',
        "On any x86_64 machine running SteamOS: Steam Deck (LCD and OLED), ROG Ally / Xbox Ally, Legion Go S and others. piShop is built and tested on a ROG Xbox Ally X and at Steam Deck resolution. Similar systems such as Bazzite should work, but aren't tested.",
      ],
      [
        'Do I need root, a sudo password or developer mode?',
        "No. Everything lives in your home folder: the app in <code>~/Applications/piShop</code> and settings in <code>~/.local/share/piShop</code>. No pacman, nothing on the system partition — which is why SteamOS updates don't wipe piShop.",
      ],
      [
        'How do I update?',
        "You don't have to: piShop looks for new versions by itself, downloads them in the background and installs them the next time it starts (or right away from Settings → About). Running the command again updates too. Your settings and cover cache stay; the bundled browser is only downloaded again when its version changes.",
      ],
      [
        'How do I uninstall?',
        `<code>${CMD} -s -- --uninstall</code> removes the app and the Steam shortcut and keeps your settings. Add <code>--purge</code> to wipe everything.`,
      ],
      [
        'What do I need to set up?',
        "Nothing to get started: the Store already searches The Pirate Bay. Prowlarr (address and API key) is optional and adds your indexers; Discover uses the isitcracked API and game details can use TheGamesDB — without them piShop says so and keeps working. It all lives in Settings → Services — and someone's setup can be imported from a file or a link (a GitHub gist, Pastebin). The Steam store and SteamGridDB don't need a key.",
      ],
      [
        'Is there a VPN?',
        "Yes: import your provider's WireGuard (.conf) or OpenVPN (.ovpn) file in Settings → VPN and turn it on with the controller. It uses SteamOS's own NetworkManager, with no root or extra packages, and while it's on the whole device — downloads, Steam and online games — goes through it.",
      ],
      [
        'What about games I added some other way?',
        "Games lists every non-Steam shortcut — emulators, other launchers, the ones you added by hand — and lets you change their Proton, components, artwork and patches, move them or remove them. piShop only treats a folder as a game's own when that's safe: never your home folder, a shared folder or one another shortcut runs from.",
      ],
      [
        'Why does the installer download a browser?',
        "piShop's interface is a PWA shown full screen by a dedicated Chromium (Chrome for Testing, downloaded straight from Google). It stays isolated in the app folder — no pop-ups, translate bar or notifications — and is downloaded only once (~190 MB).",
      ],
      [
        'Do my keys or data go anywhere?',
        "No. Keys stay on your device, and piShop has no server of its own and no telemetry: it talks directly to the services you set up.",
      ],
      [
        'Is this legal?',
        "piShop is a client, like a browser or a torrent app: it doesn't host, index or distribute any content. What shows up in it depends on the sources you configure — only use it with content you have the right to download.",
      ],
      [
        'I found a bug. How can I help?',
        'Open an <a href="https://github.com/anarqz/pishop/issues">issue on GitHub</a> with your device model and what happened. Pull requests are welcome.',
      ],
    ],
  }

  const SCREENS = [
    {
      id: 'discover',
      pt: ['Descobrir', 'Os 5 cracks mais novos em destaque no estilo Big Picture — L2/R2 trocam — e a busca no centro.'],
      en: ['Discover', 'The 5 newest cracks in a Big Picture spotlight — L2/R2 flip through them — with search front and center.'],
    },
    {
      id: 'discover-grid',
      pt: ['Cracks recentes', 'Uma grade só, que vai carregando enquanto você desce — até o último jogo do isitcracked.'],
      en: ['Recent cracks', 'A single grid that keeps loading as you scroll — all the way to the last game on isitcracked.'],
    },
    {
      id: 'game',
      pt: ['Página do jogo', 'Sinopse, gêneros e datas da loja da Steam, arte do SteamGridDB e o status do crack.'],
      en: ['Game page', 'Synopsis, genres and dates from the Steam store, SteamGridDB art and the crack status.'],
    },
    {
      id: 'store',
      pt: ['Loja', 'Resultados do The Pirate Bay e do seu Prowlarr com tamanho, data, seeders e leechers — e o jogo no cabeçalho.'],
      en: ['Store', 'Results from The Pirate Bay and your Prowlarr with size, date, seeders and leechers — and the game in the header.'],
    },
    {
      id: 'release',
      pt: ['Torrent', 'Os detalhes do lançamento já trazem a arte e o crack do jogo encontrado. Um botão e ele começa a baixar.'],
      en: ['Release', "A release's details carry the matched game's art and crack info. One button and it starts downloading."],
    },
    {
      id: 'download',
      pt: ['Download', 'Em Downloads ou numa pasta que você escolher — com o espaço livre de cada lugar.'],
      en: ['Download', "To Downloads or a folder you pick — with each place's free space."],
    },
    {
      id: 'transfers',
      pt: ['Transferências', 'Cada download com a capa do jogo, um anel de progresso, velocidade e tempo restante.'],
      en: ['Transfers', "Every download with the game's cover, a progress ring, speed and time left."],
    },
    {
      id: 'install',
      pt: ['Instalado', 'Depois do instalador: o jogo está na Steam e em Jogos, e os arquivos baixados podem sair.'],
      en: ['Installed', "After the installer: the game is in Steam and in Games, and the downloaded files can go."],
    },
    {
      id: 'games',
      pt: ['Jogos', 'Todos os atalhos não-Steam: os instalados pelo piShop e, embaixo, todo o resto.'],
      en: ['Games', 'Every non-Steam shortcut: the ones installed through piShop, then everything else.'],
    },
    {
      id: 'game-page',
      pt: ['Configurar o jogo', 'Atalho, pasta e prefixo com o espaço livre de cada disco, o Proton, componentes e mover o jogo.'],
      en: ['Game setup', "Its shortcut, folder and prefix with each disk's free space, its Proton, components and moving it."],
    },
    {
      id: 'patches',
      pt: ['Patches', 'Correções nas opções de inicialização, ligadas e desligadas por jogo.'],
      en: ['Patches', 'Launch-option workarounds, switched on and off per game.'],
    },
    {
      id: 'components',
      pt: ['Componentes', 'Visual C++, DirectX e .NET dos instaladores da própria Steam, offline, e o winetricks para o resto — por jogo.'],
      en: ['Components', "Visual C++, DirectX and .NET from Steam's own installers, offline, and winetricks for the rest — per game."],
    },
    {
      id: 'explorer',
      pt: ['Explorar', 'Dois painéis: o NAS à esquerda, as pastas do EmuDeck à direita, o espaço de cada disco no alto. X marca, Y copia.'],
      en: ['Explore', "Two panes: your NAS on the left, EmuDeck folders on the right, each disk's free space on top. X marks, Y copies."],
    },
    {
      id: 'vpn',
      pt: ['VPN', 'As configurações WireGuard e OpenVPN do seu provedor, ligadas e desligadas pelo controle.'],
      en: ['VPN', "Your provider's WireGuard and OpenVPN configs, switched on and off with the controller."],
    },
    {
      id: 'settings',
      pt: ['Configurações', 'Fontes de jogos, serviços (The Pirate Bay, Prowlarr, TheGamesDB, isitcracked) com importar/exportar, limite de download, VPN, tela, idioma e atualizações.'],
      en: ['Settings', 'Game sources, services (The Pirate Bay, Prowlarr, TheGamesDB, isitcracked) with import/export, download speed cap, VPN, display, language and updates.'],
    },
  ]
  const DEVICES = { ally: { name: 'ROG Ally', w: 1920, h: 1080 }, deck: { name: 'Steam Deck', w: 1280, h: 800 } }

  // ---------------------------------------------------------------- language
  const norm = v => (v && /^pt/i.test(v) ? 'pt' : v && /^en/i.test(v) ? 'en' : null)
  const browserLang = () => ((navigator.languages || [navigator.language]).some(l => /^pt/i.test(l || '')) ? 'pt' : 'en')
  let lang = norm(new URLSearchParams(location.search).get('lang')) || norm(store.get('pishop-lang')) || browserLang()
  const tr = key => I18N[lang][key] ?? I18N.pt[key] ?? key
  // Screens are captured in each language; Portuguese ones at the root.
  const shotUrl = (dev, id) => `assets/screens/${lang === 'en' ? 'en/' : ''}${dev}/${id}.webp`

  function applyLang() {
    const d = I18N[lang]
    document.documentElement.lang = lang === 'pt' ? 'pt-BR' : 'en'
    document.title = d['meta.title']
    $('meta[name="description"]').content = d['meta.description']
    $$('[data-i18n]').forEach(el => {
      const v = d[el.dataset.i18n]
      if (v != null) el.textContent = v
    })
    $$('[data-i18n-html]').forEach(el => {
      const v = d[el.dataset.i18nHtml]
      if (v != null) el.innerHTML = v
    })
    $$('[data-i18n-attr]').forEach(el =>
      el.dataset.i18nAttr.split(';').forEach(pair => {
        const [attr, key] = pair.split(':')
        if (d[key] != null) el.setAttribute(attr, d[key])
      }),
    )
    $$('.lang button').forEach(b => b.setAttribute('aria-pressed', String(b.dataset.lang === lang)))
    $('#hero-ally').src = shotUrl('ally', 'discover')
    $('#hero-deck').src = shotUrl('deck', 'game')
    renderFeatures()
    renderRoadmap()
    renderFaq()
    renderTabs()
    showScreen(current, false)
    if (termStarted) playTerminal()
  }

  $$('.lang button').forEach(b =>
    b.addEventListener('click', () => {
      if (b.dataset.lang === lang) return
      lang = b.dataset.lang
      store.set('pishop-lang', lang)
      const url = new URL(location.href)
      url.searchParams.set('lang', lang)
      history.replaceState(null, '', url)
      applyLang()
    }),
  )

  // ---------------------------------------------------------------- sections
  function renderFeatures() {
    $('#feature-grid').innerHTML = FEATURES.map(f => {
      const [title, text] = f[lang]
      const tags = f.tags ? `<div class="tags">${f.tags.map(t => `<span>${t}</span>`).join('')}</div>` : ''
      const body = `<span class="icon">${icon(f.icon)}</span><h3>${title}</h3><p>${text}</p>${tags}`
      return f.shot
        ? `<article class="card wide"><div class="text">${body}</div><div class="shot"><img src="${shotUrl('ally', f.shot)}" width="1920" height="1080" alt="" loading="lazy" decoding="async"></div></article>`
        : `<article class="card">${body}</article>`
    }).join('')
  }

  function renderRoadmap() {
    $('#roadmap-grid').innerHTML = ROADMAP.map(r => {
      const [title, text, items] = r[lang]
      return `<article class="road"><span class="status ${r.status}">${tr('status.' + r.status)}</span><h3>${title}</h3><p>${text}</p><ul>${items
        .map(i => `<li>${i}</li>`)
        .join('')}</ul></article>`
    }).join('')
  }

  function renderFaq() {
    const open = $$('#faq-list details').map(d => d.open)
    $('#faq-list').innerHTML = FAQ[lang]
      .map(([q, a], i) => `<details${open[i] ? ' open' : ''}><summary>${q}</summary><div class="answer"><p>${a}</p></div></details>`)
      .join('')
  }

  // ---------------------------------------------------------------- copy
  let toastTimer
  function toast(msg) {
    const t = $('#toast')
    t.textContent = msg
    t.classList.add('show')
    clearTimeout(toastTimer)
    toastTimer = setTimeout(() => t.classList.remove('show'), 2400)
  }
  async function copyText(text) {
    try {
      await navigator.clipboard.writeText(text)
      return true
    } catch {
      const ta = document.createElement('textarea')
      ta.value = text
      ta.setAttribute('readonly', '')
      ta.style.cssText = 'position:fixed;top:0;left:0;opacity:0'
      document.body.append(ta)
      ta.select()
      let ok = false
      try {
        ok = document.execCommand('copy')
      } catch {}
      ta.remove()
      return ok
    }
  }
  $$('.cmd').forEach(box => {
    const btn = $('.copy', box)
    const label = $('[data-i18n="copy.btn"]', btn)
    let timer
    btn.addEventListener('click', async () => {
      const ok = await copyText($('code', box).textContent.trim())
      if (!ok) {
        const range = document.createRange()
        range.selectNodeContents($('code', box))
        getSelection().removeAllRanges()
        getSelection().addRange(range)
        toast(tr('copy.fail'))
        return
      }
      btn.classList.add('done')
      if (label) label.textContent = tr('copy.done')
      toast(tr('copy.toast'))
      clearTimeout(timer)
      timer = setTimeout(() => {
        btn.classList.remove('done')
        if (label) label.textContent = tr('copy.btn')
      }, 1800)
    })
  })

  // ---------------------------------------------------------------- gallery
  let device = 'ally'
  let current = 0
  let swapToken = 0
  const stageImg = $('#stage-img')
  const stageDevice = $('#stage-device')

  function renderTabs() {
    const tabs = $('#screen-tabs')
    tabs.innerHTML = ''
    SCREENS.forEach((s, i) => {
      const b = document.createElement('button')
      b.type = 'button'
      b.textContent = s[lang][0]
      b.addEventListener('click', () => {
        stopAuto()
        showScreen(i)
      })
      tabs.append(b)
    })
  }

  function showScreen(i, animate = true) {
    current = (i + SCREENS.length) % SCREENS.length
    const s = SCREENS[current]
    const dev = DEVICES[device]
    const url = shotUrl(device, s.id)
    stageDevice.className = `device ${device}`
    stageImg.alt = `${s[lang][0]} — ${dev.name}`
    $('#stage-caption b').textContent = s[lang][0]
    $('#stage-caption span').textContent = s[lang][1]
    const tabs = $('#screen-tabs')
    $$('button', tabs).forEach((b, j) => b.setAttribute('aria-current', String(j === current)))
    const active = tabs.children[current]
    if (active && tabs.scrollWidth > tabs.clientWidth) {
      tabs.scrollTo({ left: active.offsetLeft - tabs.clientWidth / 2 + active.clientWidth / 2, behavior: reduceMotion ? 'auto' : 'smooth' })
    }
    if (stageImg.getAttribute('src') === url) return
    const token = ++swapToken
    const swap = () => {
      if (token !== swapToken) return
      stageImg.width = dev.w
      stageImg.height = dev.h
      stageImg.src = url
      stageImg.classList.remove('fading')
    }
    if (!animate || reduceMotion) return swap()
    stageImg.classList.add('fading')
    const next = new Image()
    next.src = url
    const decoded = next.decode ? next.decode().catch(() => {}) : Promise.resolve()
    Promise.race([decoded, new Promise(r => setTimeout(r, 1500))]).then(() => setTimeout(swap, 120))
  }

  function preload(dev) {
    SCREENS.forEach(s => {
      const img = new Image()
      img.src = shotUrl(dev, s.id)
    })
  }

  $$('.seg button').forEach(b =>
    b.addEventListener('click', () => {
      stopAuto()
      device = b.dataset.device
      $$('.seg button').forEach(x => x.setAttribute('aria-pressed', String(x === b)))
      preload(device)
      showScreen(current)
    }),
  )
  $('#prev').addEventListener('click', () => {
    stopAuto()
    showScreen(current - 1)
  })
  $('#next').addEventListener('click', () => {
    stopAuto()
    showScreen(current + 1)
  })
  $('.gallery').addEventListener('keydown', e => {
    if (e.target.closest('.screen-tabs') || e.target.closest('.stage') || e.target.closest('.seg')) {
      if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
        e.preventDefault()
        stopAuto()
        showScreen(current + (e.key === 'ArrowRight' ? 1 : -1))
      }
    }
  })
  let touchX = null
  $('.stage').addEventListener('touchstart', e => (touchX = e.touches[0].clientX), { passive: true })
  $('.stage').addEventListener(
    'touchend',
    e => {
      if (touchX == null) return
      const dx = e.changedTouches[0].clientX - touchX
      touchX = null
      if (Math.abs(dx) > 40) {
        stopAuto()
        showScreen(current + (dx < 0 ? 1 : -1))
      }
    },
    { passive: true },
  )

  // Auto-advance while the gallery is on screen, until the visitor takes over.
  let autoTimer = null
  let autoStopped = reduceMotion
  function stopAuto() {
    autoStopped = true
    clearInterval(autoTimer)
  }
  new IntersectionObserver(
    ([entry]) => {
      clearInterval(autoTimer)
      if (entry.isIntersecting && !autoStopped) autoTimer = setInterval(() => showScreen(current + 1), 5500)
    },
    { threshold: 0.45 },
  ).observe($('.stage'))

  const lightbox = $('#lightbox')
  $('#stage-open').addEventListener('click', () => {
    stopAuto()
    $('img', lightbox).src = stageImg.src
    $('img', lightbox).alt = stageImg.alt
    lightbox.showModal()
  })
  lightbox.addEventListener('click', () => lightbox.close())

  // ---------------------------------------------------------------- terminal
  let version = 'v1.0.0'
  let termRun = 0
  let termStarted = false
  const TERM = {
    pt: {
      banner: 'para SteamOS',
      lookup: 'Procurando a versão mais recente…',
      app: 'Baixando o piShop…',
      browser: 'Baixando o navegador embutido (Chromium 154.0.8037.92, ~190 MB)…',
      install: 'Instalando em /home/deck/Applications/piShop…',
      installed: v => `piShop ${v} instalado.`,
      ask: 'A Steam está aberta. Fechar a Steam agora para registrar o atalho? [S/n]',
      yes: 's',
      closed: 'Steam fechada.',
      shortcut: 'Atalho criado na Steam (Biblioteca → Não-Steam).',
      done: 'Pronto!',
      next: '  Volte ao Modo de Jogo → Biblioteca → Não-Steam → piShop.',
      update: '  Daqui em diante o piShop se atualiza sozinho.',
    },
    en: {
      banner: 'for SteamOS',
      lookup: 'Looking up the latest version…',
      app: 'Downloading piShop…',
      browser: 'Downloading the bundled browser (Chromium 154.0.8037.92, ~190 MB)…',
      install: 'Installing into /home/deck/Applications/piShop…',
      installed: v => `piShop ${v} installed.`,
      ask: 'Steam is running. Close Steam now to register the shortcut? [Y/n]',
      yes: 'y',
      closed: 'Steam closed.',
      shortcut: 'Steam shortcut created (Library → Non-Steam).',
      done: 'All set!',
      next: '  Go back to Gaming Mode → Library → Non-Steam → piShop.',
      update: '  From now on piShop keeps itself up to date.',
    },
  }

  async function playTerminal() {
    termStarted = true
    const run = ++termRun
    const el = $('#term')
    const t = TERM[lang]
    const instant = reduceMotion
    el.textContent = ''
    const cursor = document.createElement('span')
    cursor.className = 'cursor'
    el.append(cursor)
    const alive = () => run === termRun
    const wait = ms => (instant ? Promise.resolve() : new Promise(r => setTimeout(r, ms)))
    const add = (text, cls) => {
      const s = document.createElement('span')
      if (cls) s.className = cls
      s.textContent = text
      el.insertBefore(s, cursor)
      return s
    }
    const type = async (text, cls, speed = 26) => {
      const s = add('', cls)
      for (const ch of text) {
        if (!alive()) return
        s.textContent += ch
        await wait(speed + Math.random() * 30)
      }
    }
    const line = async (parts, delay = 260) => {
      if (!alive()) return
      for (const [cls, text] of parts) add(text, cls)
      add('\n')
      await wait(delay)
    }
    const bar = async ms => {
      const s = add('', 'd')
      const width = 34
      const steps = instant ? 1 : 24
      for (let i = 1; i <= steps; i++) {
        if (!alive()) return
        const n = Math.round((width * i) / steps)
        s.textContent = '#'.repeat(n) + ' '.repeat(width - n) + ` ${((100 * i) / steps).toFixed(1)}%`
        await wait(ms / steps)
      }
      add('\n')
      await wait(200)
    }

    add('(deck@steamdeck ~)$ ', 'p')
    await wait(500)
    await type(CMD, null)
    if (!alive()) return
    add('\n')
    await wait(500)
    await line([['b', '\n  π  piShop'], ['d', `  ${t.banner}\n`]], 400)
    await line([['c', '› '], [null, t.lookup]], 500)
    await line([['c', '› '], [null, t.app]], 150)
    await bar(900)
    await line([['c', '› '], [null, t.browser]], 150)
    await bar(2000)
    await line([['c', '› '], [null, t.install]], 600)
    await line([['g', '✓ '], [null, t.installed(version)]], 500)
    if (!alive()) return
    add('? ', 'y')
    add(t.ask + ' ')
    await wait(900)
    await type(t.yes, 'b', 60)
    if (!alive()) return
    add('\n')
    await wait(900)
    await line([['g', '✓ '], [null, t.closed]], 500)
    await line([['g', '✓ '], [null, t.shortcut]], 400)
    await line([['b', `\n${t.done}`]], 120)
    await line([[null, t.next]], 120)
    await line([['d', t.update]], 300)
    if (alive()) add('\n(deck@steamdeck ~)$ ', 'p')
  }
  $('#term-replay').addEventListener('click', playTerminal)
  new IntersectionObserver(
    ([entry], obs) => {
      if (entry.isIntersecting) {
        obs.disconnect()
        playTerminal()
      }
    },
    { threshold: 0.35 },
  ).observe($('.terminal'))

  // ---------------------------------------------------------------- release
  const showVersion = v => {
    version = v
    $('#version').textContent = v
  }
  const cached = (() => {
    try {
      return sessionStorage.getItem('pishop-version')
    } catch {
      return null
    }
  })()
  if (cached) showVersion(cached)
  else
    fetch('https://api.github.com/repos/anarqz/pishop/releases?per_page=10')
      .then(r => (r.ok ? r.json() : []))
      .then(list => {
        const rel = list.find(r => !r.draft)
        if (!rel) return
        showVersion(rel.tag_name)
        try {
          sessionStorage.setItem('pishop-version', rel.tag_name)
        } catch {}
      })
      .catch(() => {})

  // ---------------------------------------------------------------- chrome
  const nav = $('#nav')
  const onScroll = () => nav.classList.toggle('scrolled', scrollY > 8)
  addEventListener('scroll', onScroll, { passive: true })
  onScroll()

  const revealables = $$('.stats, .section-head, .install-grid, .extra-cmds, .gallery, .features, .roadmap, .chips, .faq, .cta')
  if ('IntersectionObserver' in window && !reduceMotion) {
    const io = new IntersectionObserver(
      entries =>
        entries.forEach(e => {
          if (e.isIntersecting) {
            e.target.classList.add('in')
            io.unobserve(e.target)
          }
        }),
      { threshold: 0.08, rootMargin: '0px 0px -40px 0px' },
    )
    revealables.forEach(el => {
      el.classList.add('reveal')
      io.observe(el)
    })
  }

  applyLang()
  addEventListener('load', () => preload(device))
})()
