# Documentação — G2 Boost

**Última revisão:** 2026-10-14

Índice geral. Porta de entrada do agente: [../CLAUDE.md](../CLAUDE.md). Cada doc tem `**Última revisão:**` no topo; ler ANTES de codar, atualizar no MESMO PR que muda comportamento.

## core/ — Visão, arquitetura e engenharia

| Documento | Descrição |
|-----------|-----------|
| [architecture.md](./core/architecture.md) | Opções de stack (Tauri 2 recomendado, Electron, .NET), modelo de processos/elevação, MCP, empacotamento e assinatura. **Decisão pendente.** |
| [global-rules.md](./core/global-rules.md) | **Regras globais** — segurança (caller hostil, destrutivo, nativo), testes, docs vivas, i18n, GSD, checklist de PR. Fonte única. |
| [design-system.md](./core/design-system.md) | Tokens da paleta estilo ExitLag, tipografia, componentes base, convenção de gráficos/gauges. |
| [next-steps.md](./core/next-steps.md) | Roadmap por fases (0 → 6) e backlog. |

## references/ — Material de referência

| Documento | Descrição |
|-----------|-----------|
| [spacesniffer.md](./references/spacesniffer.md) | Spec funcional do analisador de disco extraída do manual/release notes do SpaceSniffer 2.1 (clean-room; licença proíbe engenharia reversa). |
| [exitlag.md](./references/exitlag.md) | Spec por tela do ExitLag 5.23 (prints de 2026-10-14): o que replicar, como medir, o que fica fora de escopo, fontes de dados gratuitas. |
| [binary-analysis.md](./references/binary-analysis.md) | Análise estática dos binários do ExitLag (`D:\ExitLag`, Qt 6 + driver NDIS/nfapi) e do SpaceSniffer: imports, strings, chaves de registro dos tweaks, método de limpeza de RAM, resolvers DNS. |
| [legacy-bat-scripts.md](./references/legacy-bat-scripts.md) | Inventário dos scripts `Limpador Diario/Mensal 2026.bat` → catálogo de categorias de limpeza e tweaks, lições e bugs. |

## security/ — Segurança

| Documento | Descrição |
|-----------|-----------|
| [kaspersky-false-positive.md](./security/kaspersky-false-positive.md) | Por que o Kaspersky flagra os scripts, verdicts prováveis, como confirmar, mitigação nos scripts e no app novo (assinatura, nativo, submissão). |
