# Console G2

**Última revisão:** 2026-10-23 (atualizar todos na aba Atualizações) · 2026-10-18 · Código: `crates/core/src/console.rs`, `winget.rs` (parser + comandos), `disk::usage` (pasta com prazo), `winevents.rs` (erros); shell `src-tauri/src/lib.rs` (`console_commands`, `console_run`); UI `src/pages/console.tsx` (rota **Console**), `src/lib/luli.tsx` (`askOnce`), `src/lib/ai.ts` (`console` no bloco).

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

## winget (2026-10-18)

`winget buscar <termo>`, `winget atualizacoes`, `winget instalar <id>`, `winget atualizar <id>` (`crates/core/src/winget.rs`).

- **Como:** o `winget.exe` oficial (alias do App Installer em `%LOCALAPPDATA%\Microsoft\WindowsApps`) chamado direto, sem shell, com argumentos fixos (`--source winget --accept-source-agreements --disable-interactivity`), sem janela, stdin fechado, saída limitada a 4 MB, prazo (busca 90 s, instalação 45 min) e kill. Exceção registrada em [global-rules §1.3](./global-rules.md).
- **Leitura da saída:** tabela alinhada pelo cabeçalho, traduzida, em UTF-8 (o OEM fica de reserva). Cada linha é decodificada antes e cortada nas colunas do cabeçalho contadas em caracteres (cortar por byte desalinha "Versão"). O spinner reescreve a linha com `\r` e só o que ficou visível conta. O rodapé "N atualizações disponíveis." sai porque o id não é válido.
- **Instalar/atualizar:** o console mostra a prévia (`winget search --id X --exact`) num bloco de confirmação com token de uso único (2 min, amarrado à ação e ao id). O clique chama `console_confirm`, que roda `install|upgrade --id X --exact --silent --accept-package-agreements` e registra o resultado em `winget-log.json`. O instalador do app pode pedir administrador.
- **Verificado na máquina do dono:** busca (Valve.Steam), pacote exato, id inexistente, lista de atualizações (Claude, GitHub CLI, .NET Desktop Runtime 8, Terminal). Instalação real fica para o dono confirmar no console.
- **Atualizar todos (2026-10-23):** a aba **Desinstalar apps › Atualizações** faz o `winget upgrade --all` sem usar `--all`: um `install(id, true)` validado por pacote, com seleção, exclusões salvas, token amarrado à lista, progresso, cancelar entre pacotes e histórico. Ver [updates.md](./updates.md).

### Por que não a API COM (histórico do teste)

1. Ativação fora do processo: `0x80073D54` (APPMODEL_ERROR_NO_PACKAGE). O proxy de `IPackageManager` ({B375E3B9-F2E0-5C93-87A7-B67497F7E593}) só existe como `windows.activatableClass.proxyStub` dentro do pacote do App Installer.
2. A DLL do App Installer não carrega dentro do processo: `LoadLibrary` em `WindowsApps` dá acesso negado para exe sem pacote.
3. A DLL cliente do NuGet `Microsoft.WindowsPackageManager.ComInterop` 1.29.380 (MIT, assinada pela MS, testada com o SHA-256 fixado) cria o objeto, mas falha igual na primeira chamada. Ela só ativa e não traz proxy.
4. **Pacote esparso MSIX** (dar identidade ao exe):
   - Este Windows (26200) recusa o publisher "sem assinatura" (`OID.2.25.311729368913984317654407730594956997722=1`) no manifesto do exe: "O fornecedor msix deve ser válido". O exe nem abre.
   - Com `CN=GrovicData`, o exe abre normal sem o pacote registrado.
   - O pacote assinado foi gerado com `makeappx` e `signtool` (NuGet SDK BuildTools em `D:\dev\sdk-buildtools`, certificado de teste no repositório do usuário).
   - Registrar exige confiar no certificado: o passo foi barrado pela proteção do agente e aguarda o dono. Com a assinatura de release, o mesmo caminho vale sem certificado de teste.

## Testes

`console::tests`: tokenizer (aspas), apelidos únicos, linhas hostis recusadas (vazia, longa, NUL, `del /s /q`, host com `;`, porta inválida, tela fora da lista), comandos locais respondem (`disco`, `energia`, `inicializacao`, `rede`, `erros 1`, `pc`, `pasta` no temp) e a data do Event Log. `tests/ai.test.ts`: sugestões `console` limitadas e o trecho do prompt só aparece quando perguntado do console. E2E: a barra lateral chega ao Console.
