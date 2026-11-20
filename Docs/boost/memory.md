# Memória RAM: composição, limpeza do cache em espera e quem usa

**Última revisão:** 2026-11-20 (rodada 11, bloco A5) · Código: `crates/core/src/memory.rs` (composição PDH, agrupamento, conselho, relatório da IA), `boost.rs` (`purge_standby_list`, `purge_result`), `procs.rs` (memória privada por processo), `startup.rs` (`exe` do item), `icons.rs` (ícone por caminho) · Shell: `src-tauri/src/ram_cmds.rs` (`ram_composition`, `ram_apps`), `exe_icon` em `lib.rs` · UI: `src/components/boost/ram-tab.tsx` (aba RAM do PC Boost e lista "Quem usa a RAM"), aba Inicialização em `src/pages/boost.tsx`, helpers puros em `src/lib/ram-apps.ts` · MCP: `get_memory`.

## Composição ao vivo (sem admin)

PDH com nomes em inglês (independe do idioma do Windows), uma consulta por processo mantida aberta (`memory::composition`):

| Parte | Contador | O que é |
|---|---|---|
| Livre | `\Memory\Free & Zero Page List Bytes` | páginas vazias, uso imediato |
| Cache em espera | soma de `Standby Cache Core/Normal Priority/Reserve Bytes` | arquivos lidos há pouco; o Windows entrega na hora a quem pedir |
| Modificada | `\Memory\Modified Page List Bytes` | alterada pelos apps, esperando ir para o disco |
| Em uso | total − livre − espera − modificada | a conta do Gerenciador de Tarefas (inclui a comprimida) |
| Comprimida | `\Process(Memory Compression)\Working Set` | só quando o processo aparece (compressão ligada); senão some da tela |
| Confirmada | `Committed Bytes` / `Commit Limit` | linha de rodapé |

O total vem do `GlobalMemoryStatusEx`. Contadores incoerentes nunca dão negativo (`saturating_sub`). A UI lê a cada 2,5 s só com a aba aberta e a janela visível; a lista de apps a cada 5 s.

## Quando limpar ajuda (regra única no core, `memory::compose`)

- `helps`: livre < 5% do total **e** cache em espera > 25%. Purge entrega páginas livres na hora; corta engasgo ao carregar quando a lista livre está seca.
- `in_use`: em uso > 85%. O problema é uso real: limpar não resolve, feche apps ou tire-os da inicialização.
- `fine`: o resto. O cache volta sozinho para quem precisar; limpar não muda nada no jogo.

O mesmo código vai para a tela, para a Luli (base `knowledge/`) e para o MCP.

## Limpeza (admin)

`purge_standby_list()`: `SeProfileSingleProcessPrivilege` + `NtSetSystemInformation(80, MemoryPurgeStandbyList=4)`. **Nunca** apara working set (`EmptyWorkingSets` piora, decisão antiga mantida). Mede a composição antes e 250 ms depois:

- `freed_bytes` = o que a lista **livre** ganhou (até a 0.9.3 era o delta de "disponível", que quase não muda: o cache em espera já conta como disponível; por isso aparecia "Nada a liberar" com 8 GB de cache). Sem PDH, volta ao delta de disponível.
- `standby_before/after`, `free_before/after` alimentam o "antes → depois" da aba; a barra segmentada encolhe o trecho do cache e cresce o livre (transição de `flex-grow`, `--motion-slow`; parada com `prefers-reduced-motion`), e o liberado entra com `CountUp`.
- O botão Limpar fica desabilitado sem admin (com "Elevar" ao lado). O Painel continua chamando o mesmo `ram_purge`.
- **Limpar ao abrir jogo** (`game_ram_clean`): quando roda em segundo plano e libera algo, vira linha no sino (`activity.ram.game`); `needs_admin` não vira linha (repetiria a cada jogo).

## Quem usa a RAM

`memory::group` agrupa o `procs::Sampler` por **caminho do executável** (mesmo exe em qualquer caixa = um app; outro diretório = outro app), soma memória privada (`PROCESS_MEMORY_COUNTERS_EX.PrivateUsage`, sem contar duas vezes o que a família divide) e conjunto de trabalho, ordena pela privada. Um grupo só é protegido se **todos** os processos forem (sistema, segurança, serviço, o próprio G2 Boost); `procs` traz só os que podem ser encerrados (pid + horário de criação em texto).

Ações por linha, todas pelos fluxos que já existiam:

- **Ver no Gerenciador de tarefas**: troca para a aba Processos com o filtro preenchido (`ProcsTab query`).
- **Encerrar**: confirmação, depois `procs_end` em cada processo; o core confere de novo (pid + criação, sessão 0, crítico, System32, segurança).
- **Tirar da inicialização** (sinergia RAM ↔ Inicialização): aparece quando o `exe` de um item ligado da Inicialização é o mesmo do grupo; `startup_set(..., false)` com desfazer no toast. Itens de todos os usuários pedem admin.

A aba **Inicialização** usa a mesma lista no cartão "Quem usa mais RAM agora" e mostra "usa X agora" ao lado de cada item que está rodando.

## Ícones reais

- `StartupItem.exe`: programa do comando (`program_of`: aspas ou até o `.exe` que fecha o token, `%VARS%` expandidas, nome solto = System32) ou alvo do `.lnk`, só se existir (`canonicalize`, caminho longo). Sem programa conhecido: inicial do nome.
- `exe_icon` (shell) chama `icons::exe_data_url`: `local_exe` aceita só `X:\…\*.exe` em unidade local (recusa UNC, `\\?\`, `\\.\`, `/`, unidade mapeada de rede, link simbólico/ponto de reparse), extrai o ícone do recurso; exe em `WindowsApps\<pacote>` sem ícone usa o logo do pacote (`apps::package_logo`, `FindPackageByUserSecurityIdPackageFullName`, nome validado). Nada é executado; cache por caminho no shell e na UI.

## MCP

`get_memory` (só leitura): composição, conselho e os 12 apps com mais memória privada por **nome de arquivo** (sem caminho, sem nome de usuário), contagem de processos, privada, conjunto de trabalho, se é componente do Windows e se inicia com o Windows.

## Testes

- Rust: `memory::tests` (aritmética e conselho, composição real somando, agrupamento por caminho, proteção do grupo, `created` como texto), `boost::tests::purge_reports_the_cache_that_became_free`, `startup::tests::program_of_a_command_line`, `icons::tests::exe_paths_from_outside_are_local_files_only` (UNC, device, mapeada, relativo, não-exe) e `largest_exe_never_crosses_a_link` (sandbox com junção real e symlink quando o privilégio deixa).
- TS: `tests/ram-apps.test.ts` (pouco usados e grandes, grupos de sobras, inicialização por exe).
- e2e: `e2e/ram.spec.ts` (sem backend: estado vazio honesto; backend falso: conselho, barra, ícone de reserva, tirar da inicialização, antes/depois, abrir no gerenciador filtrado; Inicialização com "usa X agora"; reduced-motion).

**Não verificado na máquina real ainda:** purge elevado com os números novos, contador da memória comprimida com a compressão ligada (aqui está desligada), logo de pacote da Store num exe sem ícone.
