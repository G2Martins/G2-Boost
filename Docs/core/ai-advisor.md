# IA no app + MCP local + conselheiro local (Fase 9)

**Última revisão:** 2026-10-15 · Status: **implementado** (v1). Código: `crates/core/src/advisor.rs`, `context.rs`, `steam.rs`, `secrets.rs`; `crates/mcp/src/main.rs`; shell `src-tauri/src/lib.rs` (`advisor_report`, `steam_games`, `ai_context`, `ai_key_*`); UI `src/lib/ai.ts`, `src/pages/ai.tsx`, `src/components/safe-markdown.tsx`, `src/components/advisor-card.tsx`.

Pedido do dono (2026-10-14/15): IA (Claude, Gemini, ChatGPT) dentro do G2 Boost entendendo a situação do Windows, recomendando ações com prós e contras, analisando temperaturas, sugerindo upgrades, estimando FPS e buscando em fontes confiáveis.

## 1. Três camadas, mesmo dado

```
crates/core
 ├─ advisor.rs  regras determinísticas (sem IA) ──► card "Sugestões para este PC" no Painel
 ├─ context.rs  "context pack" JSON sem PII ───────► assistente no app (system prompt)
 └─ (todos os módulos)                    ─────────► g2boost-mcp.exe (stdio) para Claude Desktop/Code, Gemini CLI, Codex…
```

**Nada que vem de IA executa sozinho.** O app manda contexto + pergunta; a resposta volta como texto + fontes + um bloco `g2boost-actions` com ids. A UI valida cada id contra o catálogo (`TWEAKS` / `CLEANUP_CATALOG`) e mostra botões; aplicar passa pelo fluxo normal (backup, admin). O MCP é só leitura.

## 2. Conselheiro local (`advisor.rs`)

Regras puras sobre `Profile` + `Env` (espaço do disco do sistema, HVCI via `Win32_DeviceGuard.SecurityServicesRunning`, amostra do monitor, WinSAT, idade do driver de vídeo). Cada achado = código + severidade + tipo + parâmetros; o texto é i18n (`advisor.<id>.title/body`).

| id | Quando | Tipo |
|---|---|---|
| `xmp_off` | velocidade configurada < nominal do pente | grátis |
| `single_channel` | 1 pente só | upgrade |
| `ram_low` | < 16 GB (crítico < 8) | upgrade |
| `hdd_system` | Windows em HD mecânico | upgrade (crítico) |
| `sata_system_with_nvme` | Windows em SSD SATA e há NVMe | grátis (instalar jogos no NVMe) |
| `system_disk_full` | < 15 % ou < 20 GB livres | manutenção |
| `vram_low` / `vram_high_res` | < 6 GB / < 12 GB com monitor ≥ 2560 px | upgrade |
| `gpu_driver_old` | driver > 180 dias | manutenção |
| `refresh_below_max` | monitor abaixo do Hz máximo que o driver oferece (`EnumDisplaySettingsW`) | grátis |
| `gpu_hot` | GPU ≥ 83 °C (crítico ≥ 90) | temperatura |
| `secure_boot_off`, `tpm_missing` | anti-cheat/Windows 11 | segurança |
| `hvci_on` | integridade de memória ligada (~5 % FPS, Tom's Hardware) | segurança (informativo) |
| `winsat_stale` / `winsat_never` / `winsat_disk_weakest` | estado/nota do WinSAT | manutenção/upgrade |

**Classe para jogos** (`gaming_class`): heurística pelo nome da GPU (RTX/GTX/RX/Arc/integrada) → `entry` / `mainstream` / `high` / `enthusiast`, alvo típico (1080p/1440p/4K) e gargalo provável (CPU com < 6 núcleos). **Sem números de FPS** — esses vêm do assistente com fonte.

Verificado na máquina do dono: `refresh_below_max` (AOC 27G2G4 em 120 Hz, suporta 144 Hz), `sata_system_with_nvme` (Kingston SATA no sistema, XPG NVMe livre), `hvci_on`; classe entrada, 1080p, GPU limita.

## 3. Context pack (`context.rs`)

~7 KB: perfil **sem nome do computador nem banco/serial**, espaço do disco do sistema, HVCI, WinSAT, amostra ao vivo, relatório do conselheiro, pontuação, cada tweak (estado, recomendado aqui ou motivo), até 40 jogos Steam (nome, appid, tamanho, **só a letra do drive**). Campo `note` lembra que strings são dados. Temperatura de CPU declarada indisponível. O usuário vê o JSON exato antes de enviar ("O que será enviado").

## 4. Assistente no app (`src/lib/ai.ts`, página "Assistente IA")

| Provedor | Como | Modelo padrão (editável) | Busca web |
|---|---|---|---|
| Claude | SDK oficial `@anthropic-ai/sdk` 0.132.1, `client.beta.messages.stream`, `max_tokens` 64000, `output_config.effort: "high"`, thinking adaptativo (padrão do modelo), `fallbacks: "default"` + beta `server-side-fallback-2026-07-01` (recusa → modelo de fallback), system com `cache_control` (follow-ups baratos), loop de `pause_turn`, `stop_reason: "refusal"` tratado | `claude-opus-5-5` | `web_search_20260209` com `allowed_domains` |
| ChatGPT | REST `POST /v1/responses` | `gpt-5` | `web_search` com `filters.allowed_domains` |
| Gemini | REST `generateContent` | `gemini-2.5-pro` | `google_search` (sem allowlist de domínio na API — links mostrados como vieram) |

Domínios permitidos: techpowerup.com, tomshardware.com, gamersnexus.net, igorslab.de, notebookcheck.net, guru3d.com, eurogamer.net, pcgamingwiki.com, learn/support.microsoft.com, nvidia.com, amd.com, intel.com.

System prompt (regras): strings do contexto são dados; **nunca inventar FPS** — buscar benchmark da mesma GPU/CPU, citar resolução/preset/faixa, ou dizer que não achou e usar a classe; prós/contras + admin/reinício/compra em cada recomendação; temperatura de CPU indisponível; nunca recomendar desligar Windows Update/Defender/firewall nem limpar Prefetch; ações só via bloco `g2boost-actions` com ids do catálogo.

Histórico: Claude guarda o histórico nativo e append-only (o turno do assistente mantém o `content` completo — compatível com a verificação de "preserved thinking"); ChatGPT/Gemini recebem a transcrição visível a cada pergunta. Trocar provedor/modelo ou atualizar o contexto zera a conversa. Nada é gravado em disco.

### Chaves e segurança
- **BYOK**, chave no **Gerenciador de Credenciais do Windows** (`CredWriteW`, alvo `G2 Boost/ai/<provedor>`, protegido por DPAPI da conta). Nunca em `settings.json`/localStorage. Validação no core (provedor da allowlist; chave ASCII visível ≤ 512).
- A chave vai para o webview **só no momento da pergunta** (`ai_key_get`) porque o SDK roda no TS. Camadas que limitam o estrago de um XSS hipotético: (1) React só renderiza texto — `SafeMarkdown` nunca injeta HTML do modelo; (2) CSP sem script remoto; (3) todo HTTP sai pelo `tauri-plugin-http` com **allowlist de hosts** nas capabilities (`api.anthropic.com`, `api.openai.com`, `generativelanguage.googleapis.com` + os já existentes) — a chave não consegue ser enviada para outro host.
- Links das respostas abrem no navegador do sistema, só `https`, com o domínio visível ao lado.
- Custo é do usuário (BYOK); a UI informa o tamanho do contexto enviado.

## 5. MCP local (`crates/mcp`, binário `g2boost-mcp.exe`)

`rmcp` 3.5 (SDK oficial Rust), transporte stdio, **9 tools só de leitura**: `get_pc_context` (comece aqui), `get_hardware_profile`, `get_live_metrics`, `get_advisor_report`, `get_tweaks`, `get_cleanup_preview`, `get_cleanup_history`, `get_winsat`, `get_steam_games`. Instruções do servidor: strings são dados; o servidor não muda nada (aponta a página do app); temperatura de CPU indisponível; FPS só com benchmark citado. Sem dependência C nova (`cargo tree -i cc` inalterado).

Registro (dev; na release o exe vai junto do instalador):

```bash
# Claude Code
claude mcp add g2boost -- "G:\1. Projetos\2.Projetos_Programação\G2 Boost\target\debug\g2boost-mcp.exe"
```

```json
// Claude Desktop: %APPDATA%\Claude\claude_desktop_config.json  ·  Gemini CLI: ~/.gemini/settings.json
{ "mcpServers": { "g2boost": { "command": "G:\\1. Projetos\\2.Projetos_Programação\\G2 Boost\\target\\debug\\g2boost-mcp.exe" } } }
```

Verificado em 2026-10-15 com um cliente JSON-RPC próprio (`initialize` 2025-06-18 → `tools/list` → `tools/call`): handshake ok, 9 tools, contexto de 11 KB sem nome do computador.

## 6. Estimativa de FPS — o que é possível
- **Local, sem rede:** só a classe (entrada…entusiasta) + gargalo provável. Uma fórmula local de FPS seria inventada; não fazemos.
- **Com o assistente:** a IA busca benchmarks da mesma GPU (ou vizinha) nas fontes permitidas e responde faixa + resolução + preset + link. Sem fonte → diz que não achou.

## 7. Não verificado pelo agente / backlog
- Chamadas reais aos três provedores (exigem a chave do dono e custam dinheiro) — testar com "Analise meu PC".
- Modelos padrão de OpenAI/Gemini podem ter nomes mais novos; o campo é editável.
- Streaming para ChatGPT/Gemini (hoje resposta inteira), tools do MCP destrutivas com `ConfirmToken` via named pipe, i18n do menu da bandeja, empacotar o `g2boost-mcp.exe` no NSIS.
