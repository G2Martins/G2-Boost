# Jogos: biblioteca, Modo Jogo e rota até o servidor

**Última revisão:** 2026-10-22 (ajustes por jogo: núcleos via CPU Sets + GPU dedicada) · 2026-10-19 · Código: `crates/core/src/games.rs`, `gamemode.rs`, `cpusets.rs`, `qos.rs`, `procs.rs` (`list_paths`, `raise_priority`, `normal_priority`, `set_cpu_sets`); shell `src-tauri/src/lib.rs` (`GamesState`, `games_*`, `game_mode_*`, `game_servers`, `game_mode_tick` no laço de rotinas, `RunEvent::Exit`); UI `src/pages/games.tsx`, `src/components/games/route-panel.tsx`, `src/components/games/game-tweaks.tsx`, `src/lib/route.ts`; testes `games::tests`, `gamemode::tests`, `cpusets::tests`, `procs::tests::cpu_sets_on_a_child_process`, `tests/route.test.ts`, `e2e/games.spec.ts`. Viabilidade e o que exigiria servidores: [../network/game-routes.md](../network/game-routes.md).

## Biblioteca (`games::installed`)

Lida dos arquivos de cada launcher, sem rede e sem abrir nada:

| Fonte | Onde |
|---|---|
| Steam | `libraryfolders.vdf` + `appmanifest_*.acf` (nome, tamanho, `installdir` → pasta) |
| Epic | `%ProgramData%\Epic\EpicGamesLauncher\Data\Manifests\*.item` (JSON; só `AppCategories` com `games`, sem instalação incompleta; `AppName` só com caracteres simples; `LaunchExecutable` sem `..`) |
| GOG | `HKLM\SOFTWARE\WOW6432Node\GOG.com\Games\<id>` (`gameName`, `path`, `exe`) |
| Ubisoft | `HKLM\SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs\<id>\InstallDir` |
| EA app, Riot, Battle.net, Rockstar e outros | entradas `Uninstall` por fabricante (`classify`), sem launchers, anti-cheats, runtimes e ferramentas (`TOOL_WORDS`) |
| Adicionados pelo usuário | `settings.game_exes` (`nome.exe`) |

Pastas normalizadas (Riot grava com `/`), duplicatas por pasta e depois por nome, executável principal = o maior `.exe` plausível até 3 níveis (pastas `bin\win64`/`Binaries\Win64` primeiro). Na máquina do dono: CS2, EA FC 27, League of Legends, Teamfight Tactics, Megastore.

**Jogando agora** (`games::running`): processo dentro da pasta de instalação (prefixo com `\` para não casar pasta irmã), nome do exe para os adicionados à mão, ou `RunningAppID` da Steam.

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

## Overlay de jogo (2026-10-25)

FPS, tempo de quadro, 1% low, CPU, GPU e RAM por cima do jogo, numa janela à parte, sem injeção e sem driver (eventos do próprio Windows), com atalho global: [overlay.md](./overlay.md). O nome do jogo no overlay vem desta biblioteca (exe dentro da pasta de instalação, `games::running`).
