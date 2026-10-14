# Roadmap — fases e backlog

**Última revisão:** 2026-10-14

Ordem pensada pela dor: o C: do dono está com 40 GB livres e os scripts .bat estão bloqueados pelo Kaspersky — disco e limpeza primeiro; rede e boost depois; MCP quando o core estiver estável.

| Fase | Entrega | Critério de pronto |
|---|---|---|
| **0 — Decisões + ambiente** | Stack e nome do produto decididos · `git init` + `.gitignore` · toolchain instalado · scaffold Tauri (React/TS/Tailwind/shadcn) · Biome + clippy + CI lint · `.planning/` via `/gsd-new-project` | app vazio abre com o shell estilo ExitLag (sidebar + topbar + página placeholder) |
| **1 — Analisador de disco** | scan paralelo (walk; MFT depois) · árvore · treemap canvas · filtros (`*.ext`, `\|`, `\pasta`, `>1gb`, `<3months`, atributos) · zoom/drill · tooltip · top-N · "abrir no Explorer" · mover p/ Lixeira com confirmação · snapshot | scan do C: em < 30 s sem admin; testes sandbox + segurança (junction) |
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
