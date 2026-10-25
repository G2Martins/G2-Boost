# Overlay de jogo: FPS e sensores por cima do jogo

**Última revisão:** 2026-10-25 · Código: `crates/core/src/fps.rs` (sessão ETW, contagem de quadros, estatística), `crates/core/src/overlay.rs` (configurações, atalho global, posição, janela passiva); shell `src-tauri/src/overlay_cmds.rs` (`overlay_status`, `overlay_set`, `overlay_test`, janela `overlay`, worker) e ganchos em `src-tauri/src/lib.rs` (`setup`, `on_window_event`, `RunEvent::Exit`); capability `src-tauri/capabilities/overlay.json`; UI `src/pages/overlay.tsx` (rota `overlay`), janela `overlay.html` + `src/overlay/main.tsx` + `src/overlay/hud.tsx`; testes `fps::tests`, `overlay::tests`, `e2e/overlay.spec.ts`.

Uma janela à parte, transparente e sempre no topo, mostra FPS, tempo de quadro, 1% low, CPU, GPU, RAM, VRAM e o nome do jogo. **Nada é injetado no jogo e nenhum driver é instalado**: o FPS vem do registro de eventos do próprio Windows (ETW), o resto do monitor que o Painel já usa. Um atalho global mostra e esconde.

## FPS sem injeção (`fps.rs`)

Sessão ETW em tempo real `G2Boost-Present`, só API nativa (`StartTraceW`, `EnableTraceEx2`, `OpenTraceW`, `ProcessTrace`; nada de logman, xperf ou PowerShell). Os mesmos eventos em que o PresentMon (MIT) se apoia; ids, palavras-chave e campos lidos dos manifestos registrados no Windows 11 26200 (`Get-WinEvent -ListProvider <nome>`, na hora de desenvolver):

| Provedor | GUID | Eventos | Palavra-chave |
|---|---|---|---|
| Microsoft-Windows-DXGI | `{CA11C036-0102-4A2D-A6AD-F03CFED5D3C9}` | 42 `Present_Start`, 55 `PresentMultiplaneOverlay_Start` | Events `0x2` |
| Microsoft-Windows-D3D9 | `{783ACA0A-790E-4D7F-8451-AA850511C6B9}` | 1 `Present_Start` | Events `0x2` |
| Microsoft-Windows-DxgKrnl | `{802EC45A-1E99-4B83-9920-87C98277BA9D}` | 166 `Blit`, 168 `Flip`, 171 `PresentHistory_Start`, 215 `PresentHistoryDetailed_Start`; 184 `Present` (fim da chamada de present no núcleo) | Present `0x8000000` |

- **Contagem** (`Tracker`, puro): Direct3D 9/10/11/12 = um quadro por `Present_Start` do runtime, agrupado por swap chain (primeiro campo do evento, 4 bytes em processo de 32 bits); vale a swap chain mais ativa do processo (um widget ou launcher no mesmo processo não soma). Vulkan e OpenGL (processo sem present de DXGI/D3D9) = um quadro por chamada de present no núcleo numa thread: o primeiro evento de trabalho (blit, flip, histórico) abre, o `Present` 184 fecha, e o mesmo evento de abertura de novo sem fechamento é o próximo quadro. Present do DXGI também chega ao núcleo, por isso o caminho do núcleo só conta quando não houve present de runtime.
- **Estatística** (`fps::stats`, pura): FPS = quadros do último segundo; tempo de quadro = média do último segundo; **1% low** = média de FPS dos 1% quadros mais lentos da janela; **p99** = percentil 99 do tempo de quadro (posto mais próximo). Janela de 10 s ancorada no present mais novo (atraso de entrega não distorce). Intervalo acima de 1 s é pausa (alt-tab, carregamento), não quadro. Sem present por 2 s = o processo não está desenhando: sem número ("—"). Entrada fora de ordem, repetida, NaN/infinito ou no futuro é descartada.
- **Limites**: só os ids da tabela chegam à sessão (`EVENT_FILTER_TYPE_EVENT_ID`); buffers de 64 KB, no máximo 2 por CPU + 16; 16 swap chains × 4096 presents e 256 threads no rastreador (acima de 1000 FPS a janela encurta para uns 4 s). Antes de cada leitura a sessão leva `EVENT_TRACE_CONTROL_FLUSH`, então o overlay nunca espera o timer de 1 s.
- **Callback** sem `unwrap`, sem indexação, alvo comparado antes de qualquer trabalho; evento malformado ou curto conta o quadro sem swap chain (nunca entra em pânico: `extern "system"` aborta).
- **Administrador**: `StartTraceW` responde acesso negado sem elevação → `needs_admin`; o overlay mostra o resto e um escudo no lugar do FPS, a página mostra Elevar.
- **Ciclo da sessão**: aberta quando o overlay aparece, fechada quando ele some, quando é desligado e ao sair (o worker é dono dela). Uma sessão que sobrou de um travamento é parada antes de cada início e ao abrir o app elevado (`fps::stop_stale`).

## Janela `overlay` (`overlay_cmds.rs`, `overlay::show`/`hide`)

- Criada na primeira exibição, escondida: transparente, sem bordas e sem sombra, sempre no topo, fora da barra de tarefas, **não focável** (`WS_EX_NOACTIVATE`), sem arrastar e soltar. Conteúdo: `overlay.html` (entrada própria do Vite; carrega React, o dicionário e o HUD, não o app).
- **Mostrar, mover e esconder só por `overlay::show`/`hide`, na thread principal**: `SetWindowPos(HWND_TOPMOST, SWP_NOACTIVATE | SWP_SHOWWINDOW)` com `WS_EX_LAYERED | WS_EX_TRANSPARENT` (cliques atravessam), `WS_EX_TOOLWINDOW` (fora do Alt+Tab) e sem `WS_EX_APPWINDOW`. Nunca o `show()` do Tauri: ele chama `ShowWindow(SW_SHOW)`, que ativaria a janela e tiraria o foco do jogo depois de um atalho.
- **Posição**: o canto escolhido do monitor onde está a janela do jogo (`MonitorFromWindow`); o monitor inteiro quando o jogo cobre o monitor (sem bordas), a área de trabalho quando é janela; margem de 16 px × DPI. Tamanho lógico por estilo, métricas e escala (`overlay::window_size`), conteúdo ancorado no canto e o resto transparente e atravessável.
- **Alvo**: a janela em primeiro plano, menos o próprio G2 Boost e o shell do Windows (área de trabalho, barras, Iniciar, alternador de tarefas); trocar de app recomeça a contagem do zero. Nome: o da biblioteca de jogos quando o exe está dentro da pasta de um jogo (cache da lista de jogos), senão o nome do exe, sem caracteres de controle, até 48.
- **Worker**: a cada 500 ms emite `overlay:frame` (configuração + FPS + última amostra); a cada 1 s segue o jogo, reposiciona e amostra CPU, GPU e RAM (`monitor::Monitor`). Temperatura da CPU só quando um programa de sensores com memória compartilhada já roda (como no Painel); temperatura e clock da GPU por NVML (NVIDIA).
- A página da janela só escuta `overlay:frame` e `overlay:visible` (capability `overlay`: `listen`/`unlisten`, nada mais) e limpa os números ao esconder, para não reaparecer com dados velhos.
- **Desligar** = atalho liberado, worker parado (sessão ETW fechada) e janela destruída. "Testar agora" com o overlay desligado também destrói a janela ao esconder. Fechar a janela principal com a bandeja ligada mantém o app na bandeja com o atalho; sem bandeja, o overlay vai junto e o app sai. `RunEvent::Exit` libera tudo.

## Atalho global (`overlay::Hotkey`)

- `RegisterHotKey` numa thread dedicada com fila própria (`PeekMessage` cria a fila, `GetMessage` em laço, `WM_QUIT` encerra, `UnregisterHotKey` na saída), `MOD_NOREPEAT`. Sem plugin. O clique do atalho só entrega a troca para outra thread.
- **Lista segura**: F1 a F11, Insert, Home, End, Page Up, Page Down, Pause e Scroll Lock, com Ctrl, Alt e Shift. Nunca F12 (o Windows a reserva para depuradores), tecla Windows, letras, números, Esc, Tab ou Enter; Alt+F4 é recusado (fecha o jogo).
- Atalho já registrado por outro programa → `hotkey_taken`, mostrado na página. Funciona com o app na bandeja.

## Configurações (`settings.overlay`, validadas em `overlay::sanitize`)

| Campo | Padrão | Validação |
|---|---|---|
| `enabled` | desligado | registra o atalho |
| `hotkey` | `F7` | lista segura, forma canônica (`Ctrl+Shift+F8`); inválido volta ao padrão |
| `corner` | `top_left` | `top_left`, `top_right`, `bottom_left`, `bottom_right` |
| `metrics` | FPS, tempo de quadro, 1% low, uso de CPU, uso de GPU, temperatura da GPU, RAM | só conhecidas, sem repetição, na ordem do HUD; vazio volta ao padrão |
| `style` | `bar` | `bar` (uma linha) ou `list` (uma linha por grupo) |
| `opacity` | 70 | 0 a 100 (só o fundo; o texto fica opaco, com sombra) |
| `size` | 100 | 75 a 150 |

Gravadas pelo comando `overlay_set` (a página), que aplica na hora: atalho trocado, overlay escondido se foi desligado; a janela visível lê a configuração a cada quadro.

## Página `overlay`

- **Prévia**: uma tela de jogo fictícia (céu, brilho do accent no horizonte, morros); os quatro cantos são o seletor de posição. Valores de exemplo (rotulados) ou ao vivo quando o overlay está na tela.
- **Configurar**: ligar, atalho (Ctrl, Alt, Shift + tecla da lista), estilo Barra ou Lista, Fundo e Tamanho. Salva 250 ms depois da última mudança e ao sair da página.
- **O que mostrar**: métricas por grupo; a última ligada não desliga.
- **Como funciona**: notas honestas (abaixo) e Elevar quando falta administrador. **Testar agora** mostra o overlay sobre o que estiver na tela; `G2BOOST_AUTORUN=1` faz isso ao abrir a página (ajuda de QA).

## Limites, ditos na tela

- **Janela ou tela cheia sem bordas**: funciona. **Tela cheia exclusiva**: o jogo toma o monitor e o overlay não aparece. Nas otimizações de tela cheia do Windows o jogo costuma rodar como sem bordas e o overlay aparece.
- **Anti-cheat**: nada entra no processo do jogo (sem injeção, sem hook, sem driver); é uma janela separada e leitura de eventos do sistema.
- **Foco**: a janela nunca é ativada nem recebe cliques. Com o overlay visível o Windows compõe a tela por cima do jogo (vale para qualquer overlay); escondido, nada fica por cima.
- **Vulkan e OpenGL**: contagem pelo núcleo, estimativa. Direct3D é exato.
- **Memória**: com o overlay ligado a janela fica criada e escondida (um processo do WebView2 a mais); desligar libera.

## Verificação

- `cargo test -p g2boost-core -- fps overlay settings`: estatística com sequências sintéticas (60 FPS estável, travadas no 1% low e no p99, fora de ordem/repetido/NaN/futuro, parado, pausa e janela de 10 s), classificação de eventos e swap chain de 32 bits, rastreador (swap chain mais ativa, abre/fecha/repete do núcleo, duas threads, limites), layout do filtro de ids, `sanitize` com valores hostis (também pelo `settings::sanitize`), lista segura do atalho, cantos em monitores com coordenadas negativas, escala e caber na tela, detecção de sem bordas, registrar → duplicado recusado → liberar (atalho real `Ctrl+Alt+Shift+F11`).
- `e2e/overlay.spec.ts`: página sem backend, prévia segue estilo, canto, métricas, atalho e fundo; Testar agora desabilitado no navegador.
- **Verificado sem elevação (2026-10-25, exe de debug com os assets embutidos, pasta de dados isolada)**: a janela só é criada no primeiro atalho; com o app na bandeja (`--tray`) o atalho mostra e esconde; a janela em primeiro plano não muda nas duas vezes (o overlay não toma foco); estilos `TOOLWINDOW | LAYERED | TRANSPARENT | NOACTIVATE | TOPMOST`, sem `APPWINDOW`; `WindowFromPoint` sobre o overlay devolve a janela de baixo (cliques atravessam); na captura, só a placa aparece e o resto é transparente; posição no canto do monitor da janela em primeiro plano (um monitor à esquerda, coordenadas negativas); sem administrador o FPS mostra o escudo e o resto aparece. `G2BOOST_START_ROUTE=overlay` + `G2BOOST_AUTORUN=1` repete o teste visual.
- **Ao vivo (falta, precisa de administrador e de um jogo aberto)**: abra o jogo em janela ou sem bordas, anote o PID e, num terminal elevado na raiz: `$env:G2BOOST_FPS_PID = <pid>; cargo test -p g2boost-core -- --ignored fps_live --nocapture`. Cinco segundos de estatística, depois confere que a sessão some e reabre. Log em `%TEMP%\g2boost-live\fps.txt`.
