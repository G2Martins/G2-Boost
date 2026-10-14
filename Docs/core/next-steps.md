# Roadmap — fases e backlog

**Última revisão:** 2026-10-14 (noite)

**Estado:** Fase 0 ✅ executada em 2026-10-14 (ver seção "Feito" no fim). Fase 1 (disco) é a próxima; parte da fase 3 (rede) já saiu.

Ordem pensada pela dor: o C: do dono está com 40 GB livres e os scripts .bat estão bloqueados pelo Kaspersky — disco e limpeza primeiro; rede e boost depois; MCP quando o core estiver estável.

| Fase | Entrega | Critério de pronto |
|---|---|---|
| **0 — Decisões + ambiente** | Stack e nome do produto decididos · `git init` + `.gitignore` · toolchain instalado · scaffold Tauri (React/TS/Tailwind/shadcn) · Biome + clippy + CI lint · `.planning/` via `/gsd-new-project` | app vazio abre com o shell estilo ExitLag (sidebar + topbar + página placeholder) |
| **1 — Analisador de disco** (scan + treemap + maiores ✅ 2026-10-14; falta filtros, Lixeira, snapshot, essenciais) | scan paralelo (walk; MFT depois) · árvore · treemap canvas · filtros (`*.ext`, `\|`, `\pasta`, `>1gb`, `<3months`, atributos) · zoom/drill · tooltip · top-N · "abrir no Explorer" · mover p/ Lixeira com confirmação · snapshot | scan do C: em < 30 s sem admin (hoje 64 s para 1,43 M arquivos — MFT no backlog); testes sandbox + segurança (junction) |
| **2 — Limpeza** | catálogo portado dos .bat (diário/mensal) com risco e apps-abertos · prévia por categoria · execução nativa · histórico · "arquivos essenciais grandes" explicados (hiberfil, pagefile, WinSxS, WSL/Docker vhdx, caches IDE) | substitui os dois .bat; Kaspersky silencioso no teste local |
| **3 — Rede** | monitor (ICMP gateway/8.8.8.8/1.1.1.1; latência/jitter/perda/picos; gráfico) · speedtest Cloudflare + bufferbloat (nota A+…F) · ping p/ jogos (Steam SDR + lista curada) · MTR/Ping/TCPing · DNS ranking + aplicar/restaurar | paridade com as abas "Analisador de rede" e "DNS Optimizer" do ExitLag |
| **4 — PC Boost** | tweaks `detect/apply/revert` + backup · presets Padrão/Alto desempenho/Personalizado · plano de energia · limpeza de RAM (standby list) com allowlist e rotina | paridade com "Ajustes de sistema" e "Limpeza de RAM" |
| **5 — MCP local** | `g2boost-mcp.exe` (stdio) · tools read-only + destrutivas com token · registro no Claude Desktop/Code · doc de uso | Claude da máquina analisa o scan e propõe limpeza; humano confirma na UI |
| **6 — Release** | assinatura de código · instalador NSIS · auto-update · VirusTotal + submissão de falso positivo (Kaspersky/Microsoft) · README público | exe assinado instala limpo numa VM com Kaspersky |

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
