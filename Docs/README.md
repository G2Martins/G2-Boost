# Documentação — G2 Boost

**Última revisão:** 2026-10-22

Índice da documentação viva. Porta de entrada do agente: [../CLAUDE.md](../CLAUDE.md). Cada doc
traz `**Última revisão:**` no topo; ler **antes** de codar e atualizar no **mesmo** PR que muda o
comportamento.

## Por onde começar

| Quero… | Leia |
| --- | --- |
| Entender a arquitetura e o porquê do Tauri | [core/architecture.md](./core/architecture.md) |
| Saber as regras (segurança, testes, i18n, docs) | [core/global-rules.md](./core/global-rules.md) |
| Ver o que está feito e o que vem | [core/next-steps.md](./core/next-steps.md) |
| Mexer na interface | [core/design-system.md](./core/design-system.md) |
| Publicar uma versão | [release/distribution-and-trust.md](./release/distribution-and-trust.md) |

## core/ — Visão, arquitetura e engenharia

| Documento | Descrição |
| --- | --- |
| [architecture.md](./core/architecture.md) | Stack (Tauri 2: core Rust + UI React), alternativas avaliadas, processos e elevação, MCP, empacotamento, toolchain sem admin fora do C: (§8). |
| [global-rules.md](./core/global-rules.md) | **Fonte única das regras**: todo caller é hostil, destrutivo = prévia + confirmação + allowlist + log, nativo (sem shell), menor privilégio, TDD, docs vivas, i18n, GSD, checklist de fase. |
| [design-system.md](./core/design-system.md) | Tokens HSL, tipografia, componentes base, gráficos e gauges. |
| [next-steps.md](./core/next-steps.md) | Roadmap por fases, o que foi feito em cada sessão, backlog. |
| [ai-advisor.md](./core/ai-advisor.md) | Conselheiro local (achados de hardware, BIOS e temperatura), context pack sem dados pessoais, assistente no app (Claude/ChatGPT/Gemini, chave no Cofre do Windows, busca só em fontes confiáveis) com playbook de segurança, ações validadas, MCP só leitura via `--mcp`. |

## Funcionalidades

| Documento | Descrição |
| --- | --- |
| [boost/dashboard.md](./boost/dashboard.md) | Painel (pontuação por hardware, pontos de restauração, limpeza, RAM), monitor em tempo real, Meu PC (CPU, memória, GPU, monitores, discos, WinSAT), **BIOS/firmware** visível pelo Windows, sensores externos, recomendação por hardware. |
| [boost/tweaks.md](./boost/tweaks.md) | Catálogo de ajustes (registro, serviços, SPI, energia, DirectX), presets, efeitos visuais em 4 níveis, backup/revert, elevação, limpeza de RAM, **inicialização e processos**, o que foi avaliado e recusado. |
| [boost/routines.md](./boost/routines.md) | Iniciar com o Windows, limpeza agendada, RAM ao abrir jogo, bandeja, configurações validadas. |
| [cleanup/catalog.md](./cleanup/catalog.md) | Catálogo de limpeza com caminhos reais, prévia, token de confirmação, execução segura contra junções, histórico. |
| [cleanup/system-files.md](./cleanup/system-files.md) | Arquivos do sistema pelos handlers da Limpeza de Disco do Windows (WinSxS, Windows.old, drivers antigos, logs), allowlist e defesa contra handler plantado. |
| [core/maintenance.md](./core/maintenance.md) | Manutenção: DISM pela DismApi (verificar/reparar imagem), SFC e MRT pela exceção da §1.3, parsers do CBS.log/mrt.log, passos de verificação elevada. |
| [disk/analyzer.md](./disk/analyzer.md) | Varredura, treemap, filtros (`*.mp4 >1gb >6m`), mostrar na pasta, Lixeira com guarda, essenciais grandes. |
| [network/network.md](./network/network.md) | Monitor (gateway/DNS/jogos), speedtest + bufferbloat, ping Steam, MTR/Ping/TCPing, DNS Optimizer com aplicar/restaurar. |
| [boost/games.md](./boost/games.md) | Jogos: biblioteca de todos os launchers, Modo Jogo (energia, prioridade, RAM, rotinas; desfaz sozinho), rota até o servidor com análise honesta e Reotimizar. |
| [boost/privacy-health.md](./boost/privacy-health.md) | Privacidade (câmera/microfone), saúde dos discos, uso de rede por app, Wi-Fi, tempo de boot, prioridade fixa, apps pouco usados, duplicados. |
| [network/game-routes.md](./network/game-routes.md) | Jogos e conexões estilo ExitLag: o que dá sem servidores (lista de jogos, Modo Jogo, diagrama MTR, reotimizar honesto, QoS) e o que exigiria relays. Proposta. |

## release/ e security/

| Documento | Descrição |
| --- | --- |
| [release/distribution-and-trust.md](./release/distribution-and-trust.md) | Assinatura de código e SmartScreen/Smart App Control, Tauri (signCommand, updater), Store e winget, falso positivo de antivírus, teste em máquina limpa, checklist até a v1, **estado atual e como instalar em outro PC**. |
| [security/kernel-driver-options.md](./security/kernel-driver-options.md) | Sensores de CPU sem driver próprio: v1 lê HWiNFO/LibreHardwareMonitor (feito), v2 PawnIO opt-in, v3 driver KMDF só leitura; WinRing0 proibido. |
| [security/kaspersky-false-positive.md](./security/kaspersky-false-positive.md) | Por que antivírus marcaram os scripts antigos e como o app evita os mesmos padrões. |

## references/ — Pesquisa

| Documento | Descrição |
| --- | --- |
| [windows-services-research.md](./references/windows-services-research.md) | Serviços do Windows com fontes: o que fazem, padrão do Windows 11, efeitos colaterais, ganho real em jogos, mitos (Prefetch, Nagle, "limpadores de RAM", VBS/HVCI). Base dos presets. |
| [legacy-bat-scripts.md](./references/legacy-bat-scripts.md) | Inventário dos scripts `.bat` antigos que viraram o catálogo de limpeza, com lições e bugs. |

Demais notas de pesquisa ficam em `references/` para uso interno da equipe e não acompanham o produto.

## Fora de `Docs/`

[README](../README.md) (visão do produto e desenvolvimento) · [SECURITY](../SECURITY.md) (modelo de ameaças e como reportar) · [LICENSE](../LICENSE) · [THIRD_PARTY_NOTICES](../THIRD_PARTY_NOTICES.md) · `.planning/` (GSD, local, fora do git).
