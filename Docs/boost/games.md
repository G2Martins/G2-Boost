# Jogos: biblioteca com capas, Preparar e jogar, Modo Jogo e rota até o servidor

**Última revisão:** 2026-10-25 (rodada 9: biblioteca em pôsteres com capas, Preparar e jogar como usuário comum, adicionar jogo e trocar .exe pela janela Abrir do Windows, prioridade alta por jogo) · 2026-10-22 (ajustes por jogo: núcleos via CPU Sets + GPU dedicada) · 2026-10-19 · Código: `crates/core/src/games.rs` (biblioteca, `launch_target`/`launch`, `pick_exe`, `plain_exe`/`exe_ok`), `covers.rs`, `icons.rs` (`png_sized`), `gamemode.rs`, `cpusets.rs`, `qos.rs`, `procs.rs` (`list_paths`, `raise_priority`, `normal_priority`, `set_cpu_sets`); shell `src-tauri/src/lib.rs` (`GamesState`, `games_*`, `game_mode_*`, `game_servers`, `game_mode_tick` no laço de rotinas, `RunEvent::Exit`) e `src-tauri/src/games_cmds.rs` (esquema `cover`, `game_launch`, `pick_exe`, `game_exe_reset`, `games_remove`); UI `src/pages/games.tsx`, `src/components/games/poster.tsx`, `src/components/games/route-panel.tsx`, `src/components/games/game-tweaks.tsx`, `src/lib/route.ts`; testes `games::tests`, `covers::tests`, `gamemode::tests`, `cpusets::tests`, `icons::tests`, `procs::tests::cpu_sets_on_a_child_process`, `tests/route.test.ts`, `e2e/games.spec.ts`. Viabilidade e o que exigiria servidores: [../network/game-routes.md](../network/game-routes.md).

## Biblioteca (`games::installed`)

Lida dos arquivos de cada launcher, sem rede e sem abrir nada:

| Fonte | Onde |
|---|---|
| Steam | `libraryfolders.vdf` + `appmanifest_*.acf` (nome, tamanho, `installdir` → pasta) |
| Epic | `%ProgramData%\Epic\EpicGamesLauncher\Data\Manifests\*.item` (JSON; só `AppCategories` com `games`, sem instalação incompleta; `AppName` só com caracteres simples; `LaunchExecutable` sem `..`) |
| GOG | `HKLM\SOFTWARE\WOW6432Node\GOG.com\Games\<id>` (`gameName`, `path`, `exe`) |
| Ubisoft | `HKLM\SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs\<id>\InstallDir` |
| EA app, Riot, Battle.net, Rockstar e outros | entradas `Uninstall` por fabricante (`classify`), sem launchers, anti-cheats, runtimes e ferramentas (`TOOL_WORDS`) |
| Adicionados pelo usuário | `settings.game_paths` (caminho completo escolhido na janela Abrir, rodada 9) e `settings.game_exes` (`nome.exe`, forma antiga, continua valendo) |

Pastas normalizadas (Riot grava com `/`), duplicatas por pasta e depois por nome, executável principal = o maior `.exe` plausível até 3 níveis (pastas `bin\win64`/`Binaries\Win64` primeiro). Na máquina do dono: CS2, EA FC 27, League of Legends, Teamfight Tactics, Megastore.

`games::library(settings)` é a lista da página (e de tudo que o shell usa: Modo Jogo, GPU, ícones, Preparar e jogar): `installed` + os jogos escolhidos por caminho + o exe que o usuário escolheu por jogo. O MCP ainda chama `games::installed(&game_exes)` (sem os jogos por caminho; trocar para `library` quando o `crates/mcp` for mexido).

**Jogando agora** (`games::running`): processo dentro da pasta de instalação (prefixo com `\` para não casar pasta irmã); para os adicionados à mão, o caminho exato do exe (escolhido por caminho) ou só o nome (forma antiga); ou `RunningAppID` da Steam. Jogo escolhido por caminho não tem pasta de instalação de propósito: a pasta dele pode ter outros jogos (`D:\Jogos\x.exe`), então nada casa por prefixo; o Modo Jogo usa a pasta do exe em execução para as próprias checagens (como já fazia com os adicionados por nome).

## Adicionar jogo e trocar o .exe (janela Abrir do Windows)

- `pick_exe(game)` abre a `IFileOpenDialog` nativa (filtro `*.exe`, dona = janela principal, thread COM STA própria, `FOS_FORCEFILESYSTEM | FOS_FILEMUSTEXIST | FOS_NOCHANGEDIR | FOS_DONTADDTORECENT`). **O webview nunca manda caminho:** ele diz só "adicionar" ou o id do jogo; o caminho é o que o usuário escolheu na janela do Windows, revalidado no core (`games::exe_ok`: letra de unidade + `:\`, sem UNC/`\\?\`, sem `..`, sem `:` depois da unidade (fluxo alternativo), `.exe`, sem caractere de controle, existe; dentro da pasta do jogo quando ele tem uma).
- **Adicionar jogo (.exe)** grava em `game_paths`; id `exe:<nome do arquivo>` (o mesmo de uma entrada antiga por nome, então Modo Jogo e ajustes continuam valendo; a entrada por nome some). Escolher de novo o mesmo nome de arquivo troca o caminho.
- **Trocar .exe** (diálogo Ajustes) grava em `game_exe_override[id]`; a janela abre na pasta do jogo. Vale para ícone, preferência de GPU, perfil do driver NVIDIA e para abrir jogos fora da Steam/Epic. Se o arquivo sumir, volta o detectado. "Voltar ao detectado" apaga a escolha; "Remover da biblioteca" (só adicionados à mão) tira o jogo da lista e mantém os ajustes dele.
- `game_paths` e `game_exe_override` são sanitizados ao carregar (só caminhos `plain_exe`, sem duplicata, limites de 50 e 200) e **só mudam por esses comandos**: `settings_set` mantém a cópia viva (como `game_tweaks`), então uma página com configurações velhas não desfaz nada nem injeta caminho.

## Capas (`covers.rs` + esquema `cover`)

- Só jogos da Steam têm capa (`steam:<appid>`, id canônico: dígitos, sem sinal nem zero à esquerda, cabe em u32). Ordem: cache do próprio cliente Steam (`HKCU\Software\Valve\Steam\SteamPath` → `appcache\librarycache\<appid>\library_600x900_2x.jpg` ou `library_600x900.jpg`, ou o layout novo `<sha1>\library_capsule.jpg`, pasta com nome de 40 hex), depois o nosso cache (`<data_dir>\covers\steam_<appid>.jpg`), depois **um** download da CDN da Steam (hosts https fixos: `shared.akamai.steamstatic.com` 600x900, depois `cdn.cloudflare.steamstatic.com` 300x450). Na máquina do dono os 3 jogos da Steam acharam capa no cache local (CS2 no layout antigo, os outros dois em `library_capsule.jpg`); jogos novos dão 404 na CDN sem o hash, por isso o cache local vem antes.
- Download no shell (`reqwest` do `tauri-plugin-http`, native-tls, sem crate nova), fora da thread da UI, no máximo 3 ao mesmo tempo (3 travas escolhidas pelo appid; o mesmo appid nunca baixa duas vezes em paralelo), sem redirecionamento, exige status 200, no máximo 2 MB lidos, só bytes JPEG/PNG (mágica conferida); gravação atômica (`.part` + rename). Resposta da CDN que não é capa (404, HTML, grande demais) vira marcador `steam_<appid>.miss` e não pergunta de novo por um dia; falha de rede não é lembrada.
- **Esquema `cover`** registrado no builder do Tauri: o webview carrega `http://cover.localhost/<id>` (capa) e `http://cover.localhost/icon/<id>` (ícone do exe em até 256 px, `SHDefExtractIconW` + WIC, cache em memória). O id é decodificado de forma estrita (`covers::decode_id`) e **precisa estar na lista instalada**; o arquivo é resolvido no Rust, nunca a partir da URL. Resposta com `Content-Type` pela mágica, `Cache-Control: max-age=86400`, `nosniff`; 404 com `no-store` no resto. CSP: `img-src` ganhou `http://cover.localhost`.
- Outros launchers não usam rede: o pôster mostra o ícone do exe grande sobre um fundo feito do mesmo ícone desfocado, com o nome na fonte de display; sem ícone, a inicial. No navegador (`bun run dev`) nada é pedido ao esquema e todos os pôsteres caem no ladrilho.
- **Verificado (2026-10-25, app real sem admin, build com o CSP de produção, dados em pasta temporária):** as 3 capas da Steam vieram do cache local (CS2 no layout antigo, EA FC 27 e Megastore em `library_capsule.jpg`, 300x450), League of Legends e Teamfight Tactics viraram ladrilho com ícone de 256 px, e o esquema recusou (404) um id fora da lista, um `..\..\windows\win.ini` codificado e um escape inválido. Download real da CDN (`games_cmds::tests::cover_download_live`, `#[ignore]`): CS2 baixa imagem; jogo novo (4080220) dá "sem capa". **Falta verificar:** Preparar e jogar com o app elevado (caminho pelo Explorer, conferir no Gerenciador de Tarefas que o jogo não está elevado) e a janela Abrir com clique humano (`games::tests::launch_live` / `pick_exe_live`, `#[ignore]`).

## Preparar e jogar (`game_launch`)

1. O id precisa estar na lista instalada.
2. Alvo (`games::launch_target`, puro e testado): Steam → `steam://rungameid/<appid>`; Epic → `com.epicgames.launcher://apps/<AppName>?action=launch&silent=true` com o `AppName` validado (ASCII simples, sem `..`, sem começar com `.`); o resto → o exe principal (o escolhido, se houver) iniciado na pasta dele, só se `plain_exe` e existir. Jogo conhecido só pelo nome não tem o que abrir (botão desligado, dica para Trocar .exe).
3. **Nunca herda administrador.** O G2 Boost costuma rodar elevado; elevado, o pedido passa pelo Explorer da área de trabalho (técnica do Raymond Chen: `IShellWindows::FindWindowSW(SWC_DESKTOP)` → `IServiceProvider::QueryService(SID_STopLevelBrowser)` → `IShellBrowser::QueryActiveShellView` → `IShellView::GetItemObject(SVGIO_BACKGROUND)` → `IShellFolderViewDual::Application` → `IShellDispatch2::ShellExecute`), então o jogo (e a Steam, se ela abrir agora) roda com os direitos normais do usuário. Sem Explorer, erro `shell_unavailable`, sem cair num início elevado. Sem elevação, `ShellExecuteExW` (`SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI`).
4. Se o Modo Jogo estiver desligado para o jogo, ele fica **armado só para essa partida** (em memória, `games_cmds::mode_games` soma o id à lista do laço de 5 s): a sessão começa quando o jogo aparece e o armado some quando ela termina (jogo fechou ou Encerrar), ou após 30 min se o jogo nunca abrir. O fluxo do Modo Jogo é o mesmo (sessão gravada antes, tudo desfeito ao fechar).

## Prioridade alta por jogo

`GameTweaks.high_priority` (diálogo Ajustes, seção Modo Jogo): o Modo Jogo sobe a prioridade desse jogo mesmo com a opção geral "Prioridade alta para o jogo" desligada (`opts.high_priority || tweaks.high_priority` em `gamemode::start`, mesmas checagens de processo dentro da pasta e sessão de usuário, nunca tempo real). Com a opção geral ligada, a chave aparece ligada e travada com o aviso de que já vale para todos.

## Página (pôsteres)

- Grade responsiva de pôsteres 2:3 (`.poster`: scrim, brilho e inclinação pelo ponteiro já no CSS), `<img loading="lazy">`, chips de launcher, "Jogando" e "Modo Jogo"; o pôster do jogo rodando ganha contorno brasa.
- Passar o mouse ou focar pelo teclado revela **Preparar e jogar** (botão primário tátil, o CTA da página), **Ajustes** (com a contagem de ajustes ligados) e **Rota** (abre o painel de rota abaixo e rola até ele). O véu e o deslize só animam com `prefers-reduced-motion: no-preference`.
- Busca sem acento ("tatica" acha "Tática") e filtro por launcher (chips com contagem). Ações no cabeçalho: **Adicionar jogo (.exe)** e **Procurar de novo**.
- Diálogo Ajustes: Modo Jogo para este jogo (antes era o switch do card) e Prioridade alta, núcleos e GPU (como antes), e a seção Executável (Trocar .exe, Voltar ao detectado, Remover da biblioteca). As chaves valem com Salvar; trocar o exe vale na hora.
- Ajuda de dev no navegador: `#/games/demo` mostra 4 jogos de exemplo (só fora do app), usada pelo `e2e/games.spec.ts` para busca, filtro, foco pelo teclado, diálogo e rota.

## Modo Jogo (`gamemode.rs`)

Liga por jogo (`settings.game_mode_games`, ids validados no core). Quando um jogo escolhido roda (laço de 5 s):

1. **Plano Desempenho máximo** (criado uma vez se faltar); o plano anterior é guardado e volta no fim.
2. **Prioridade alta** (nunca tempo real) só para processo cuja imagem está dentro da pasta do jogo, sessão de usuário; anti-cheat que nega acesso vira `priority_denied`, sem insistir.
3. **Liberar RAM** (purge da standby list) só com o app elevado; senão `needs_admin` e botão Elevar.
4. **Prioridade na rede** (`network_priority`, padrão ligado): política QoS por exe no `ActiveStore` (`MSFT_NetQosPolicySettingData` via WMI, DSCP 46), some ao reiniciar o Windows. Só com o app elevado (`qos_needs_admin` + Elevar). O provedor grava o nome em minúsculas, então o prefixo é `g2boost-` e a comparação ignora caixa; ao abrir, o app elevado apaga qualquer política `g2boost-*` que sobrou.
5. **Pausar rotinas** do G2 Boost até o jogo fechar.
6. **Núcleos do jogo** (ajustes por jogo, abaixo): `SetProcessDefaultCpuSets` no mesmo processo da prioridade (imagem dentro da pasta do jogo, sessão de usuário). O conjunto é gravado na sessão (`affinity`) **antes** de aplicar e desfeito (`SetProcessDefaultCpuSets(h, None)`) quando o jogo fecha, no Encerrar, ao sair e na recuperação após travamento. Anti-cheat que nega acesso vira `affinity_denied`, sem insistir.

**Verificado na máquina do dono (2026-10-19, elevado):** jogo falso (exe próprio) → `applied = power_plan, priority, ram, qos`, 82–125 MB liberados, prioridade 0x80, plano devolvido; roundtrip QoS criar → listar → limpar → 0 políticas sobrando. Sem admin: plano + prioridade aplicados e desfeitos ao fechar o jogo e após matar o app (recuperação na abertura).

A sessão (`gamemode_session.json`) é gravada **antes** da primeira mudança e desfeita quando o jogo fecha, quando o usuário clica Encerrar (fica desligado para aquele jogo até ele fechar), ao sair do app (`RunEvent::Exit`) e na próxima abertura depois de um travamento. Arquivo de sessão é entrada hostil (plano validado como GUID; `power::set_active` ainda confere a lista viva). Notificações do Windows: sem API documentada para pausar; a UI aponta a regra nativa "não perturbe ao jogar".

## Ajustes por jogo (`settings.game_tweaks`, botão "Ajustes do jogo" no card)

Por id de jogo validado (mesma regra de `game_mode_games`; entradas vazias são descartadas). Gravados **só** pelo comando `game_tweaks_set` (o jogo precisa estar na lista instalada); `settings_set` mantém o `game_tweaks` vivo, então uma página com cópia velha das configurações não desfaz nada.

| Opção | O que faz | Quando aparece |
|---|---|---|
| `perf_cores_only` | só a maior `EfficiencyClass` (núcleos P) | CPU híbrida |
| `free_core0` | tira o núcleo físico do processador lógico 0 e o irmão SMT (onde cai a maior parte das interrupções/DPCs) | mais de 1 núcleo |
| `physical_only` | um processador lógico por núcleo físico (sem SMT) | CPU com SMT |
| `gpu_high_perf` | preferência de gráficos "Alto desempenho" do Windows para o exe principal | 2+ adaptadores de vídeo (DXGI) |

- **Seleção = função pura** `cpusets::select` sobre a topologia de `GetSystemCpuSetInformation` (Id, Group, CoreIndex, LogicalProcessorIndex, EfficiencyClass); núcleo = (grupo, CoreIndex). As opções se combinam por interseção; opção que não se aplica (sem SMT, não híbrida) não corta nada; **nunca conjunto vazio** e, se nada mudaria, nada é aplicado. Testada com topologias sintéticas: híbrida 8P (SMT) + 16E, 8C/16T, 4C/4T, 1 núcleo, dois grupos de processadores. Teste real: `SetProcessDefaultCpuSets` num processo filho, lido de volta com `GetProcessDefaultCpuSets`, limpo, recusado fora da pasta e no próprio G2 Boost.
- **Valem no Modo Jogo** (aplicadas no início da sessão; mudar no meio vale no próximo lançamento). A UI avisa quando o Modo Jogo está desligado para o jogo e é honesta: ganho situacional, `free_core0` costuma perder com 4 núcleos ou menos, `physical_only` perde em jogos modernos, o Windows 11 já acerta P/E na maior parte dos casos.
- **GPU dedicada:** `HKCU\Software\Microsoft\DirectX\UserGpuPreferences`, valor = caminho completo do exe principal, editando **só** `GpuPreference=2;` na string (Auto HDR/VRR do mesmo valor ficam). Persistente (o Windows lê ao abrir o jogo). O `GpuPreference` anterior vai para o backup `gpu_pref` (uma entrada por exe) e volta ao desligar; sem backup, desligar remove a chave ("deixar o Windows decidir"). Do backup só volta um número (arquivo hostil não injeta chaves). Exe validado no core (`gamemode::gpu_exe`): absoluto com letra de unidade (sem UNC), sem `..`, `.exe`, dentro da pasta de instalação quando há uma, existente. Jogos adicionados só pelo nome (`exe:nome.exe`) não têm caminho: opção desligada com aviso (backlog: resolver pelo processo em execução).
- `game_tweaks_info` expõe o resumo da CPU (lógicos, físicos, SMT, híbrida, threads nos núcleos P) e a contagem de GPUs para esconder o que não se aplica.

## Rota até o servidor (`route-panel.tsx`, `route.ts`)

- **Destino:** servidores TCP que o jogo aberto usa (`game_servers`: conexões estabelecidas, públicas, de processos dentro da pasta do jogo); jogos da Steam: relay SDR mais próximo (`GetSDRConfig` + ping em lotes de 8); ou endereço digitado. UDP não tem endereço remoto nas tabelas do Windows (dito na tela).
- **Diagrama:** Você → saltos (`trace_host`, ICMP com TTL, sem admin) → destino, cada salto com zona (sua rede = privado; provedor = CGNAT e os 2 primeiros públicos, heurística sem ASN) e o ms que acrescenta.
- **Análise honesta:** um salto só é "o pior" se os seguintes continuam altos (roteador que responde ICMP devagar não é culpado); perda só conta se chega ao destino.
- **Reotimizar:** DNS mais rápido agora (`dns_rank`), Wi-Fi (sinal/banda), salto do roteador de casa (ms/perda), onde está o pior trecho e lembrete do teste de bufferbloat, cada um com botão para a tela que corrige.
