# Roadmap — fases e backlog

**Última revisão:** 2026-10-25 (rodada 9: paridade com o concorrente mapeado, capas dos jogos, overlay, debloat, instalar apps, release público) · 2026-10-23 (rodada 8: atualizações de apps e do Windows, Manutenção+, sempre abrir como administrador, monitor, auditoria ponytail) · 2026-10-22 (rodada 7 planejada: BIOS, ajustes por jogo, limpeza do sistema + reparo, auditoria de exposição, configurações 3D do driver) · 2026-10-19 (rodada 6 fechada com testes ponta a ponta, só Wi-Fi sem hardware para testar; assinatura/SmartScreen e PawnIO/driver próprio fora do roadmap por decisão do dono)

**Estado:** Fases 0 ✅, 1 (núcleo) ✅, 3 (núcleo) ✅ em 2026-10-14; **Fases 4 (Boost), 2 (Limpeza), 5 (Rotinas) e 6 (Painel/Meu PC/Monitor) ✅ em 2026-10-15** (verificação com admin pendente pelo dono). **6.5 Meu PC completo ✅** (CPU-Z/Speccy user-mode + WinSAT + relatório). **9 IA + MCP ✅** (conselheiro local, assistente no app com 3 provedores, `g2boost-mcp.exe` só leitura). **2026-10-16:** disco (filtros, Lixeira, mostrar na pasta, essenciais) ✅, rede (TCPing, gateway, DNS Optimizer) ✅, BIOS/firmware + sensores externos ✅, inicialização + processos ✅, IA endurecida ✅, licença/READMEs/avisos de terceiros ✅, MCP no próprio exe (`--mcp`) ✅, instalador NSIS com licença ✅ (sem assinatura). Próximas: **rodada 6** (abaixo), MFT. Planejamento GSD em `.planning/`.

Ordem pensada pela dor: o C: do dono está com 40 GB livres e os scripts .bat estão bloqueados pelo Kaspersky — disco e limpeza primeiro; rede e boost depois; MCP quando o core estiver estável.

| Fase | Entrega | Critério de pronto |
|---|---|---|
| **0 — Decisões + ambiente** | Stack e nome do produto decididos · `git init` + `.gitignore` · toolchain instalado · scaffold Tauri (React/TS/Tailwind/shadcn) · Biome + clippy + CI lint · `.planning/` via `/gsd-new-project` | app vazio abre com o shell estilo ExitLag (sidebar + topbar + página placeholder) |
| **1 — Analisador de disco** (scan + treemap + maiores ✅ 2026-10-14; filtros, Lixeira, mostrar na pasta, essenciais ✅ 2026-10-16; falta snapshot, MFT) | scan paralelo (walk; MFT depois) · árvore · treemap canvas · filtros (`*.ext`, `\|`, `\pasta`, `>1gb`, `<3months`, atributos) · zoom/drill · tooltip · top-N · "abrir no Explorer" · mover p/ Lixeira com confirmação · snapshot | scan do C: em < 30 s sem admin (hoje 64 s para 1,43 M arquivos — MFT no backlog); testes sandbox + segurança (junction) |
| **2 — Limpeza** ✅ 2026-10-15 (falta: essenciais grandes → fase 6; rotinas → fase 5) | catálogo portado dos .bat com caminhos reais · prévia por categoria · `ConfirmToken` · execução nativa junction-safe · histórico · Lixeira/DNS/`StartComponentCleanup` | substitui os dois .bat; Kaspersky silencioso no teste local (a confirmar elevado) |
| **3 — Rede** ✅ 2026-10-16 (TCPing, gateway no monitor, DNS Optimizer com aplicar/restaurar) | monitor (ICMP gateway/8.8.8.8/1.1.1.1; latência/jitter/perda/picos; gráfico) · speedtest Cloudflare + bufferbloat (nota A+…F) · ping p/ jogos (Steam SDR + lista curada) · MTR/Ping/TCPing · DNS ranking + aplicar/restaurar | paridade com as abas "Analisador de rede" e "DNS Optimizer" do ExitLag |
| **4 — PC Boost** ✅ 2026-10-15 (falta: rotina de RAM → fase 5) | 16 tweaks `detect/apply/revert` + backup exato · efeitos visuais em 4 níveis · serviços (SysMain, telemetria, WSearch, Xbox) · presets Padrão/Alto desempenho/Personalizado · plano de energia · purge da standby list · Elevar via `runas` | paridade com "Ajustes de sistema" e "Limpeza de RAM" (manual) |
| **6 — Painel, Meu PC e Monitor** ✅ 2026-10-15 | KPIs (pontuação por hardware, pontos de restauração, bytes para limpar, RAM) · Boost agora · monitor CPU/GPU/RAM/disco/rede 1 s · Meu PC (SMBIOS/DXGI/IOCTL/TPM/Secure Boot) · recomendações por hardware · tweaks privacidade/aparência · `hvci_off` beta | painel abre sem admin em < 2 s com dados reais (verificado por screenshot) |
| **5 — Rotinas** ✅ 2026-10-15 | iniciar com o Windows (tarefa de logon `--tray`, Highest se elevado) · limpeza agendada (ao ligar / a cada N h / diária / semanal) com categorias · limpar RAM ao abrir jogo (Steam `RunningAppID` + exes + `steamapps\common`) · bandeja · Configurações | app vive na bandeja e limpa sozinho; tudo visível no histórico (verificar logon real + Highest pelo dono) |
| **8 — MCP local** ✅ só leitura (13 tools, `G2 Boost.exe --mcp`); destrutivas com token = backlog | `g2boost-mcp.exe` (stdio) · tools read-only + destrutivas com token · registro no Claude Desktop/Code · doc de uso | Claude da máquina analisa o scan e propõe limpeza; humano confirma na UI |
| **9 — Release** (NSIS ✅ 2026-10-16; **assinatura de código / "baixar sem aviso do Windows" fora do roadmap**, decisão do dono 2026-10-18; pesquisa guardada em [../release/distribution-and-trust.md](../release/distribution-and-trust.md)) | instalador NSIS · auto-update · submissão de falso positivo (Kaspersky/Microsoft) · README público | instala numa VM com Kaspersky sem bloqueio |

## Rodada 9 (2026-10-25, pedido do dono: prints do Galdino Control v2.2.1, "máximo de features que fazem sentido juntas", capas dos jogos, interface com efeitos sem cara de IA, revisão ponytail e release com instalador + portátil)

Decisões do dono: tudo, inclusive o overlay; antivírus = detectar qual está ativo e adaptar (Defender: exclusão automática com confirmação e desfazer, só com o exe em pasta protegida; Kaspersky, Avast, AVG, ESET, Norton, Bitdefender, McAfee, Malwarebytes, Trend Micro, Panda: passo a passo); publicar o release no GitHub. Fora de propósito: driver PawnIO (decisão anterior), limpar Prefetch, "Windows Update só avisa", aplicar ajustes a "contas novas" (perfil Default), PowerShell por baixo. Mapeamento interno em [../references/galdino-optimizer.md](../references/galdino-optimizer.md).

| Frente | Como | Estado |
|---|---|---|
| Navegação | barra lateral agrupada (Principal / Otimizar / Ferramentas / Sistema) com rótulos, recolhível, marcador que desliza; Meu PC vira rota; 9 rotas novas | ✅ |
| Visual | tinta deslizante nas abas, título com máscara + fio, toasts, `Meter` com ponta acesa, `.poster` (capa 2:3 com brilho e inclinação), botão tátil, Lucide traço 1,5, sem glifos de texto e sem o ícone de "IA" | ✅ |
| Limpeza | anel do disco do sistema + barras por categoria da última prévia | ✅ |
| Modo portátil | `portable.txt` ao lado do exe: dados em `<pasta do exe>\data` | ✅ |
| Jogos | capas (cache da Steam, CDN da Steam 1 vez, ícone do exe), "Preparar e jogar" sem herdar admin, seletor de exe, prioridade por jogo | ✅ |
| Debloat + ajustes | página Debloat (Essencial/Recomendado/Avançado, IA do Windows, apps de brinde), ~30 ajustes novos, busca/filtros/"Ver o que muda" | ✅ |
| Perfis e restauração | perfis com nome, "Sem IA", "Padrão do Windows", desfazer tudo, histórico; Mouse e teclado | ✅ |
| Manutenção+ e antivírus | revisão semanal num clique, redefinir Windows Update, impressora, áudio; antivírus detectado + exclusão/guia | ✅ |
| Monitor de hardware + Check-up | árvore de sensores com mín./máx. e CSV; check-up de ~20 medições | ✅ |
| Instalar apps | catálogo curado via winget, em lote | ✅ |
| Ferramentas | teste do mouse (polling), teste de monitor, perda de pacotes | ✅ |
| Overlay de jogo | FPS/tempo de quadro/1% low por ETW (sem injeção), janela por cima, atalho F7 | ✅ |
| Painel + pós-formatação | chip de pendentes, velocímetro com Boost agora (ajustes + limpeza + RAM, confirmado), card e assistente pós-formatação | ✅ |
| Tela antes do UAC | "Sempre abrir como administrador" mostra a explicação do G2 Boost e só então pede o UAC; recusar abre em modo limitado | ✅ |
| Release | v0.9.0: instalador NSIS (por máquina) + MSI pt-BR/en + zip portátil + SHA256, WebView2 embutido, README com FAQ | ✅ |

**Verificado na máquina do dono (2026-10-25/26, elevado):** ida e volta dos 33 ajustes da rodada 9 (2 bugs achados e corrigidos na 0.9.1: chave do Widgets protegida pelo UCPD, núcleos mínimos com faixa 0..0), revisão semanal completa (DISM = reparável), Preparar e jogar abre o programa sem admin, overlay conta FPS (Edge = 120 FPS em 120 Hz; bug de horário do ETW corrigido na 0.9.2), áudio + impressora + redefinir Windows Update + apagar backups, instalar/abrir/desinstalar NSIS e MSI, tela antes do UAC, 3 ferramentas MCP novas por JSON-RPC.

**Pendente do dono:** remover o componente Recall de verdade (pede reinício), apps de brinde removidos de verdade, exclusão no Defender numa máquina sem outro antivírus, FPS num jogo real. Antes de o Kaspersky reagir a gravações em `Policies` ou à parada de `wuauserv`, registrar em [../security/kaspersky-false-positive.md](../security/kaspersky-false-positive.md).

## Rodada 8 (2026-10-23, pedido do dono: "winget upgrade --all", cheatsheet de comandos, ponytail, sempre admin)

| Frente | Como | Estado |
|---|---|---|
| Atualizar todos os apps | Apps › Atualizações: `winget::upgrades()` + `install(id, true)` por pacote (nunca `--all`), token preso à lista ordenada, excluídos, parar entre pacotes, histórico (`updates.rs`) | ✅ |
| Windows Update | WUA COM (`IUpdateSession`): busca sem admin (cache ou online), baixar + instalar com admin, histórico, reiniciar só se pendente | ✅ |
| Do cheatsheet | verificar disco (`MSFT_Volume.Repair` Scan/SpotFix), otimizar unidades (`Optimize` Analyze/ReTrim/Defrag por tipo), reiniciar nas Opções avançadas (`SHUTDOWN_RESTART_BOOTOPTIONS`), renovar IP + limpar DNS + abrir a redefinição de rede do Windows (`volumes.rs`, `netrepair.rs`) | ✅ |
| Fora (de propósito) | `net user /add` e admin novo (risco), `bcdedit`/`bootrec` (pode impedir o boot), `netsh winsock reset` (sem API: o app abre a página do Windows) | — |
| Sempre abrir como administrador | Configurações: um UAC por abertura normal (como o ExitLag), nunca no `--mcp`/`--tray`; recusar mantém a cópia sem elevação. Sem UAC só com tarefa agendada = desvio de UAC com exe em pasta gravável: recusado | ✅ |
| Monitor em tempo real | cadência fixa de 1 s, disco = tempo ativo, gráfico que rola, gauge que desliza | ✅ |
| Auditoria ponytail | 3 lotes: helpers duplicados unificados (`is_reparse`, `system_root`, FILETIME, caminho de processo, `blocking()` nos comandos), 12 chaves i18n e 2 deps JS mortas, lib só `rlib`. Rejeitados após checagem: remover `windows-core` (a macro `#[implement]` precisa), unificar FNV (mudaria chaves gravadas) | ✅ |

**Verificado na máquina do dono (2026-10-23):** elevado: winget atualizou o GitHub CLI (`ok`), WUA instalou o driver AOC pendente (`ok`, sem reinício), C: verificado online (saudável, 217 s) + ReTrim (2,6 s), D: (USB, tipo desconhecido) só Analyze (5 %), o core recusou Analyze em SSD (`NotAllowed`). "Sempre abrir como administrador": abertura normal → UAC aceito → cópia elevada aberta e a sem elevação encerrada. Kaspersky acusou um binário de **teste** (`PDM:Trojan.Win32.Generic`, ver [../security/kaspersky-false-positive.md](../security/kaspersky-false-positive.md)). **Pendente:** renovar IP e reiniciar nas Opções avançadas pela UI (derrubam a conexão / reiniciam), release com Kaspersky ativo.

## Rodada 7 (2026-10-22, mapeamento do dono: prints de concorrente + ideias)

Decisões: as 4 frentes abaixo entram; exceção de `sfc.exe`/`mrt.exe` aberta ([global-rules.md §1.3](./global-rules.md)). Já existiam: `svc_sysmain` e tarefas via `Kind::Tasks`. Servidor MCP do NVIDIA App (`127.0.0.1:13508`, beta 11.0.9.565): o G2 Boost não usa (lê a GPU pela NVML), não há medição de perda de FPS (em repouso fica no `HTTP.sys`, pid 4); desligar se o usuário não usa agente com ele.

| Frente | Como (nativo) | Estado |
|---|---|---|
| Guia de BIOS + Reiniciar na BIOS | checklist dos achados de `firmware.rs` + caminho do menu por fabricante da placa (SMBIOS) + `OsIndications` (`SetFirmwareEnvironmentVariableExW`, admin) | ✅ 2026-10-22 |
| Ajustes por jogo | liberar núcleo 0 / só físicos (sem SMT) / só núcleos de desempenho via CPU Sets no Modo Jogo (desfeito ao fechar) + GPU dedicada por exe (`UserGpuPreferences`, com backup) | ✅ 2026-10-22 |
| Tarefas e serviços extras | WinSAT, DiskDiagnostic, Feedback\Siuf, WER QueueReporting; Spooler sem impressora. Nunca `ScheduledDefrag` (TRIM), `UpdateOrchestrator`, Defender | ✅ 2026-10-22 |
| Limpeza de arquivos do sistema + reparo | handlers `IEmptyVolumeCache` (`VolumeCaches`) no fluxo prévia + token; DISM RestoreHealth (`DismApi`); SFC e MRT pela exceção | ✅ 2026-10-22 |
| Auditoria de acesso remoto e exposição | RDP/NLA, Assistência Remota, WinRM, SMBv1, firewall por perfil (`INetFwPolicy2`), portas abertas no roteador via UPnP (`NATUPnP`), apps remotos, conexões de entrada da internet, sessões (`WTS`), tarefas/serviços fora da Microsoft sem assinatura; monitor no laço de rotinas. Não é antivírus (dito na tela) | ✅ 2026-10-22 |
| Configurações 3D do driver | NVIDIA: NVAPI DRS (global e por jogo, escrita sem admin, testada ida e volta). AMD: ADLX (sem placa AMD aqui para testar). Seção escolhida pelo vendor id do DXGI | ✅ 2026-10-22 |

**Verificado elevado na máquina do dono (2026-10-22):** EFI aceita reiniciar na BIOS (`OsIndicationsSupported` = 0x1f; o reboot real fica para o dono); Assistência Remota desligar → desfazer ok; tarefa WinSAT ida e volta ok; Modo Jogo aplicou `power_plan, priority, affinity, ram, qos`; DISM scan = **reparável** (CBS.log: 1.495 corrupções de payload, 0 reparadas), SFC saudável, MRT limpo, sem reação do Kaspersky; Arquivos do sistema: 10 de 11 handlers só têm `IEmptyVolumeCache` v1 (bug achado e corrigido com fallback; Update Cleanup = 190 MB); Segurança elevada achou 1 tarefa sem assinatura (MATLAB Startup Accelerator). **Pendente:** reiniciar na BIOS de verdade, DISM RestoreHealth pela UI, AMD em hardware real.

## Rodada 6 (2026-10-18, pedido do dono: "siga com essas frentes")

Decisões: **sem PawnIO e sem driver próprio** (temperatura só por software gratuito de terceiros que o usuário já roda: HWiNFO, LibreHardwareMonitor e os que a pesquisa indicar); assinatura de código fora do roadmap.

| Frente | Estado |
|---|---|
| Jogos: lista multi-launcher (Steam, Epic, EA, Riot, Battle.net, Ubisoft, GOG + exe à mão) com chave Modo Jogo | ✅ ([../boost/games.md](../boost/games.md)) |
| Modo Jogo de um clique (energia, prioridade, RAM, pausar rotinas; tudo volta ao fechar, ao sair e após travamento) | ✅ |
| Diagrama da rota real até o servidor do jogo · Reotimizar honesto (DNS, Wi-Fi, roteador, onde piora, bufferbloat) | ✅ |
| Prioridade do jogo na rede (QoS por exe, `MSFT_NetQosPolicy`, admin) | ✅ 2026-10-19, verificado elevado |
| Histórico de câmera/microfone · quem usa a rede · Wi-Fi (segurança + canais) · saúde do disco com alerta · tempo de boot | ✅ ([../boost/privacy-health.md](../boost/privacy-health.md)) |
| Limpeza +16 categorias · apps pouco usados · duplicados · regras de prioridade por processo | ✅ |
| Temperatura da CPU por software gratuito de terceiros | ✅ decidido: HWiNFO (grátis, memória compartilhada) e LHM se já rodam; Core Temp/Afterburner fora (drivers flagrados) |
| Bytes por processo na rede (ETW, admin) · SMART SATA com admin · RAM no Modo Jogo · tempo de boot elevado | ✅ 2026-10-19, verificado na máquina do dono |
| Wi-Fi testado em PC com adaptador | pendente (este PC é cabeado, sem serviço WLAN) |

## Feito (2026-10-16 — "finalize tudo")
- **Sensores sem driver próprio:** pesquisa de opções ([../security/kernel-driver-options.md](../security/kernel-driver-options.md)) → v1 `sensors.rs` (HWiNFO shared memory / LibreHardwareMonitor WMI quando o usuário roda); temp. da CPU no monitor, no contexto da IA e no achado `cpu_hot`. WinRing0 proibido; PawnIO = v2 opt-in; driver próprio = v3 só se preciso.
- **BIOS/firmware:** `firmware.rs` (UEFI/legado, virtualização com VBS, ReBAR via NVML BAR1, XMP, idade da BIOS, Fast Startup, hibernação) + achados `legacy_boot`, `rebar_off`, `bios_old`; card no Meu PC. Na máquina do dono: ReBAR desligado e BIOS de 3,4 anos.
- **Inicialização + processos:** `startup.rs` (semântica do Gerenciador de Tarefas, Segurança do Windows protegida), `procs.rs` (top por exe, só leitura); aba nova no PC Boost.
- **Tweak novo:** `windowed_optimizations` (flip model para jogos em janela, edita só a própria chave da string DirectX). Itens dos vídeos avaliados e recusados documentados em [../boost/tweaks.md](../boost/tweaks.md).
- **IA:** playbook de segurança/temperatura/FPS/input lag/processos no system prompt, ações de inicialização (validadas contra a lista ao vivo), tweaks beta fora das ações, máx. 6 por lista, `tests/ai.test.ts`; contexto com firmware, sensores, inicialização (só nome do exe) e processos.
- **Disco:** filtro (`*.mp4 >1gb >6m`), mostrar na pasta (shell API), Lixeira com guarda (`SafePath` + raízes protegidas + aviso do próprio Windows se não couber), essenciais grandes com explicação. Lixeira real verificada.
- **Rede:** TCPing, gateway no monitor, DNS Optimizer (10 provedores, ranking por consulta UDP própria, aplicar/restaurar por adaptador com backup).


## Feito (2026-10-15, madrugada — sessão autônoma após o dono decidir o escopo)
- **Fase 4 (Boost):** `registry.rs` (HKCU/HKLM + backups JSON), `visualfx.rs` (17 itens do diálogo, 4 níveis, round-trip ao vivo ok), `boost.rs` (16 tweaks, SCM, SPI, energia, purge, `runas`); comandos Tauri; aba Boost com estado real, presets, níveis, pills de serviço, Elevar. Screenshot verificado: nível Média detectado = setup do dono.
- **Fase 2 (Limpeza):** `cleanup.rs` (catálogo, prévia, `TokenStore`, execução, histórico), `tasksched.rs` (COM: rodar `StartComponentCleanup`, tarefa de logon), `system::running_processes`; comandos Tauri; página com prévia, painel de confirmação, progresso, resultado e histórico. Teste de junction NTFS real.
- **Fase 5 (Rotinas):** `settings.rs`, `routines.rs` (agenda + detector de jogo), tarefa de logon via `tasksched.rs`, thread de rotinas + bandeja no shell Tauri, página Configurações nova, aba RAM religada (jogo atual, último purge, lista de exes). Round-trip da tarefa de logon como usuário comum: ok.
- **Fase 6 (Painel):** `hardware.rs` (SMBIOS parser testado, DXGI, storage IOCTLs com SSD/HD, TPM, Secure Boot), `monitor.rs` (PDH + NVML), `restore.rs` (WMI + `SRSetRestorePointW`), `recommend.rs` (regras + score), 9 tweaks novos com categoria; shell com `Dashboard` (perfil cacheado, monitor refcount, restore, recomendações); UI Painel (KpiCard, Gauge, LiveLine), Meu PC, badges de categoria/motivo no Boost, ponto de restauração antes do preset. Pesquisa de serviços com fontes em `Docs/references/windows-services-research.md` (Xbox saiu do Alto; HVCI = única alavanca real, opt-in).
- **Fase 6.5:** Meu PC no nível user-mode do CPU-Z/Speccy (CPUID, caches, microcode, pentes, BIOS, driver/PCIe/potência GPU, monitores com Hz máximo, WinSAT ler/rodar, relatório, RAPL, clock efetivo). Análise estática dos dois: ambos usam o driver `cpuz.sys`.
- **Fase 9:** `advisor.rs`, `context.rs`, `steam.rs`, `secrets.rs`, `crates/mcp` (9 tools só leitura, testado por JSON-RPC), página Assistente IA, card de sugestões no Painel.
- **GSD:** `.planning/` criado (config yolo, PROJECT/REQUIREMENTS/ROADMAP/STATE). `bun run check` verde (49 testes).
- **Validado e descartado:** limpar Prefetch (1 arquivo / 0 MB na máquina do dono; SysMain off não grava `.pf`; anti-forense).

## Backlog (não prometido)
- Limitador de tráfego local via QoS (`MSFT_NetQosPolicy`) — ExitLag faz pelo túnel; aqui seria por política do Windows.
- Overlay de FPS/ping in-game.
- Agendador (limpeza diária/mensal em segundo plano via Task Scheduler API).
- Tema claro.
- Leitura de MFT para scan instantâneo (exige admin).

## Feito (2026-10-14, noite — sessão autônoma, revisar de manhã)
- Stack decidida (Tauri 2) · nome **G2 Boost** · `git init` com commits atômicos · `.gitignore`/`.gitattributes`/`bunfig.toml` (exact + 7 dias).
- Toolchain em D: sem admin: Rust 1.99 (`D:\dev\rustup|cargo`), `xwin` (CRT + SDK libs), `rc.exe` do NuGet, `D:\dev\cargo\config.toml` (rust-lld), stub `tools/vswhom-sys-stub`. Build Tools ficou pendente (UAC cancelado).
- Scaffold Tauri 2 + React 19 + Vite 8 + Tailwind 4 + Radix + lucide + Biome; Cargo workspace (`crates/core` + `src-tauri`); ícones gerados do logo `src-tauri/icons/g2boost.svg`.
- Shell estilo ExitLag (sidebar/topbar/banner/abas), 5 páginas, i18n pt-BR/en tipado (simetria garantida pelo `tsc`), tokens de design.
- **Funcional:** Scanner de rede (speedtest Cloudflare: download/upload/latência ociosa e sob carga/jitter/bufferbloat A+…F/atividade/ISP/IP/cidade) · **Monitoramento em tempo real** (ICMP a cada 1 s para Google e Cloudflare; saúde, latência, perda, jitter, picos; gráfico de 60 s com marcadores) · **Ferramentas Ping e MTR** (ICMP nativo com TTL incremental, sem `ping.exe`) · **Ping para jogos** (Steam Datagram Relay: CS2, Dota 2, Deadlock, por região) · drives com espaço livre · barra de RAM ao vivo · Sobre (versão, OS por `RtlGetVersion`, elevação).
- **Funcional (3ª rodada da noite — Fase 1 parcial):** Analisador de disco: scanner paralelo em Rust (rayon, pula reparse points, progresso por evento, cancelável), **treemap squarified em canvas** (duplo clique entra na pasta, trilha, tooltip, cores por classe de arquivo) e **Maiores arquivos** (top 50). Medido: C: inteiro sem admin = 1.426.705 arquivos / 174,9 GB em 64 s (126 pastas sem permissão); D:\dev = 46 mil arquivos em ~3 s.
- **Só UI (botões desabilitados "Em breve"):** filtros do treemap, essenciais grandes, mover para Lixeira, prévia/execução de limpeza, TCPing, alvos Gateway/AWS do monitor, aplicar tweaks, limpar RAM, DNS.
- Core Rust com testes reais: `SafePath` (segurança de caminho), `system.rs` (Win32 read-only), `network.rs` (ICMP, traceroute, estatísticas). `bun run check` verde (tsc + Biome + clippy -D warnings + 9 testes).
- Docs: análise estática dos binários (`binary-analysis.md`), verdict do Kaspersky confirmado, arquitetura/design/regras atualizados.
- Verificação visual: exe aberto e fotografado (disco, monitor, MTR, ping de jogos) — `scratchpad/verify-ui.ps1` + env `G2BOOST_START_ROUTE`/`G2BOOST_START_TAB`/`G2BOOST_AUTORUN`.

