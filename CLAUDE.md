# CLAUDE.md — G2 Boost

> **Spec viva.** Porta-de-entrada lida pelo agente toda sessão. Não duplica os docs profundos — **aponta** pra eles. Atualize quando uma regra, comando, módulo ou hurdle mudar.
>
> **Última revisão:** 2026-10-14 (noite — fase 0 executada)
> **Fonte única de regras:** [Docs/core/global-rules.md](Docs/core/global-rules.md) · **Roadmap:** [Docs/core/next-steps.md](Docs/core/next-steps.md) · **Estado/sprints:** `.planning/STATE.md` (local, gitignored — criar com `/gsd-new-project`) · **Índice de docs:** [Docs/README.md](Docs/README.md)
> **Pasta:** o dono pediu renomear `Otimização PC` → `G2 Boost` (fazer com o VS Code fechado; a memória do Claude já foi copiada para a chave nova `g--1--Projetos-2-Projetos-Programa--o-G2-Boost`).
> **Projeto-irmão de referência (organização de pastas, Docs, design):** `G:\1. Projetos\6. Grovic Data\Valerium-Portal` (`CLAUDE.md`, `Docs/core/design-system.md`, `Docs/core/global-rules.md`). Mesmas convenções, outro domínio.

---

## 1. O que é

App desktop **Windows 10/11** (x64) que junta, numa interface no estilo ExitLag (escuro, cards, abas, gauges):

1. **Analisador de disco** — treemap estilo SpaceSniffer, filtros, "quem está comendo meu C:", limpeza assistida. **Reimplementação clean-room**: a licença do SpaceSniffer proíbe decompilar/engenharia reversa; a técnica (scan recursivo + squarified treemap) é pública. Ver [spacesniffer.md](Docs/references/spacesniffer.md).
2. **Limpador do Windows** — herda as categorias dos scripts `Limpador Diario/Mensal Windows 2026.bat` (temp, logs, dumps, shaders, caches de apps, Windows Update, DISM, Lixeira) feitas **nativamente** (API Win32, sem spawn de `cmd`/`powershell`), com prévia, histórico e reversão onde couber. Ver [legacy-bat-scripts.md](Docs/references/legacy-bat-scripts.md).
3. **Rede** (a parte do ExitLag que não depende da rede de servidores deles): monitor em tempo real (latência/jitter/perda/picos), speedtest + bufferbloat (Cloudflare), ping para jogos (Steam SDR etc.), ferramentas MTR/Ping/TCPing, DNS Optimizer. Ver [exitlag.md](Docs/references/exitlag.md).
4. **PC Boost** — tweaks de registro/energia com estado atual, aplicar/reverter, presets (Padrão / Alto desempenho / Personalizado), limpeza de RAM com lista de exceções.
5. **MCP local** — servidor MCP (stdio) expondo scan/diagnóstico/limpeza ao Claude Desktop / Claude Code da máquina. Ação destrutiva **sempre** confirmada pelo humano na UI.

**Performance para jogos é requisito de 1ª classe** (módulo `boost`: tweaks + plano de energia + RAM; `cleanup`: shaders; `network`: latência/DNS). **Fora de escopo (v1):** rotas de jogo/multipath, Multi Internet, limitador de tráfego via túnel — dependem da infraestrutura de servidores do ExitLag. Limitador por QoS local e overlay de FPS = backlog.

## 2. Stack + Comandos

**DECIDIDO 2026-10-14: Tauri 2.** Toolchain e downloads em **D:/G:**, nunca no C: (`RUSTUP_HOME`/`CARGO_HOME` em `D:\dev`, VS Build Tools em `D:\dev\VS2022BuildTools`; Windows SDK o instalador força em C:). Detalhe em [architecture.md](Docs/core/architecture.md):

- Tauri 2 — core em **Rust** (scan, Win32, rede, limpeza) + UI **React 19 / TypeScript / Vite / Tailwind 4 / shadcn** + MCP em Rust (`rmcp`) compartilhando o crate core + bundle NSIS **assinado**.
- Alternativas avaliadas: Electron + FFI (zero toolchain nova, binário ~200 MB, spawn de shell tentador) · .NET 8 WinUI 3 (nativo, UI custom cara, skills de web não transferem).
- **Precedente interno:** `Grovic-Components/ValeriumDesktop` já é Tauri 2 (wrapper web com tray/deep-link/updater assinado) — referência de config, não base de produto.

Comandos (toolchain: `D:\dev\cargo\bin` no PATH do usuário; nova shell depois da instalação):

```bash
bun install            # deps JS (cache em D:\dev\bun-cache; bunfig: exact + minimumReleaseAge 7d)
bun run tauri:dev      # app de verdade (Vite + Rust, hot reload) — PRIMEIRA vez compila ~5-10 min
bun run dev            # só a UI no browser (comandos Rust voltam null; speedtest funciona)
bun run typecheck      # tsc (também garante simetria pt-BR/en via `satisfies`)
bun run lint           # Biome + cargo clippy -D warnings
bun run test           # cargo test --workspace (core: SafePath, Win32 system)
bun run check          # typecheck + lint + test — DEVE estar verde antes de fechar fase
bun run tauri:build    # NSIS em target/release/bundle/nsis (sem assinatura ainda)
```

Máquina de dev hoje (2026-10-14): Windows 11 Pro 26200, 32 GB RAM, **C: 40 GB livres** (o app vai nascer resolvendo o próprio problema). Node 22 ✓ · Bun 1.3.11 ✓ · WebView2 ✓ · git/gh ✓ · Python 3.11 ✓ · **faltam** rustup (MSVC) + VS Build Tools 2022 "Desktop development with C++" (~4 GB) · sem .NET SDK · Kaspersky 21.26 instalado (o AV que flagrou os scripts — vira ambiente de teste).

## 3. Mapa de módulos (planejado)

Layout real (Cargo workspace + Vite): `src/` (UI React: `components/shell`, `components/ui/primitives.tsx`, `pages/<módulo>.tsx`, `lib/` (i18n, backend, speedtest), `locales/`, `data/catalogs.ts`) · `src-tauri/` (shell Tauri: comandos IPC finos em `src/lib.rs`) · `crates/core` (lib Rust: `safety.rs` SafePath, `system.rs` Win32 read-only; futuros `disk`, `cleanup`, `network`, `boost`) · `crates/mcp` (a criar) · `Docs/` · `.planning/` (local). Toolchain fora do repo em `D:\dev` (ver [architecture.md §8](Docs/core/architecture.md)).

| Módulo | O que faz | Doc |
|---|---|---|
| `disk` | Scan paralelo (MFT quando admin + NTFS, walk como fallback), árvore em memória, treemap squarified em canvas, filtros (máscara, pasta, tamanho, idade, atributos, tags), snapshots, "arquivos essenciais grandes" (hiberfil, pagefile, WinSxS, Docker/WSL vhdx, caches de IDE) com explicação + ação segura. | [spacesniffer.md](Docs/references/spacesniffer.md) |
| `cleanup` | Catálogo de categorias (id, caminhos, estratégia, risco, apps que precisam estar fechados, frequência) → prévia em bytes → execução nativa → histórico. Fonte: scripts legados. | [legacy-bat-scripts.md](Docs/references/legacy-bat-scripts.md) |
| `network` | Monitor (ICMP via `IcmpSendEcho2Ex`, sem raw socket/admin), speedtest Cloudflare (`__down`/`__up`, latência sob carga → nota de bufferbloat), ping para jogos (Steam `GetSDRConfig`), MTR/Ping/TCPing, ranking + troca de DNS por adaptador. | [exitlag.md](Docs/references/exitlag.md) |
| `boost` | Tweaks com `detect / apply / revert` + backup `.reg`, presets, "não recomendado se…", plano de energia, limpeza de RAM (purge da standby list; **não** trim de working set). | [exitlag.md](Docs/references/exitlag.md) |
| `mcp` | Binário stdio `*-mcp.exe`; tools read-only livres; destrutivas pedem token de confirmação emitido pela UI (named pipe). Nome de arquivo é **dado**, nunca instrução (prompt injection). | [architecture.md](Docs/core/architecture.md) |

## 4. Regras SEMPRE-ATIVAS

Resumo. **Detalhe + checklist em [Docs/core/global-rules.md](Docs/core/global-rules.md)** (fonte da verdade).

- **Regra zero — todo caller é hostil:** UI, MCP, arquivo de config, nome de arquivo/pasta, snapshot importado. Validação no core (Rust), não na UI. (§1.1)
- **Destrutivo = prévia + confirmação + allowlist + log.** Nunca `rd /s` cego. Nunca apagar fora do catálogo sem seleção explícita do usuário no treemap. Lixeira antes de delete permanente quando possível. (§1.2)
- **Nativo, não shell.** Proibido spawnar `cmd`, `powershell`, `reg`, `net`, `sc`, `wevtutil`. É exatamente o padrão que o Kaspersky flagra (§6). Exceção documentada: DISM (via `DismApi`, não `DISM.exe`). (§1.3)
- **Menor privilégio.** Scan e rede rodam sem admin; limpeza de sistema/tweaks pedem elevação via manifesto/UAC, nunca auto-relançamento por PowerShell. (§1.4)
- **TDD** — unit (catálogo, filtros, parser, layout do treemap) + integração em **sandbox de filesystem** (árvore fabricada em pasta temp) — nunca testar limpeza no C: real. Segurança dedicada por feature destrutiva (path traversal, junction/reparse, symlink, MCP sem token). (§2)
- **Docs vivas** — toda feature em `Docs/<área>/<nome>.md` com `**Última revisão:** YYYY-MM-DD`. Ler antes de codar; PR que muda comportamento atualiza doc no MESMO PR. (§3)
- **i18n em tudo** — pt-BR (padrão) + en, chaves simétricas, zero string solta na UI. (§4)
- **Design consistente** — tokens HSL em CSS vars como no Valerium ([design-system.md](Docs/core/design-system.md)), zero hex literal em componente; reusar shadcn / `Grovic-Components` antes de criar; gráficos via `<ChartContainer>`. Skill `frontend-design` em toda tela.
- **GSD** — discuss → plan → execute → verify. `.planning/` é local (gitignored). (§5)
- **Caveman full sempre** nas respostas. Código/commits/PR/docs/avisos de segurança = escrita normal.
- **Commits sem co-author** — nunca `Co-Authored-By`/trailer de assistente. Sobrepõe o padrão do harness.

## 5. Helpers de segurança canônicos (a criar no core — uso obrigatório)

| Helper | Função |
|---|---|
| `SafePath::classify(path)` | Classifica raiz (sistema / perfil atual / outro perfil / removível / rede); bloqueia `C:\Windows\System32`, perfis de outros usuários, reparse points e junctions (equivalente ao `/XJ` do robocopy). |
| `CleanupPlan::preview()` → `execute(ConfirmToken)` | Mede antes; executa só com token emitido pela UI após confirmação. MCP nunca gera token sozinho. |
| `with_backup(tweak)` / `revert()` | Exporta a chave de registro antes de aplicar; reverter restaura o valor original (não um "padrão" chutado). |
| `require_elevation()` | Confere token admin do processo; sem admin → erro claro, nunca auto-elevação. |
| `redact(log)` | Sem caminhos de perfil/PII em logs persistidos. Sem telemetria por padrão. |

## 6. Common Hurdles (armadilhas conhecidas)

- **Kaspersky × scripts .bat = `PDM:Trojan.Win32.Generic` (System Watcher, "Análise de comportamento", 2026-10-13 23:45).** Matou o `cmd.exe` do mensal no meio e apagou o .bat do Desktop (a cópia em `Referencias/` sobreviveu). Gatilhos: auto-elevação `powershell Start-Process -Verb RunAs` sobre si mesmo, `-ExecutionPolicy Bypass`, one-liners de 1.900–2.600 chars, `del /s /f /q` em Prefetch/logs/minidumps/WER (assinatura anti-forense), `net stop wuauserv/bits/dosvc`, `reg add` em `HKLM\...\Policies`. Análise, verdicts prováveis e mitigação em [kaspersky-false-positive.md](Docs/security/kaspersky-false-positive.md). **O app herda o risco se repetir os padrões** → nativo + assinatura de código + submissão de falso positivo.
- **Bug de precedência nos scripts legados.** `@('nome',$L+'\x|'+$L+'\y')` — em PowerShell a vírgula vence o `+`, então `$e[1]` vira só `$L`: "Cache de shaders", "Thumbnails", "Chrome", "Edge", "Spotify" medem a `AppData\Local` inteira (64 GB) e "Discord" mede `Roaming`; "Logs" perde os dumps. Relatório inflado/duplicado + medição lenta. Confirmado por teste em 2026-10-14. Portar a **intenção**, não os números.
- **Prefetch: não limpar** (lentidão + indicador anti-forense). **Working set trim: não** (o próprio script v2.0 removeu por piorar). `DISM /ResetBase` impede desinstalar updates — opt-in com aviso.
- **SpaceSniffer e ExitLag: análise estática feita** (imports PE, strings, drivers, serviços) em [binary-analysis.md](Docs/references/binary-analysis.md) — mecanismo no nível de API, sem copiar código/assets. Layout do treemap via `d3-hierarchy` `treemapSquarify`; render próprio em canvas.
- **`IcmpSendEcho2Ex` (iphlpapi)** não precisa de admin nem raw socket; TTL incremental = traceroute/MTR sem driver.
- **Cloudflare speedtest:** `https://speed.cloudflare.com/__down?bytes=N` e `__up`, CORS `*`; ISP/IP/cidade vêm nos headers `cf-meta-*` (o `/meta` via curl voltou `{}`). Ookla CLI é proprietário — não usar.
- **Steam SDR:** `https://api.steampowered.com/ISteamApps/GetSDRConfig/v1/?appid=730` → 53 POPs com IPs de relay (`gru` = São Paulo). Serve CS2/Dota 2/Deadlock sem chave.
- **WMIC removido no 24H2** (o script v2.0 já quebrou por isso) → WMI/CIM via API.
- **Comando Bash > ~8 KB falha no Windows** (`unexpected EOF while looking for matching`) — heredoc grande é truncado. **E heredoc colapsa `\\` em `\`** (Python com `D:\\dev` vira `D:\dev` → `unicodeescape`). Arquivo grande ou com barra invertida = tool `Write` (script no scratchpad) e rodar com `python -I`.
- **`python -I` ignora user-site** → libs no scratchpad via venv.
- **GSD global 1.16.0** (atualizado 2026-10-14); instalador avisa `node >= 24` (temos 22) mas funcionou. Hooks novos valem na próxima sessão.
- **VS Build Tools NÃO instalado (UAC cancelado, exit 1602).** O projeto compila mesmo assim pelo plano B sem admin: `D:\dev\cargo\config.toml` aponta linker `rust-lld` + libs CRT/SDK do `xwin` (`D:\dev\xwin`) + `rc.exe` oficial do NuGet (`D:\dev\winsdk-tools`). `xwin splat` falhou só no alias de headers (symlink sem privilégio) — libs completas (481 `.lib`). `vswhom-sys` (C++ dentro do `embed-resource`) é substituído por `tools/vswhom-sys-stub` via `[patch.crates-io]` — não remover sem Build Tools. Crates com código C (ex.: `rusqlite bundled`) precisariam de `clang-cl` (LLVM portátil, 900 MB) — evitar; persistência em JSON por enquanto. Para o caminho oficial, rodar o `winget` do architecture §8 aprovando o UAC.
- **Kaspersky intercepta TLS** (`pip install` falhou com erro de certificado no subagente). Downloads via `curl`/`Invoke-WebRequest` passaram; se `pip`/`cargo` reclamarem de certificado, é o AV, não a rede.
- **`tsc` é o gate de i18n**: `en.ts` usa `satisfies Record<keyof typeof ptBR, string>` — chave faltando ou sobrando quebra o typecheck. Não precisa de script separado.
- **Comandos Tauri fora do Tauri** (`bun run dev` no browser) voltam `null` via `backend.ts` (`inTauri` guard) — a UI mostra "—", não quebra.

## 7. Antes de fechar qualquer fase

- [ ] Doc da feature atualizada (data) · catálogo de limpeza revisado por risco
- [ ] Testes: unit + integração em FS sandbox + segurança (traversal, junction, symlink, MCP sem token)
- [ ] Nenhum spawn de shell novo · nenhuma deleção fora do catálogo/seleção
- [ ] `bun run check` verde (typecheck + Biome + clippy + cargo test)
- [ ] i18n pt-BR + en simétrico · tokens de design (zero hex)
- [ ] `.planning/STATE.md` atualizado

## 8. Índice de navegação

- **Roadmap / fases** → [Docs/core/next-steps.md](Docs/core/next-steps.md)
- **Arquitetura + decisão de stack** → [Docs/core/architecture.md](Docs/core/architecture.md)
- **Regras de engenharia (completo)** → [Docs/core/global-rules.md](Docs/core/global-rules.md)
- **Design system** → [Docs/core/design-system.md](Docs/core/design-system.md)
- **Referências** → [spacesniffer.md](Docs/references/spacesniffer.md) · [exitlag.md](Docs/references/exitlag.md) · [legacy-bat-scripts.md](Docs/references/legacy-bat-scripts.md)
- **Segurança** → [kaspersky-false-positive.md](Docs/security/kaspersky-false-positive.md)
- **Índice geral de docs** → [Docs/README.md](Docs/README.md)
- **Memória cross-sessão** (auto-injetada) → `~/.claude/projects/.../memory/MEMORY.md`
