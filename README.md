# G2 Boost

Otimizador de PC para Windows 10/11, com interface no estilo ExitLag: analisador de disco (treemap), limpeza nativa do Windows, analisador de rede (speedtest, latência, DNS) e ajustes de desempenho para jogos. Tauri 2 (Rust + React).

- **Spec viva / porta de entrada:** [CLAUDE.md](CLAUDE.md)
- **Docs:** [Docs/README.md](Docs/README.md) — arquitetura, design system, regras, roadmap, referências (ExitLag, SpaceSniffer, scripts legados), segurança (Kaspersky).

## Rodar

```bash
bun install
bun run tauri:dev     # app completo (primeira compilação do Rust demora alguns minutos)
bun run dev           # só a UI no browser
bun run check         # typecheck + lint (Biome, clippy) + cargo test
```

Toolchain Rust e caches ficam em `D:\dev` (ver [Docs/core/architecture.md §8](Docs/core/architecture.md)).

## Princípios

Todo caller é hostil (UI, MCP, arquivos). Nada destrutivo sem prévia e confirmação. Nenhum spawn de `cmd`/`powershell`: só API Win32. Sem telemetria.
