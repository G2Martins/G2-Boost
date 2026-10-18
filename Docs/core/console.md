# Console G2

**Última revisão:** 2026-10-18 · Código: `crates/core/src/console.rs` (parser + comandos), `disk::usage` (pasta com prazo), `winevents.rs` (erros); shell `src-tauri/src/lib.rs` (`console_commands`, `console_run`); UI `src/pages/console.tsx` (rota **Console**), `src/lib/luli.tsx` (`askOnce`), `src/lib/ai.ts` (`console` no bloco).

Pedido do dono (2026-10-18): um terminal dentro do G2 Boost para executar comandos, com a IA sugerindo o que rodar (o apelo do Homebrew e do Rune). Decisão: **Console G2 nativo**, não um shell. Ideias de Rune (blocos por comando, sugestões da IA clicáveis) usadas como conceito; o código do Rune é proprietário e não foi copiado. O `homebrew` da pasta de referências é só um tap que instala o Rune; o análogo no Windows é o winget (ver Pendente).

## O que é

- Lista fixa de comandos, cada um ligado a uma função do core. **Nada muda o PC**: mede e mostra. `abrir` e `limpar` são efeitos de UI.
- A linha é entrada hostil: até 512 caracteres, sem caracteres de controle, tokenizada no Rust (aspas agrupam caminhos, sem escape nem expansão). Cada comando valida os próprios argumentos (host só `[A-Za-z0-9.-:]`, números com limite, porta 1..65535, tela de uma allowlist, pasta absoluta local via `disk::scan`).
- Saída estruturada: tabelas (ids de coluna traduzidos em `console.col.*`; colunas `part` e `hint` também traduzidas) e notas i18n (`console.note.*`). Erros também são chaves (`console.err.*`).

| Comando | Apelidos | Faz |
|---|---|---|
| `ajuda` | help, ? | lista |
| `ping <host> [vezes]` | | ICMP (`IcmpSendEcho2`), 1 a 20, estatística |
| `rota <host>` | tracert, traceroute | salto a salto (TTL), 30 saltos |
| `tcping <host>[:porta]` | | conexão TCP x4 (443 por padrão) |
| `dns <nome>` | nslookup, resolver | resolver do Windows (`ToSocketAddrs`) |
| `rede` | ip, ipconfig, network | adaptadores, gateway, DNS, provedor |
| `processos [n]` | top, ps | mais pesados (1 s de amostra) |
| `disco` | df, drives | espaço por unidade |
| `pasta <caminho> [s]` | du, folder | varredura com prazo (5 a 110 s), filhos maiores |
| `energia` | power, plans | planos |
| `apps [filtro]` | programas | instalados, maiores primeiro (60) |
| `erros [dias]` | eventos, errors, events | Visualizador de Eventos agrupado com dica |
| `pc` | hardware, specs | resumo do hardware |
| `sensores` | temps, sensors | HWiNFO/LibreHardwareMonitor |
| `inicializacao` | startup | itens de inicialização |
| `abrir <tela>` | open, ir | navega para uma tela |
| `limpar` | clear, cls | limpa |

## Luli no console

- `? pergunta` pergunta à Luli **fora** da conversa principal (`askOnce`: mesmo provedor, chave, contexto e base de conhecimento; web conforme a configuração global). O prompt recebe a lista de comandos e a resposta pode trazer `"console": [...]` (até 5 linhas), que viram botões **Rodar**. A UI só mostra linhas cujo primeiro termo é um comando conhecido e o Rust valida de novo ao rodar.
- Cada bloco com resultado tem **Explicar com a Luli**: a pergunta leva a tabela do bloco como `<page_data>` (12 KB no máximo, `<` escapado).

## UI

Blocos com borda por estado (rodando, ok, erro), duração, repetir, copiar (TSV). Tab completa, ↑↓ histórico (100 linhas no `localStorage`, só no PC), Ctrl+L limpa. Os blocos ficam numa store de módulo: sair da tela não perde nada e um comando lento termina mesmo fora dela.

## Pendente: winget (decisão do dono)

Plano era `winget buscar/atualizacoes/instalar/atualizar` pela **API COM** (`Microsoft.Management.Deployment`), com prévia + token de uso único antes de instalar. Bindings gerados com `windows-bindgen` a partir do `.winmd` do App Installer e CLSIDs conferidos no AppxManifest. Bloqueios encontrados em 2026-10-18:

1. Ativação fora do processo: `0x80073D54` (APPMODEL_ERROR_NO_PACKAGE). O proxy/stub das interfaces só é resolvido para processos com identidade de pacote.
2. Ativação dentro do processo com a DLL do App Installer: `LoadLibrary` negado (`0x80070005`) para código em `WindowsApps` vindo de um exe sem pacote.
3. O caminho oficial para apps sem pacote é a DLL cliente do NuGet `Microsoft.WindowsPackageManager.ComInterop` 1.29.380 (MIT, assinada pela Microsoft, 130 KB), distribuída junto do app. Carregar binário baixado precisa de aprovação do dono antes de testar.

O trabalho (bindings, `winget.rs` validando ids e consultas, ferramenta de geração) está guardado fora do repositório até a decisão.

## Testes

`console::tests`: tokenizer (aspas), apelidos únicos, linhas hostis recusadas (vazia, longa, NUL, `del /s /q`, host com `;`, porta inválida, tela fora da lista), comandos locais respondem (`disco`, `energia`, `inicializacao`, `rede`, `erros 1`, `pc`, `pasta` no temp) e a data do Event Log. `tests/ai.test.ts`: sugestões `console` limitadas e o trecho do prompt só aparece quando perguntado do console. E2E: a barra lateral chega ao Console.
