# CLAUDE.md — G2 Boost

> **Spec viva.** Porta-de-entrada lida pelo agente toda sessão. Não duplica os docs profundos — **aponta** pra eles. Atualize quando uma regra, comando, módulo ou hurdle mudar.
>
> **Última revisão:** 2026-10-23 (rodada 8: atualizações apps/Windows, Manutenção+ discos/boot/IP, sempre admin, monitor 1 s, auditoria ponytail) · 2026-10-22 (rodada 7: guia de BIOS + reiniciar na BIOS, núcleos/GPU por jogo, arquivos do sistema + Manutenção DISM/SFC/MRT, Segurança (exposição), GPU (NVAPI DRS, AMD beta), mark G²) · 2026-10-19 (rodada 6 testada ponta a ponta: QoS no Modo Jogo, ETW por processo, hurdles de QoS/DPS/UAC) · 2026-10-18 (fixes R1–R20, UI grafite + motion, Energia + Gerenciador de tarefas, Desinstalador + sobras + Store + desfazer + SmartDelete, Playwright; LICENSE/avisos removidos pelo dono)
> **Fonte única de regras:** [Docs/core/global-rules.md](Docs/core/global-rules.md) · **Roadmap:** [Docs/core/next-steps.md](Docs/core/next-steps.md) · **Estado/sprints:** `.planning/STATE.md` (local, gitignored — criar com `/gsd-new-project`) · **Índice de docs:** [Docs/README.md](Docs/README.md)
> **Pasta:** renomeada para `G2 Boost` em 2026-10-14 (memória do Claude na chave `g--1--Projetos-2-Projetos-Programa--o-G2-Boost`).
> **Projeto-irmão de referência (organização de pastas, Docs, design):** `G:\1. Projetos\6. Grovic Data\Valerium-Portal` (`CLAUDE.md`, `Docs/core/design-system.md`, `Docs/core/global-rules.md`). Mesmas convenções, outro domínio.

---

## 1. O que é

App desktop **Windows 10/11** (x64) que junta, numa interface no estilo ExitLag (escuro, cards, abas, gauges):

1. **Analisador de disco** — treemap estilo SpaceSniffer, filtros, "quem está comendo meu C:", limpeza assistida. **Reimplementação clean-room**: a licença do SpaceSniffer proíbe decompilar/engenharia reversa; a técnica (scan recursivo + squarified treemap) é pública. Ver [spacesniffer.md](Docs/references/spacesniffer.md).
2. **Limpador do Windows** — herda as categorias dos scripts `Limpador Diario/Mensal Windows 2026.bat` (temp, logs, dumps, shaders, caches de apps, Windows Update, DISM, Lixeira) feitas **nativamente** (API Win32, sem spawn de `cmd`/`powershell`), com prévia, histórico e reversão onde couber. Ver [legacy-bat-scripts.md](Docs/references/legacy-bat-scripts.md).
3. **Rede** (a parte do ExitLag que não depende da rede de servidores deles): monitor em tempo real (latência/jitter/perda/picos), speedtest + bufferbloat (Cloudflare), ping para jogos (Steam SDR etc.), ferramentas MTR/Ping/TCPing, DNS Optimizer. Ver [exitlag.md](Docs/references/exitlag.md).
4. **PC Boost** — tweaks de registro/energia com estado atual, aplicar/reverter, presets (Padrão / Alto desempenho / Personalizado), limpeza de RAM com lista de exceções.
5. **MCP local** — `G2 Boost.exe --mcp` (stdio) expõe diagnóstico só leitura ao Claude Desktop / Claude Code da máquina. Ação destrutiva **sempre** confirmada pelo humano na UI.

**Performance para jogos é requisito de 1ª classe** (módulo `boost`: tweaks + plano de energia + RAM; `cleanup`: shaders; `network`: latência/DNS). **Fora de escopo (v1):** rotas de jogo/multipath, Multi Internet, limitador de tráfego via túnel — dependem da infraestrutura de servidores do ExitLag. Limitador por QoS local e overlay de FPS = backlog.

## 2. Stack + Comandos

**DECIDIDO 2026-10-14: Tauri 2.** Toolchain e downloads em **D:/G:**, nunca no C: (`RUSTUP_HOME`/`CARGO_HOME` em `D:\dev`, VS Build Tools em `D:\dev\VS2022BuildTools`; Windows SDK o instalador força em C:). Detalhe em [architecture.md](Docs/core/architecture.md):

- Tauri 2 — core em **Rust** (scan, Win32, rede, limpeza) + UI **React 19 / TypeScript / Vite / Tailwind 4 / shadcn** + MCP em Rust (`rmcp`) compartilhando o crate core + bundle NSIS (assinatura de código fora do roadmap por decisão do dono, 2026-10-18).
- Alternativas avaliadas: Electron + FFI (zero toolchain nova, binário ~200 MB, spawn de shell tentador) · .NET 8 WinUI 3 (nativo, UI custom cara, skills de web não transferem).
- **Precedente interno:** `Grovic-Components/ValeriumDesktop` já é Tauri 2 (wrapper web com tray/deep-link/updater assinado) — referência de config, não base de produto.

Comandos (toolchain: `D:\dev\cargo\bin` no PATH do usuário; nova shell depois da instalação):

```bash
bun install            # deps JS (cache em D:\dev\bun-cache; bunfig: exact + minimumReleaseAge 7d)
bun run tauri:dev      # app de verdade (Vite + Rust, hot reload) — PRIMEIRA vez compila ~5-10 min
bun run dev            # só a UI no browser (comandos Rust voltam null; speedtest funciona)
bun run typecheck      # tsc (também garante simetria pt-BR/en via `satisfies`)
bun run lint           # Biome + cargo clippy -D warnings
bun run test           # bun test tests (filtro de ações da IA) + cargo test --workspace
bun run test:e2e       # Playwright no Edge instalado contra `bun run dev` (e2e/, uma spec por página + reduced-motion)
bun run check          # typecheck + lint + test + test:e2e — DEVE estar verde antes de fechar fase
bun run tauri:build    # NSIS em target/release/bundle/nsis (licença na tela, sem assinatura ainda)
python -I tools/third_party_notices.py   # regenera THIRD_PARTY_NOTICES.md (CARGO_HOME=D:\dev\cargo) — antes de cada release
```

Máquina de dev hoje (2026-10-14): Windows 11 Pro 26200, 32 GB RAM, **C: 40 GB livres** (o app vai nascer resolvendo o próprio problema). Node 22 ✓ · Bun 1.3.11 ✓ · WebView2 ✓ · git/gh ✓ · Python 3.11 ✓ · **faltam** rustup (MSVC) + VS Build Tools 2022 "Desktop development with C++" (~4 GB) · sem .NET SDK · Kaspersky 21.26 instalado (o AV que flagrou os scripts — vira ambiente de teste).

## 3. Mapa de módulos (planejado)

Layout real (Cargo workspace + Vite): `src/` (UI React: `components/shell`, `components/ui/primitives.tsx`, `pages/<módulo>.tsx`, `lib/` (i18n, backend, speedtest), `locales/`, `data/catalogs.ts`) · `src-tauri/` (shell Tauri: comandos IPC finos em `src/lib.rs`) · `crates/core` (lib Rust: `safety.rs` SafePath, `system.rs` Win32 read-only + processos, `disk.rs` (scan, filtro, Lixeira, reveal, essenciais), `network.rs` (ICMP, MTR, TCPing), `dns.rs`, `firmware.rs`, `sensors.rs`, `startup.rs`, `procs.rs` (top por exe + gerenciador por processo), `power.rs` (planos/ajustes), `apps.rs` (desinstalador + sobras), `registry.rs` (HKCU/HKLM + backups JSON), `visualfx.rs`, `boost.rs`, `cleanup.rs`, `tasksched.rs` (COM), `settings.rs`, `routines.rs`, `hardware.rs` (SMBIOS/DXGI/IOCTL/TPM), `monitor.rs` (PDH/NVML), `restore.rs` (SRSetRestorePoint), `recommend.rs`, `wmi.rs` (cliente COM), `winsat.rs`, `nvml.rs`, `advisor.rs`, `context.rs`, `steam.rs`, `secrets.rs` (Cofre do Windows), `luli.rs` (histórico, grupos, memória), `games.rs` + `gamemode.rs` (biblioteca multi-launcher, Modo Jogo), `smart.rs`, `wlan.rs`, `netconns.rs`, `consent.rs`, `userassist.rs`, `dupes.rs`, `boottime.rs`, `prio.rs`, `cpusets.rs` (núcleos por jogo), `volcache.rs` + `repair.rs` (arquivos do sistema, DISM/SFC/MRT), `exposure.rs` (Segurança), `nvapi.rs` + `adlx.rs` + `gpu.rs` (GPU), `updates.rs` (winget em lote + Windows Update), `volumes.rs` (verificar/otimizar unidades), `netrepair.rs` (IP/DNS)) · `crates/mcp` (lib `g2boost_mcp::serve`, servida por `G2 Boost.exe --mcp`; bin de dev) · `tests/` (bun test) · `tools/` (stub, gerador de avisos, `.wsb`) · `Docs/` · `.planning/` (local). Toolchain fora do repo em `D:\dev` (ver [architecture.md §8](Docs/core/architecture.md)).

| Módulo | O que faz | Doc |
|---|---|---|
| `disk` | **Feito:** **porcentagem real + treemap montando ao vivo** (2026-10-18, `ScanProgress::top`); scan paralelo (rayon, pula reparse points; C: = 1,43 M arquivos em 64 s sem admin), treemap squarified em canvas, **filtros** (`*.mp4 >1gb >6m nome`), **mostrar na pasta** (`SHOpenFolderAndSelectItems`), **Lixeira** (`SHFileOperationW` + `FOF_WANTNUKEWARNING`, guarda `recyclable()` sobre `SafePath`), **essenciais grandes** explicados. **Pendente:** MFT quando admin, snapshots. | [disk/analyzer.md](Docs/disk/analyzer.md) |
| `cleanup` | **Feito (2026-10-15):** catálogo com caminhos reais (`crates/core/src/cleanup.rs`), prévia em bytes, `ConfirmToken` de uso único, execução nativa (sem atravessar junction, em uso = pulado), Lixeira/DNS/`StartComponentCleanup` via tarefa do Windows, histórico JSON; UI com confirmação, progresso e histórico. Sandbox test cria junction NTFS real. **Fora:** prefetch, spool (System32). **Pendente:** rotinas (fase 5), verificação com admin pelo dono. | [cleanup/catalog.md](Docs/cleanup/catalog.md) |
| `network` | **Feito:** **Dispositivos na rede** (2026-10-18, `devices.rs`: varredura ICMP + `GetIpNetTable2` + mDNS + SSDP + OUI do IEEE, sem driver/admin, portas de risco só em IPs da última varredura); monitor (ICMP `IcmpSendEcho2`, sem admin; Google/Cloudflare/**gateway**), speedtest Cloudflare + bufferbloat, ping Steam SDR, MTR/Ping/**TCPing**, **DNS Optimizer** (`dns.rs`: 10 provedores, consulta UDP própria, aplicar por adaptador com backup, restaurar). **Pendente:** IPv6, QoS local. | [network/network.md](Docs/network/network.md) |
| `boost` | **Feito (2026-10-15/16):** tweaks como dado (+ `windowed_optimizations`, edita só a própria chave de `DirectXUserGlobalSettings`); aba **Inicialização** (`startup.rs`, semântica `StartupApproved` do Gerenciador de Tarefas, SecurityHealth protegido) + processos mais pesados (`procs.rs`, só leitura). Base: 16 tweaks como dado (`crates/core/src/boost.rs`): registro HKCU/HKLM, serviços via SCM, mouse/teclado via SPI, plano Desempenho máximo, efeitos visuais em 4 níveis (`visualfx.rs`, data-driven pela metadata do diálogo), purge da standby list, relançar elevado (`runas`). Backup JSON exato antes de aplicar; sem backup, desligar grava o padrão documentado e a UI avisa. Presets Padrão / Alto desempenho / Personalizado. **Pendente:** verificar apply/revert admin na máquina do dono; rotina de RAM (fase 5). | [boost/tweaks.md](Docs/boost/tweaks.md) |
| `dashboard` | **Feito (2026-10-15/16):** **BIOS/firmware** (`firmware.rs`: UEFI/CSM, virtualização mesmo com VBS, ReBAR pela BAR1 da NVML, XMP, idade da BIOS, Fast Startup, hibernação) e **temperatura da CPU via HWiNFO/LibreHardwareMonitor** (`sensors.rs`, sem driver nosso). Base: rota inicial Painel com KPIs reais (pontuação = recomendados-para-este-PC ativos, pontos de restauração + criar, bytes para limpar, RAM), monitor 1 s (PDH, `CallNtPowerInformation`, `GetIfTable2`, NVML), ações recomendadas, Meu PC (SMBIOS, DXGI, IOCTLs de storage com SSD/HD, Secure Boot, TPM). Regras de recomendação por hardware (`recommend.rs`). Tweaks novos de privacidade/aparência + `hvci_off` (beta) + categorias. **6.5:** Meu PC no nível user-mode do CPU-Z/Speccy (CPUID, caches, microcode, pentes de RAM + alerta de XMP, BIOS, driver/PCIe/potência da GPU, monitores, WinSAT ler/rodar, relatório JSON/TXT, potência RAPL, clock efetivo). Voltagens/SPD/timings não (exigem driver); temperatura via sensores externos. | [boost/dashboard.md](Docs/boost/dashboard.md) · [speccy-cpuz.md](Docs/references/speccy-cpuz.md) |
| `routines` | **Feito (2026-10-15):** `settings.json` validado (`settings.rs`), agenda pura testável (`routines::due`: ao ligar / a cada N h / diária / semanal), detector de jogo (Steam `RunningAppID`, lista de exes, `steamapps\common`) com borda, purge ao abrir jogo (admin), tarefa de logon `G2 Boost --tray` (`Highest` se elevado, senão LUA), bandeja (Abrir/Sair, fechar = esconder com rotina ativa), página Configurações. **Pendente:** verificar tarefa Highest + purge real elevado; i18n do menu da bandeja. | [boost/routines.md](Docs/boost/routines.md) |
| `mcp` + `ai` | **Feito (2026-10-15/16):** `G2 Boost.exe --mcp` (rmcp 3.5, stdio, sem sidecar) com 13 tools **só leitura** (+ firmware, sensores, inicialização, processos), testado por JSON-RPC. IA com **playbook** no system prompt (segurança inquebrável, temperatura, diagnóstico GPU/CPU-bound, input lag, menos processos) e ações validadas (tweaks não-beta, limpeza, inicialização contra a lista ao vivo, ≤ 6; `tests/ai.test.ts`). Conselheiro local `advisor.rs` (17 achados: XMP, canal único, RAM/VRAM, HD/SATA, disco cheio, Hz abaixo do máximo, GPU quente, driver velho, Secure Boot/TPM, HVCI, WinSAT + classe para jogos). Assistente no app (Claude via SDK oficial, ChatGPT/Gemini via REST) com context pack sem PII, busca web restrita a fontes de benchmark, ações como botões validados; chaves BYOK no Cofre do Windows. **Ações via MCP (2026-10-18):** o Claude Desktop faz tudo do app com confirmação em janela nativa do Windows (`crates/mcp/src/actions.rs`); admin volta `needs_admin`. **Pastas sem dono (2026-10-18):** `apps::unowned` varre AppData/ProgramData/Program Files de todos os discos e bibliotecas Steam (MCP `get_unowned_folders`/`remove_unowned_folders`). **Histórico da Luli (2026-10-18):** conversas salvas com grupos no estilo da barra lateral do Claude, memória (só o que o usuário aprova) e personalização (`luli.rs`, `luli-sidebar.tsx`; arquivo do histórico revalidado ao abrir). | [ai-advisor.md](Docs/core/ai-advisor.md) |
| `maintenance` | **Feito (2026-10-22):** **Limpeza › Arquivos do sistema** pelos handlers da Limpeza de Disco (`volcache.rs`, `IEmptyVolumeCache2` com fallback para a v1, allowlist de 11 handlers, DLL só direto na System32 com nome esperado, carregada pelo próprio `DllGetClassObject`) no fluxo prévia + token; página **Manutenção** (`repair.rs`): DISM scan/restore via `DismApi`, SFC e MRT pela exceção §1.3, um por vez, progresso, cancelar, histórico. **Verificado elevado (2026-10-22):** DISM scan/SFC/MRT reais sem reação do Kaspersky, 11 handlers medem. **Pendente:** RestoreHealth pela UI. | [maintenance.md](Docs/core/maintenance.md) · [system-files.md](Docs/cleanup/system-files.md) |
| `security` | **Feito (2026-10-22):** página **Segurança** (`exposure.rs`): 15 verificações (RDP/NLA, Assistência Remota, WinRM, SMBv1, firewall, portas abertas no roteador via UPnP, apps de acesso remoto, escutas públicas, conexões de entrada, sessões, persistência sem assinatura, convidado, antivírus), correções reversíveis com backup, monitor a cada 5 min no laço de rotinas com selo na barra lateral, achados no conselheiro, MCP `get_exposure_report`. Não é antivírus (dito na tela). | [exposure-audit.md](Docs/security/exposure-audit.md) |
| `gpu` | **Feito (2026-10-22):** página **GPU** por vendor (DXGI): NVIDIA via NVAPI DRS (`nvapi.rs`, `nvapi64.dll` só da System32; baixa latência incl. Ultra, energia, limite de FPS, VSync, cache de shader, filtragem, otimização threaded; global e por jogo; backup + desfazer; sem admin); AMD via ADLX **beta** isolado no processo `--gpu-helper` (`adlx.rs`); Intel "em breve". MCP `get_gpu_settings`. **Pendente:** AMD em hardware real. | [gpu.md](Docs/boost/gpu.md) |
| `updates` | **Feito (2026-10-23):** Apps › **Atualizações**: winget em lote (um `install(id, true)` validado por pacote, token da lista, excluídos, parar entre pacotes, histórico) + **Windows Update** pela WUA COM (busca sem admin, instalar com admin, histórico, reiniciar só se pendente). MCP `get_updates`. Manutenção ganhou verificar disco/otimizar unidades (`volumes.rs`, `MSFT_Volume`), reiniciar nas Opções avançadas e Rede › reparar conexão (`netrepair.rs`). | [updates.md](Docs/core/updates.md) · [maintenance.md](Docs/core/maintenance.md) |

## 4. Regras SEMPRE-ATIVAS

Resumo. **Detalhe + checklist em [Docs/core/global-rules.md](Docs/core/global-rules.md)** (fonte da verdade).

- **Regra zero — todo caller é hostil:** UI, MCP, arquivo de config, nome de arquivo/pasta, snapshot importado. Validação no core (Rust), não na UI. (§1.1)
- **Destrutivo = prévia + confirmação + allowlist + log.** Nunca `rd /s` cego. Nunca apagar fora do catálogo sem seleção explícita do usuário no treemap. Lixeira antes de delete permanente quando possível. (§1.2)
- **Nativo, não shell.** Proibido spawnar `cmd`, `powershell`, `reg`, `net`, `sc`, `wevtutil`. É exatamente o padrão que o Kaspersky flagra (§6). DISM via `DismApi` (não `DISM.exe`); exceções documentadas: `winget.exe`, `sfc.exe`, `mrt.exe` (exe direto, argumentos fixos, só por clique). (§1.3)
- **Menor privilégio.** Scan e rede rodam sem admin; limpeza de sistema/tweaks pedem elevação via manifesto/UAC, nunca auto-relançamento por PowerShell. (§1.4)
- **TDD** — unit (catálogo, filtros, parser, layout do treemap) + integração em **sandbox de filesystem** (árvore fabricada em pasta temp) — nunca testar limpeza no C: real. Segurança dedicada por feature destrutiva (path traversal, junction/reparse, symlink, MCP sem token). (§2)
- **Docs vivas** — toda feature em `Docs/<área>/<nome>.md` com `**Última revisão:** YYYY-MM-DD`. Ler antes de codar; PR que muda comportamento atualiza doc no MESMO PR. (§3)
- **i18n em tudo** — pt-BR (padrão) + en, chaves simétricas, zero string solta na UI. (§4)
- **Design consistente** — tokens HSL em CSS vars como no Valerium ([design-system.md](Docs/core/design-system.md)), zero hex literal em componente; reusar shadcn / `Grovic-Components` antes de criar; gráficos via `<ChartContainer>`. Skill `frontend-design` em toda tela.
- **GSD** — discuss → plan → execute → verify. `.planning/` é local (gitignored). (§5)
- **Caveman full sempre** nas respostas. Código/commits/PR/docs/avisos de segurança = escrita normal.
- **Autor do software:** Gustavo Martins Gripaldi (pessoa física; `bundle.publisher`/copyright/Cargo). Nunca citar outros apps, autores ou fontes por nome nos textos da interface (só na base de conhecimento da Luli e em `Docs/references`).
- **Commits sem co-author** — nunca `Co-Authored-By`/trailer de assistente. Sobrepõe o padrão do harness.

## 5. Helpers de segurança canônicos (no core — uso obrigatório)

| Helper | Função |
|---|---|
| `SafePath::classify(path)` | Classifica raiz (sistema / perfil atual / outro perfil / removível / rede); bloqueia `C:\Windows\System32`, perfis de outros usuários, reparse points e junctions (equivalente ao `/XJ` do robocopy). |
| `cleanup::preview()` → `TokenStore::issue/consume` → `execute()` | Mede antes; executa só com token de uso único (2 min, amarrado aos ids) emitido pela UI após confirmação. MCP nunca gera token sozinho. |
| `boost::apply(id, on)` + `registry::Backup` | Grava os valores originais (JSON) antes de aplicar; desligar restaura exatamente o backup. Sem backup → padrão documentado, avisado na UI. |
| `system::is_elevated()` + `boost::relaunch_elevated()` | Caminhos admin retornam `needs_admin` sem tentar; "Elevar" relança via `ShellExecuteExW("runas")` com `--route`, nunca PowerShell. |
| `redact(log)` | Sem caminhos de perfil/PII em logs persistidos. Sem telemetria por padrão. |

## 6. Common Hurdles (armadilhas conhecidas)

- **Kaspersky × scripts .bat = `PDM:Trojan.Win32.Generic` (System Watcher, "Análise de comportamento", 2026-10-13 23:45).** Matou o `cmd.exe` do mensal no meio e apagou o .bat do Desktop (a cópia em `Referencias/` sobreviveu). Gatilhos: auto-elevação `powershell Start-Process -Verb RunAs` sobre si mesmo, `-ExecutionPolicy Bypass`, one-liners de 1.900–2.600 chars, `del /s /f /q` em Prefetch/logs/minidumps/WER (assinatura anti-forense), `net stop wuauserv/bits/dosvc`, `reg add` em `HKLM\...\Policies`. Análise, verdicts prováveis e mitigação em [kaspersky-false-positive.md](Docs/security/kaspersky-false-positive.md). **O app herda o risco se repetir os padrões** → nativo + assinatura de código + submissão de falso positivo.
- **Bug de precedência nos scripts legados.** `@('nome',$L+'\x|'+$L+'\y')` — em PowerShell a vírgula vence o `+`, então `$e[1]` vira só `$L`: "Cache de shaders", "Thumbnails", "Chrome", "Edge", "Spotify" medem a `AppData\Local` inteira (64 GB) e "Discord" mede `Roaming`; "Logs" perde os dumps. Relatório inflado/duplicado + medição lenta. Confirmado por teste em 2026-10-14. Portar a **intenção**, não os números.
- **Prefetch: não limpar** (lentidão + indicador anti-forense). **Working set trim: não** (o próprio script v2.0 removeu por piorar). `DISM /ResetBase` impede desinstalar updates — opt-in com aviso.
- **SpaceSniffer e ExitLag: análise estática feita** (imports PE, strings, drivers, serviços) em [binary-analysis.md](Docs/references/binary-analysis.md) — mecanismo no nível de API. Layout do treemap via `d3-hierarchy` `treemapSquarify`; render próprio em canvas.
- **REA neste host (Windows):** Ghidra 12.1.4 em `D:\dev` só aceita **PE**; Mach-O/plist/assinatura nativos exigem macOS/Hopper. Para `.app`: `open_binary` no bundle + `decode_interface_builder` (nibs) funcionam; o resto por parser próprio de seções (`scratchpad`). Ghidra no `ExitLag.exe` (114 MB) é inviável — busca de bytes resolve.
- **`IcmpSendEcho2Ex` (iphlpapi)** não precisa de admin nem raw socket; TTL incremental = traceroute/MTR sem driver.
- **Cloudflare speedtest:** `https://speed.cloudflare.com/__down?bytes=N` e `__up`, CORS `*`; ISP/IP/cidade vêm nos headers `cf-meta-*` (o `/meta` via curl voltou `{}`). Ookla CLI é proprietário — não usar.
- **Steam SDR:** `https://api.steampowered.com/ISteamApps/GetSDRConfig/v1/?appid=730` → 53 POPs com IPs de relay (`gru` = São Paulo). Serve CS2/Dota 2/Deadlock sem chave.
- **WMIC removido no 24H2** (o script v2.0 já quebrou por isso) → WMI/CIM via API.
- **Comando Bash > ~8 KB falha no Windows** (`unexpected EOF while looking for matching`) — heredoc grande é truncado. **E heredoc colapsa `\\` em `\`** (Python com `D:\\dev` vira `D:\dev` → `unicodeescape`). Arquivo grande ou com barra invertida = tool `Write` (script no scratchpad) e rodar com `python -I`.
- **`python -I` ignora user-site** → libs no scratchpad via venv.
- **GSD global 1.16.0** (atualizado 2026-10-14); instalador avisa `node >= 24` (temos 22) mas funcionou. Hooks novos valem na próxima sessão.
- **VS Build Tools NÃO instalado (UAC cancelado, exit 1602).** O projeto compila mesmo assim pelo plano B sem admin: `D:\dev\cargo\config.toml` aponta linker `rust-lld` + libs CRT/SDK do `xwin` (`D:\dev\xwin`) + `rc.exe` oficial do NuGet (`D:\dev\winsdk-tools`). `xwin splat` falhou só no alias de headers (symlink sem privilégio) — libs completas (481 `.lib`). `vswhom-sys` (C++ dentro do `embed-resource`) é substituído por `tools/vswhom-sys-stub` via `[patch.crates-io]` — não remover sem Build Tools. Crates com código C (ex.: `rusqlite bundled`) precisariam de `clang-cl` (LLVM portátil, 900 MB) — evitar; persistência em JSON por enquanto. Para o caminho oficial, rodar o `winget` do architecture §8 aprovando o UAC.
- **`tauri-plugin-http` só com `default-features = false` + `native-tls`** (schannel). O default `rustls-tls` puxa `ring`, que compila C e quebra o build sem `cl.exe`. Mesmo cuidado para qualquer dependência nova: rodar `cargo tree -i cc` antes de aceitar.
- **Kaspersky intercepta TLS** (`pip install` falhou com erro de certificado no subagente). Downloads via `curl`/`Invoke-WebRequest` passaram; se `pip`/`cargo` reclamarem de certificado, é o AV, não a rede.
- **`tsc` é o gate de i18n**: `en.ts` usa `satisfies Record<keyof typeof ptBR, string>` — chave faltando ou sobrando quebra o typecheck. Não precisa de script separado.
- **Comandos Tauri fora do Tauri** (`bun run dev` no browser) voltam `null` via `backend.ts` (`inTauri` guard) — a UI mostra "—", não quebra.
- **Renomeou a pasta do repo? `cargo clean -p g2-boost`.** O build script do Tauri grava caminhos absolutos em `target/debug/build/tauri-*`; com o caminho velho o clippy/build morre com `failed to read plugin permissions ... Otimização PC\...`. Custou 6,5 GB de artefatos, recompila em ~3 min.
- **`windows` crate:** `VARIANT` só existe com a feature `Win32_System_Ole` (além de `Win32_System_Com`/`Variant`); `BOOL` mora em `windows::core`; `LocalFree` em `Win32::Foundation`; `NtSetSystemInformation` não existe no crate (declarar `unsafe extern "system"` em `ntdll`); clippy 1.99 exige `as_chunks::<2>()` no lugar de `chunks_exact(2)`.
- **Tarefa agendada de logon sem `SetUserId` = "qualquer usuário" → `E_ACCESSDENIED` sem admin.** Com `UserId = DOMÍNIO\usuário` o usuário comum registra a própria tarefa (round-trip testado).
- **`windows` 0.62, mais pegadinhas:** `IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS` mora em `Storage::FileSystem` (não em `System::Ioctl`); `GetActiveProcessorCount`/`ALL_PROCESSOR_GROUPS` em `System::Threading`; `STATEMGRSTATUS.nStatus` é `WIN32_ERROR` (usar `.0`); `VARIANT` BSTR via `Anonymous.Anonymous.Anonymous.bstrVal`; `AtomicU32::fetch_update` está deprecado (clippy pede `try_update`; loop `compare_exchange` funciona). DXGI pode listar a mesma GPU 2× → dedupe. Tarefa do Compatibility Appraiser chama-se "… Appraiser Exp" no 24H2+.
- **CPU-Z e Speccy usam driver de kernel (`cpuz.sys`, WHCP)** para temperatura/voltagem/SPD — instalado sob demanda e apagado ao sair. ver [speccy-cpuz.md](Docs/references/speccy-cpuz.md). Clock efetivo vem de PDH `% Processor Performance` × `Processor Frequency` (o `CallNtPowerInformation` devolve o clock base); potência do pacote de PDH `Energy Meter(RAPL_Package0_PKG)`.
- **SMBIOS: registro sem strings termina em `\0\0`** — pular os dois NULs, senão o parser se perde e some o tipo 17 (bug pego pelo teste com blob sintético).
- **IA nunca executa:** respostas trazem ids num bloco `g2boost-actions`; a UI valida contra o catálogo e vira botão (fluxo normal com backup). Chave de API só no Cofre do Windows; o webview recebe por pergunta; HTTP só pelos hosts da allowlist das capabilities. Markdown do modelo renderizado como texto (`SafeMarkdown`), nunca HTML.
- **rmcp 3.5:** `#[tool_router]` no impl + `#[tool_handler(router = self.tool_router)] impl ServerHandler` + `serve(rmcp::transport::stdio())`; tool que devolve `Result<String, String>` vira `isError` no erro. Serializar `Result` direto gera `{"Ok": …}` — desembrulhe antes.
- **`npx biome` baixa o pacote errado** (`biome@0.3.3`, um gerenciador de env de 2016 que cria `~/.biome`). Sempre `bun run lint` ou `./node_modules/.bin/biome`. Aconteceu em 2026-10-16 (pasta vazia e cache npx removidos).
- **Hyper-V/VBS esconde a virtualização:** com HVCI/VBS ligado o Windows é convidado — `PF_VIRT_FIRMWARE_ENABLED` e o bit VMX do CPUID leem **falso**. Usar o bit de hypervisor (CPUID.1:ECX[31]) como prova de VT-x ligado (`firmware.rs`, Meu PC).
- **Tauri baixa NSIS em `%LOCALAPPDATA%\tauri` (C:).** Junção criada para `D:\dev\tauri` (regra de downloads fora do C:). Em máquina nova, criar a junção antes do primeiro `tauri:build`.
- **Sidecar (`externalBin`) quebra `cargo test`/clippy** se o binário não existir (o build script do Tauri valida). Por isso o MCP é servido pelo próprio exe com `--mcp`.
- **Testes que gravam no data dir** (histórico, backups) precisam de `G2BOOST_DATA_DIR` (`registry::tests::test_data_dir()`), senão escrevem no `%LOCALAPPDATA%\G2 Boost` real.
- **`cargo test` com vários filtros:** `cargo test -p g2boost-core -- dns network` (filtros depois do `--`).
- **`DirectXUserGlobalSettings` é compartilhada** (Auto HDR, VRR, flip model): editar só a chave própria (`boost::dx_set`), nunca sobrescrever a string.
- **WinGet COM não funciona em exe sem pacote:** `0x80073D54`/proxy de `IPackageManager` ausente, mesmo com a DLL cliente do NuGet ComInterop; `LoadLibrary` de `WindowsApps` = acesso negado. Detalhe e saídas em [console.md](Docs/core/console.md).
- **Worktree de subagente pode nascer de commit antigo:** o `isolation: worktree` às vezes parte de um commit velho; nunca aplique `git diff <commit-novo> <branch>` (desfaz o que é mais novo). Use `git diff $(git merge-base <branch> main) <branch>` e confira se o main mexeu nos mesmos arquivos (2026-10-18: um patch do `procs.rs` apagou `raise_priority`; restaurado).
- **Temperatura da CPU:** decisão do dono = sem driver nosso e sem PawnIO; só HWiNFO (recomendado) ou LHM se já rodam. Core Temp e Afterburner têm drivers flagrados como vulneráveis: não recomendar.
- **e2e na partida a frio do Vite:** o primeiro lote de páginas em paralelo pode estourar o `page.goto` (30 s) enquanto o Vite otimiza dependências; rodar `bun run test:e2e` de novo antes de investigar (2026-10-18: 6 falhas → 13/13 na segunda).
- **Playwright fora do runner:** `bun` trava ao abrir o Edge (e tenta auto-instalar pacote se o script estiver fora do projeto). Script avulso = `.mjs` com `node`, dentro do repo (pasta ignorada `test-results/`).
- **QoS (`MSFT_NetQosPolicySettingData`) grava o nome em minúsculas:** comparar prefixo ignorando caixa (`qos::PREFIX = "g2boost-"`), senão o app não acha a própria política e ela vaza. Política no `ActiveStore` some ao reiniciar; o app elevado limpa `g2boost-*` ao abrir. Conferir sobras: `Get-NetQosPolicy -PolicyStore ActiveStore`.
- **Tempo de boot vazio com admin ≠ bug:** com o Serviço de Política de Diagnóstico (`svc_dps`) desligado o Windows para de gravar o evento 100 em `Diagnostics-Performance/Operational` (aqui: último em 10/05/2025). `boottime::report` devolve `last_known`; o shell manda `dps_off`.
- **Testes elevados:** `Start-Process pwsh -Verb RunAs -ArgumentList '-NoProfile','-File',<script>` com o binário de teste já compilado (`target\debug\deps\g2boost_core-*.exe --ignored`); saída em arquivo (a janela elevada não volta stdout). O primeiro UAC de cada rodada costuma ser cancelado: perguntar e abrir de novo. Scripts em `D:\dev\g2boost-tools\elevated-*.ps1`.
- **ETW de rede por processo** (`netetw.rs`, sessão `G2Boost-NetUsage`, Kernel-Network keywords 0x30) só abre elevado; uma sessão com o mesmo nome que sobrou de um travamento é parada e reaberta. Fechar ao sair da aba (`net_usage_stop`).
- **Gemini 2.5 fechado para chave nova de API** (2026-10): padrão do app é `gemini-3.8-flash`. Modelos da NVIDIA: `curl https://integrate.api.nvidia.com/v1/models` (público).
- **MCP Apps:** UI no Claude Desktop via `_meta.ui.resourceUri` + recurso `text/html;profile=mcp-app` (`crates/mcp/src/panel.html`). `#[tool(meta = ...)]` do rmcp 3.5 recebe `MetaObject` (de `serde_json::Map`).
- **Ícone novo não aparece no exe de debug:** o `generate_context!` embute os ícones na compilação e não recompila quando eles mudam. Trocou `src-tauri/icons/*` → `cargo clean -p g2-boost` antes do build (2026-10-22, a barra de título ainda mostrava o mark antigo).
- **Scripts elevados de teste: sem `-ExecutionPolicy Bypass`** (é padrão que o Kaspersky pontua, §6 acima); `Start-Process pwsh -Verb RunAs -ArgumentList '-NoProfile','-File',<script>` basta.
- **NVAPI DRS grava sem admin** (pasta do banco do driver dá controle total a Todos); `SaveSettings` funcionou como usuário comum (2026-10-22).
- **Handlers da Limpeza de Disco (`VolumeCaches`): quase todos só têm `IEmptyVolumeCache` v1.** Pedir `IEmptyVolumeCache2` direto dá `E_NOINTERFACE` (0x80004002) em 10 de 11 no 24H2; criar como `IUnknown`, tentar v2 (`InitializeEx`) e cair na v1 (`Initialize`), como o `cleanmgr` (2026-10-22).
- **Nunca rodar `cargo fmt`:** o código não segue o rustfmt padrão (linhas de até ~120 colunas, sem `rustfmt.toml`); um `cargo fmt -p g2boost-core` reformatou 55 arquivos (+11 mil linhas) em 2026-10-23. O gate é `bun run lint` (Biome + clippy).
- **Kaspersky acusa os binários de TESTE (`target\debug\deps\g2boost_core-*.exe`) como `PDM:Trojan.Win32.Generic`** (2026-10-23): exe sem assinatura em pasta de build fazendo junção, registro, prioridade, tarefas, encerrar processos. Na máquina de dev: desinfectar sem reiniciar + excluir `target\` da verificação. O app de release ainda precisa ser testado com o Kaspersky ativo ([kaspersky-false-positive.md](Docs/security/kaspersky-false-positive.md)).
- **Efeitos visuais são data-driven:** `HKLM\...\Explorer\VisualEffects\<item>` traz `SPIActionGet/Set` ou `RegPath`. Tabela congelada em `visualfx.rs`; o nível Média bateu com o setup do dono na primeira detecção.

## 7. Antes de fechar qualquer fase

- [ ] Doc da feature atualizada (data) · catálogo de limpeza revisado por risco
- [ ] Testes: unit + integração em FS sandbox + segurança (traversal, junction, symlink, MCP sem token)
- [ ] Nenhum spawn de shell novo · nenhuma deleção fora do catálogo/seleção
- [ ] `bun run check` verde (typecheck + Biome + clippy + cargo test)
- [ ] i18n pt-BR + en simétrico · tokens de design (zero hex)
- [ ] `.planning/STATE.md` atualizado

## 8. Índice de navegação

- **Roadmap / fases** → [Docs/core/next-steps.md](Docs/core/next-steps.md)
- **Release (assinatura, SmartScreen, instalar em outro PC)** → [Docs/release/distribution-and-trust.md](Docs/release/distribution-and-trust.md) · **Driver/sensores** → [Docs/security/kernel-driver-options.md](Docs/security/kernel-driver-options.md)
- **Disco** → [Docs/disk/analyzer.md](Docs/disk/analyzer.md) · **Rede/DNS** → [Docs/network/network.md](Docs/network/network.md) · **Jogos, Modo Jogo, rota** → [Docs/boost/games.md](Docs/boost/games.md) ([viabilidade](Docs/network/game-routes.md))
- **Atualizações (winget em lote, Windows Update)** → [Docs/core/updates.md](Docs/core/updates.md) · **Manutenção (DISM/SFC/MRT, discos, boot) e arquivos do sistema** → [Docs/core/maintenance.md](Docs/core/maintenance.md) · [Docs/cleanup/system-files.md](Docs/cleanup/system-files.md) · **Segurança (exposição)** → [Docs/security/exposure-audit.md](Docs/security/exposure-audit.md) · **GPU (driver 3D)** → [Docs/boost/gpu.md](Docs/boost/gpu.md)
- **Privacidade, saúde dos discos, rede por app, Wi-Fi, boot, prioridade fixa, apps pouco usados, duplicados** → [Docs/boost/privacy-health.md](Docs/boost/privacy-health.md) · **IA/MCP** → [Docs/core/ai-advisor.md](Docs/core/ai-advisor.md)

- **Base de conhecimento da Luli** (vai no prompt de toda conversa) → [knowledge/README.md](knowledge/README.md)
- **Console G2** (comandos nativos, Luli sugere; winget via `winget.exe` com confirmação) → [Docs/core/console.md](Docs/core/console.md)
- **Arquitetura + decisão de stack** → [Docs/core/architecture.md](Docs/core/architecture.md)
- **Regras de engenharia (completo)** → [Docs/core/global-rules.md](Docs/core/global-rules.md)
- **Design system** → [Docs/core/design-system.md](Docs/core/design-system.md)
- **Boost (tweaks, efeitos visuais, serviços, RAM)** → [Docs/boost/tweaks.md](Docs/boost/tweaks.md)
- **Energia (planos + ajustes AC/DC) e Gerenciador de tarefas** → [Docs/boost/power-processes.md](Docs/boost/power-processes.md)
- **Limpeza (catálogo, prévia, token, histórico)** → [Docs/cleanup/catalog.md](Docs/cleanup/catalog.md)
- **Desinstalar apps (lista, desinstalador validado, sobras com confiança, Lixeira + backup de registro, Store, arrastar e soltar)** → [Docs/cleanup/app-uninstall.md](Docs/cleanup/app-uninstall.md)
- **Rotinas (autostart, agenda, RAM ao abrir jogo, bandeja)** → [Docs/boost/routines.md](Docs/boost/routines.md)
- **Painel, Meu PC, monitor, recomendação por hardware** → [Docs/boost/dashboard.md](Docs/boost/dashboard.md)
- **IA no app, MCP, conselheiro local, estimativa de FPS** → [Docs/core/ai-advisor.md](Docs/core/ai-advisor.md) · **CPU-Z/Speccy (análise)** → [Docs/references/speccy-cpuz.md](Docs/references/speccy-cpuz.md)
- **Pesquisa de serviços (evidência dos presets)** → [Docs/references/windows-services-research.md](Docs/references/windows-services-research.md) · **Galdino Optimizer (extração)** → [Docs/references/galdino-optimizer.md](Docs/references/galdino-optimizer.md)
- **Referências** → [spacesniffer.md](Docs/references/spacesniffer.md) · [exitlag.md](Docs/references/exitlag.md) · [legacy-bat-scripts.md](Docs/references/legacy-bat-scripts.md) · [appcleaner.md](Docs/references/appcleaner.md) (base da 3C: desinstalar + resíduo) · [kaspersky.md](Docs/references/kaspersky.md) (recursos a trazer)
- **Segurança** → [kaspersky-false-positive.md](Docs/security/kaspersky-false-positive.md)
- **Índice geral de docs** → [Docs/README.md](Docs/README.md)
- **Memória cross-sessão** (auto-injetada) → `~/.claude/projects/.../memory/MEMORY.md`
