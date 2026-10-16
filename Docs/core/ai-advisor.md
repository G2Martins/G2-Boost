# IA no app + MCP local — desenho (Fase 9)

**Última revisão:** 2026-10-15 · Status: **proposta** (nada implementado). Pedido do dono (2026-10-14/15): a IA (Claude, Gemini, ChatGPT) dentro do G2 Boost deve entender a situação do Windows, recomendar ações com prós e contras, analisar temperaturas, sugerir upgrades para rodar jogos, estimar FPS e, quando precisar, buscar em fontes confiáveis.

## 1. Uma camada de ferramentas, dois consumidores

```
crates/core  ──► crates/tools (registry: nome, descrição, JSON Schema, handler)
                    │
                    ├─► app (chat na UI): SDK oficial do provedor no webview, transporte HTTP via tauri-plugin-http
                    │     (sem CORS, TLS nativo), tools executadas por comandos Tauri
                    └─► g2boost-mcp.exe (stdio, `rmcp` 3.x, sem código C) para Claude Desktop/Code, Gemini CLI etc.
```

- **Mesmas tools nos dois lugares**, definidas uma vez em Rust (schema `schemars`). Tudo **read-only ou "propor"**; executar continua sendo clique humano no fluxo normal (backup/`ConfirmToken`). A IA nunca aplica tweak nem apaga arquivo sozinha.
- **Prompt injection:** nomes de arquivo, processos, títulos de janela, resultados de busca são **dados**. O system prompt diz isso; as tools devolvem JSON tipado, nunca texto livre vindo do disco sem rótulo.
- **Chaves:** BYOK por provedor, guardadas no **Gerenciador de Credenciais do Windows** (`CredWriteW`/`CredReadW`, nativo), nunca em JSON. Sem chave → o chat sugere o MCP (usa a assinatura que o usuário já tem no Claude Desktop).

## 2. Tools (v1)

| Tool | Devolve | Fonte |
|---|---|---|
| `pc_profile` | `hardware::Profile` + `Facts` + score + WinSAT | fase 6 / 6.5 |
| `monitor_snapshot` | últimas 60 amostras (CPU/GPU/RAM/disco/rede, temp. GPU) | `monitor.rs` |
| `tweaks_state` | estado + recomendação + motivo por tweak | `boost::detect_all` + `recommend` |
| `cleanup_preview` | bytes por categoria | `cleanup::preview` |
| `history` | últimas execuções (sem caminhos) | `cleanup::history` |
| `games_installed` | biblioteca Steam (`steamapps/*.acf`: nome, appid, tamanho) | leitura de arquivos, sem rede |
| `propose_plan` | **escreve um plano** (ids de tweak/categoria + justificativa) que a UI renderiza com botões Aplicar/Limpar | não executa |

## 3. Contexto do Windows ("context pack")

Um JSON compacto (≤ 8 KB) montado pelo core: edição/build, CPU (nome, núcleos, flags AVX2/AVX-512), RAM (total, canais, velocidade configurada vs. máxima → "XMP desligado?"), GPU (nome, VRAM, driver, PCIe link), discos (SSD/HD, barramento, espaço livre), monitores (resolução, Hz), notebook/bateria, Secure Boot/TPM/HVCI, serviços-chave, tweaks ativos, score, WinSAT, temps disponíveis (GPU; CPU "indisponível sem driver"), jogos instalados. É o que a IA "vê" — sem PII (sem nome de usuário, serial, IP).

## 4. Estimativa de FPS

Não existe fórmula local honesta. Duas camadas:

1. **Classe de hardware (local, determinística):** tabela própria pequena (faixas por geração/série: GPU tier × CPU tier × resolução) → "entrada / médio / alto / entusiasta" + aviso de gargalo (ex.: RTX 3050 em 1080p + i5-10400 = GPU-bound; 8 GB VRAM ok em 1080p). Sem números de FPS.
2. **Números com fonte (IA + busca web):** a tool de busca do provedor com **`allowed_domains`** (Anthropic `web_search_20260209` aceita allowlist; OpenAI/Gemini têm busca própria) restrita a fontes de benchmark reputadas (techpowerup.com, tomshardware.com, gamersnexus.net, igorslab.de, notebookcheck.net, pcgamingwiki.com para requisitos). A IA responde "CS2 1080p médio: ~X–Y FPS (fonte: …)" com link, e diz quando não achou. Nada de tabela copiada de terceiros embutida no app (direitos autorais do TechPowerUp etc.).

## 5. Temperaturas e upgrades

- Temps: GPU via NVML/ADLX; discos via SMART (admin); **CPU só se HWiNFO (shared memory) ou LibreHardwareMonitor (WMI) estiverem rodando** — integração opcional, lida como dado. Sem isso, a IA diz que não tem o dado.
- Upgrades que o core já consegue **detectar sozinho** (regras, sem IA): XMP desligado (velocidade configurada < máxima do SPD), single channel, RAM < 16 GB, SSD SATA no sistema com slot NVMe livre (SMBIOS/PCI), VRAM < 8 GB em 1440p+, driver NVIDIA velho, Secure Boot off, HVCI on (ganho ~5 % se desligar, custo de segurança). A IA explica e prioriza; o app mostra como "Sugestões" com prós/contras.

## 6. Provedores

| Provedor | Modelo padrão | Busca web | Tool use |
|---|---|---|---|
| Anthropic | `claude-opus-5-5` (adaptive thinking; streaming; `fallbacks: "default"`) | `web_search_20260209` + `allowed_domains` | sim (tools cliente) |
| OpenAI | o modelo GPT atual do usuário | Responses API web search | sim |
| Google | Gemini atual | Google Search grounding | sim |

SDKs oficiais JS no webview com `fetch` do `tauri-plugin-http`. Loop de tools no TS (ou tool runner do SDK), streaming para a UI, histórico por sessão em memória (sem gravar conversa em disco por padrão).

## 7. MCP stdio

`g2boost-mcp.exe` = `rmcp` (features `server`, `transport-io`; verificado: nenhum crate C). Registro: `claude mcp add g2boost -- "<path>\g2boost-mcp.exe"`. Expõe as tools da seção 2 (read-only + `propose_plan`). Tools destrutivas ficam fora até existir o named pipe com `ConfirmToken` emitido pela UI (architecture.md §6).

## 8. Fora / riscos
- Sem auto-aplicar. Sem enviar o contexto para a nuvem sem o usuário clicar "Analisar" (consentimento explícito, mostrar o que vai).
- Custo: BYOK é do usuário; mostrar tokens/custo estimado por análise.
- Prompt injection via nomes de jogos/arquivos: rótulo de dado + sem instruções em tool results.
