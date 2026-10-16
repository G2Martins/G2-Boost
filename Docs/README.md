# Documentação — G2 Boost

**Última revisão:** 2026-10-15

Índice geral. Porta de entrada do agente: [../CLAUDE.md](../CLAUDE.md). Cada doc tem `**Última revisão:**` no topo; ler ANTES de codar, atualizar no MESMO PR que muda comportamento.

## core/ — Visão, arquitetura e engenharia

| Documento | Descrição |
|-----------|-----------|
| [architecture.md](./core/architecture.md) | Stack decidida (Tauri 2), alternativas avaliadas, modelo de processos/elevação, MCP, empacotamento e assinatura, toolchain sem admin (§8). |
| [global-rules.md](./core/global-rules.md) | **Regras globais** — segurança (caller hostil, destrutivo, nativo), testes, docs vivas, i18n, GSD, checklist de PR. Fonte única. |
| [design-system.md](./core/design-system.md) | Tokens da paleta estilo ExitLag, tipografia, componentes base, convenção de gráficos/gauges. |
| [next-steps.md](./core/next-steps.md) | Roadmap por fases e backlog; seção "Feito" por sessão. |
| [ai-advisor.md](./core/ai-advisor.md) | Fase 9 implementada: conselheiro local (17 achados + classe para jogos), context pack sem PII, assistente no app (Claude SDK / ChatGPT / Gemini, BYOK no Cofre do Windows, busca restrita a fontes de benchmark), servidor MCP stdio só leitura e como registrar. |

## boost/ — PC Boost

| Documento | Descrição |
|-----------|-----------|
| [tweaks.md](./boost/tweaks.md) | Catálogo de tweaks (registro, serviços, SPI, energia), efeitos visuais em níveis (data-driven), backup/revert, elevação, purge de RAM, testes. |
| [routines.md](./boost/routines.md) | Iniciar com o Windows (tarefa de logon), limpeza agendada, RAM ao abrir jogo (detecção Steam/exe), bandeja, settings validados, testes. |
| [dashboard.md](./boost/dashboard.md) | Painel (KPIs: pontuação por hardware, pontos de restauração, bytes para limpar, RAM), monitor em tempo real (PDH/NVML), Meu PC (SMBIOS, DXGI, IOCTLs, TPM, Secure Boot), regras de recomendação. |

## cleanup/ — Limpeza

| Documento | Descrição |
|-----------|-----------|
| [catalog.md](./cleanup/catalog.md) | Catálogo com caminhos reais, prévia, `ConfirmToken`, execução nativa (junction-safe), histórico, o que ficou fora e por quê. |

## references/ — Material de referência

| Documento | Descrição |
|-----------|-----------|
| [spacesniffer.md](./references/spacesniffer.md) | Spec funcional do analisador de disco extraída do manual/release notes do SpaceSniffer 2.1 (clean-room; licença proíbe engenharia reversa). |
| [exitlag.md](./references/exitlag.md) | Spec por tela do ExitLag 5.23 (prints de 2026-10-14): o que replicar, como medir, o que fica fora de escopo, fontes de dados gratuitas. |
| [binary-analysis.md](./references/binary-analysis.md) | Análise estática dos binários do ExitLag (`D:\ExitLag`, Qt 6 + driver NDIS/nfapi) e do SpaceSniffer: imports, strings, chaves de registro dos tweaks, método de limpeza de RAM, resolvers DNS. |
| [legacy-bat-scripts.md](./references/legacy-bat-scripts.md) | Inventário dos scripts `Limpador Diario/Mensal 2026.bat` → catálogo de categorias de limpeza e tweaks, lições e bugs. |
| [windows-services-research.md](./references/windows-services-research.md) | Pesquisa com fontes (Galdino Tech, winutil, Microsoft, benchmarks): o que cada serviço faz, padrão do Win11, efeitos colaterais, ganho real para jogos, mitos (Prefetch, Nagle, RAM cleaners, VBS/HVCI). Base das decisões de preset. |
| [speccy-cpuz.md](./references/speccy-cpuz.md) | Análise estática do CPU-Z 3.01 e Speccy 1.34: os dois carregam o driver de kernel `cpuz.sys`; tabela funcionalidade × mecanismo × o que fazemos sem driver; licença; o que não copiar. |
| [galdino-optimizer.md](./references/galdino-optimizer.md) | Extração do story do Galdino Optimizer (2026-10-14): painel com KPIs, pontuação de otimização, pontos de restauração, monitor em tempo real, ações recomendadas, Meu PC — com a API nativa de cada item e o que fica fora (temperatura de CPU exige driver). |

## security/ — Segurança

| Documento | Descrição |
|-----------|-----------|
| [kaspersky-false-positive.md](./security/kaspersky-false-positive.md) | Por que o Kaspersky flagra os scripts, verdicts prováveis, como confirmar, mitigação nos scripts e no app novo (assinatura, nativo, submissão). |
