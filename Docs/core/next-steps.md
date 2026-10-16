# Roadmap — fases e backlog

**Última revisão:** 2026-10-15 (madrugada)

**Estado:** Fases 0 ✅, 1 (núcleo) ✅, 3 (núcleo) ✅ em 2026-10-14; **Fases 4 (Boost), 2 (Limpeza), 5 (Rotinas) e 6 (Painel/Meu PC/Monitor) ✅ em 2026-10-15** (verificação com admin pendente pelo dono). **6.5 Meu PC completo ✅** (CPU-Z/Speccy user-mode + WinSAT + relatório). **9 IA + MCP ✅** (conselheiro local, assistente no app com 3 provedores, `g2boost-mcp.exe` só leitura). Próximas: 7 disco (filtros/Lixeira/essenciais), 8 rede (TCPing/DNS), 10 release (assinar, empacotar o MCP). Planejamento GSD em `.planning/`.

Ordem pensada pela dor: o C: do dono está com 40 GB livres e os scripts .bat estão bloqueados pelo Kaspersky — disco e limpeza primeiro; rede e boost depois; MCP quando o core estiver estável.

| Fase | Entrega | Critério de pronto |
|---|---|---|
| **0 — Decisões + ambiente** | Stack e nome do produto decididos · `git init` + `.gitignore` · toolchain instalado · scaffold Tauri (React/TS/Tailwind/shadcn) · Biome + clippy + CI lint · `.planning/` via `/gsd-new-project` | app vazio abre com o shell estilo ExitLag (sidebar + topbar + página placeholder) |
| **1 — Analisador de disco** (scan + treemap + maiores ✅ 2026-10-14; falta filtros, Lixeira, snapshot, essenciais) | scan paralelo (walk; MFT depois) · árvore · treemap canvas · filtros (`*.ext`, `\|`, `\pasta`, `>1gb`, `<3months`, atributos) · zoom/drill · tooltip · top-N · "abrir no Explorer" · mover p/ Lixeira com confirmação · snapshot | scan do C: em < 30 s sem admin (hoje 64 s para 1,43 M arquivos — MFT no backlog); testes sandbox + segurança (junction) |
| **2 — Limpeza** ✅ 2026-10-15 (falta: essenciais grandes → fase 6; rotinas → fase 5) | catálogo portado dos .bat com caminhos reais · prévia por categoria · `ConfirmToken` · execução nativa junction-safe · histórico · Lixeira/DNS/`StartComponentCleanup` | substitui os dois .bat; Kaspersky silencioso no teste local (a confirmar elevado) |
| **3 — Rede** | monitor (ICMP gateway/8.8.8.8/1.1.1.1; latência/jitter/perda/picos; gráfico) · speedtest Cloudflare + bufferbloat (nota A+…F) · ping p/ jogos (Steam SDR + lista curada) · MTR/Ping/TCPing · DNS ranking + aplicar/restaurar | paridade com as abas "Analisador de rede" e "DNS Optimizer" do ExitLag |
| **4 — PC Boost** ✅ 2026-10-15 (falta: rotina de RAM → fase 5) | 16 tweaks `detect/apply/revert` + backup exato · efeitos visuais em 4 níveis · serviços (SysMain, telemetria, WSearch, Xbox) · presets Padrão/Alto desempenho/Personalizado · plano de energia · purge da standby list · Elevar via `runas` | paridade com "Ajustes de sistema" e "Limpeza de RAM" (manual) |
| **6 — Painel, Meu PC e Monitor** ✅ 2026-10-15 | KPIs (pontuação por hardware, pontos de restauração, bytes para limpar, RAM) · Boost agora · monitor CPU/GPU/RAM/disco/rede 1 s · Meu PC (SMBIOS/DXGI/IOCTL/TPM/Secure Boot) · recomendações por hardware · tweaks privacidade/aparência · `hvci_off` beta | painel abre sem admin em < 2 s com dados reais (verificado por screenshot) |
| **5 — Rotinas** ✅ 2026-10-15 | iniciar com o Windows (tarefa de logon `--tray`, Highest se elevado) · limpeza agendada (ao ligar / a cada N h / diária / semanal) com categorias · limpar RAM ao abrir jogo (Steam `RunningAppID` + exes + `steamapps\common`) · bandeja · Configurações | app vive na bandeja e limpa sozinho; tudo visível no histórico (verificar logon real + Highest pelo dono) |
| **8 — MCP local** | `g2boost-mcp.exe` (stdio) · tools read-only + destrutivas com token · registro no Claude Desktop/Code · doc de uso | Claude da máquina analisa o scan e propõe limpeza; humano confirma na UI |
| **9 — Release** | assinatura de código · instalador NSIS · auto-update · VirusTotal + submissão de falso positivo (Kaspersky/Microsoft) · README público | exe assinado instala limpo numa VM com Kaspersky |

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

### Para o dono revisar de manhã
1. Renomear a pasta para `G2 Boost` (VS Code fechado).
2. Opcional: aprovar o UAC do `winget install Microsoft.VisualStudio.2022.BuildTools …` (architecture §8) — o build já funciona sem isso.
3. Rodar `bun run tauri:dev` e clicar em tudo; o que estiver "Em breve" é o backlog das fases 1, 2 e 4.
4. Decidir a prioridade: Fase 1 (scan + treemap, resolve o C: com 38 GB) ou fechar a Fase 3 (TCPing, DNS, gateway) / Fase 4 (tweaks, RAM).
