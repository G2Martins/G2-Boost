# Limpeza inteligente (análise, três baldes, sinergia com o disco)

**Última revisão:** 2026-11-20 (rodada 11, bloco A2) · Código: `crates/core/src/smartclean.rs` · Shell: `src-tauri/src/clean_cmds.rs` · UI: `src/pages/cleanup.tsx`, `src/components/cleanup/{smart-view,detailed,quick-clean,live-panel}.tsx`, `src/components/disk/intent.ts`, `src/pages/disk.tsx` · MCP: `get_cleanup_plan` · Catálogo: [catalog.md](./catalog.md) · Arquivos do sistema: [system-files.md](./system-files.md) · Disco: [../disk/analyzer.md](../disk/analyzer.md)

## O que mudou para quem usa

A página Limpeza abre na visão **Inteligente**: um botão **Analisar**, a análise mostra cada fonte sendo medida ao vivo e devolve três baldes com o total em destaque. Um botão **Limpar** apaga o balde Seguro (mais o balde Sistema quando o app está como administrador), com confirmação. As abas antigas (Diária, Mensal, Avançado, Arquivos do sistema) continuam como **Modo detalhado**, a um clique no cabeçalho.

## Fontes da análise (`smartclean::analyze`)

Duas threads (lado catálogo e lado apps), eventos `smart:source` (`SourceEvent {source, state, bytes, count, current}`) para a UI desenhar a fila:

| Fonte | Como mede | Vai para |
|---|---|---|
| `catalog` | `cleanup::preview_live` em todas as categorias (`current` = categoria em andamento) | Seguro / Sistema / Revisar pela regra abaixo |
| `system` | `volcache::preview` só dos handlers ligados por padrão (os de aviso ficam no Modo detalhado). Sem admin: itens `needs_admin`, nada carregado; vaga de servicing ocupada (`busy`): pulado | Sistema |
| `big_old` | caminhada de até 4 s nas pastas do usuário **no disco do sistema** (Downloads, Área de Trabalho, Documentos, Vídeos, Imagens, Música via `SHGetKnownFolderPath`), filtro do disco `>500mb >6m` (`disk::parse_filter`), sem atravessar reparse point, sem contar arquivo de nuvem só online (`OFFLINE`/`RECALL_ON_*`) | Revisar |
| `unowned` | `apps::unowned(instalados)`; dados do usuário (`precious`) não contam como espaço a liberar | Revisar |
| `leftovers` | `apps::orphans` + `apps::leftovers`, sem contar de novo o que já é pasta sem dono | Revisar |
| `dupes` | resultado da última busca de duplicados do Analisador de disco (não roda uma nova) | Revisar |

Também medido: `extras` (o que a limpeza nunca toca, com o jeito seguro de reduzir): `hiberfil.sys`, `pagefile.sys`, `swapfile.sys` (tamanho pela listagem da raiz), jogos da Steam no disco do sistema (`steam::installed`, `SizeOnDisk`), pontos de restauração (contagem; tamanho exige admin). E `tight` = disco do sistema com menos de 15 % ou menos de 20 GB livres.

Na máquina do dono (2026-11-20, sem admin): análise completa em ~25 s, 8,6 GB no catálogo, 5,5 GB em pastas sem dono.

## Regra dos baldes (`smartclean::bucket`, pura e testada)

- **Seguro**: `default_on` e risco diferente de alto, sem admin.
- **Sistema**: a mesma regra com admin, mais os handlers do Windows ligados por padrão.
- **Revisar**: tudo o que é opt-in ou de risco alto (Lixeira, shaders, caches de desenvolvimento, DISM, logs, dumps) e as fontes de fora do catálogo.

Categoria que não existe na máquina ou mediu 0 bytes não é oferecida (o flush de DNS, sem medida, é). Revisar fica em ordem de tamanho. **Nada de Revisar entra no botão Limpar**: cada item leva ao fluxo que já existe, com a própria confirmação:

| Item | Ação |
|---|---|
| categoria opt-in | **Limpar só isso** (`QuickClean`: prévia, confirmação, token, histórico); precisa de admin = Elevar |
| arquivos grandes e antigos | **Ver no disco**: abre o Analisador na aba Maiores já filtrada (`>500mb >6m`) e, sem varredura na memória, começa a do disco do sistema (`openDisk`) |
| pastas sem dono | **Revisar em Apps** (`boot.startTab = "unowned"`: aba Sobras com a varredura de todas as unidades) |
| sobras de apps removidos | **Revisar em Apps** (`openOrphans`) |
| duplicados | **Ver duplicados** (aba do Analisador) |

## Execução (`smart_preview` / `smart_confirm` / `smart_execute`)

`smartclean::split_ids` valida a seleção (não vazia, até 64, sem repetição, cada id é do catálogo ou da allowlist de handlers) e separa em catálogo e handlers. A prévia e a execução rodam o catálogo (`cleanup::preview_live` / `cleanup::execute`) e depois os handlers (`volcache::preview` / `volcache::execute`) como **uma fila** (`smartclean::joined`): eventos `smart:measure` e `smart:progress` com `done/total/current/bytes/files` contínuos. O token é o mesmo `TokenStore` (uso único, 2 min, amarrado à lista exata) e só é emitido quando a pessoa clica em "Apagar agora". Cada perna grava o próprio registro no histórico. `smart_cancel` para as duas (flag da página + `repair::cancel`). O resultado traz o disco do sistema antes e depois e `tight`.

Execução manual com 15 s ou mais vira linha na central de atividade (`activity.clean.done`, rota `cleanup`). O Modo detalhado usa os mesmos comandos: uma seleção só para catálogo e Arquivos do sistema, e a máquina de estados duplicada da aba de sistema foi removida.

## Progresso honesto e o disco "ganhando espaço"

`DriveVessel` desenha o disco do sistema como uma barra: usado | para liberar (hachurado no accent) | livre (tom de sucesso com borda acesa). Na análise o hachurado cresce com o que as fontes encontram; na limpeza ele escoa para o livre **pelos bytes realmente apagados** (`progress.bytes`), com CountUp do espaço livre. Ao lado, o `LivePanel`: posição N de M, relógio, bytes, item atual, aviso de item longo, fila de categorias **riscadas** ao concluir e Parar. No fim: liberados, livre antes e depois (e o ganho quando o disco cresceu), detalhes por item e o **próximo passo**: disco ainda apertado = "Ver o que mais ocupa" (Analisador filtrado) e os grandes que a limpeza não toca (paginação, hibernação, pontos de restauração com o texto dos Essenciais; jogos no disco do sistema levam a Jogos); senão, agendar a rotina. Com `prefers-reduced-motion: reduce` as larguras e os números mudam sem transição (e2e verifica).

## Sinergia com o Analisador de disco

- `clean_owners(ids)`: para a pasta na tela e as subpastas diretas, quais categorias do catálogo **são donas** (a pasta é a raiz da categoria ou está dentro dela) ou **estão dentro** dela, e quais a última análise achou **sem dono**. Comparação léxica, minúscula, contra as raízes reais (`catalog_roots`: só categorias que esvaziam pastas; raiz que não existe ou é reparse point fica de fora; `canonicalize` resolve o caminho curto 8.3 do `%TEMP%`). O caminho passa por `SafePath` antes: relativo, `..`, UNC, `System32`/`WinSxS`, `C:\Windows` em si e perfil de outro usuário não recebem selo.
- No mapa: ponto verde no canto da subpasta e a frase no hover; faixa abaixo do caminho com **Limpar só isso** / **Limpar estes** (mesmo `QuickClean`) e **Revisar em Apps** para pastas sem dono. O mapa não muda de tamanho depois (diz isso: "atualiza na próxima análise").
- Aba Maiores: `clean_owner_paths` põe o selo "Limpeza: categoria" no arquivo que está numa pasta do catálogo. Essenciais: Downloads do Windows Update e Lixeira ganham **Limpar só isso**.
- Varredura de disco com 15 s ou mais vira linha na central de atividade (`activity.clean.scan_done`).
- Ícone real do app em cada categoria (Chrome, Edge, Discord, Steam...) por `smartclean::app_exes` (caminhos conhecidos, só o que existe) + `ExeIcon`; sem o app, a inicial num quadrado.

## MCP

`get_cleanup_plan` (só leitura): o mesmo `Plan` com `for_mcp()` (sem a lista de arquivos grandes, que traz caminhos do perfil). Sem duplicados (o MCP roda em outro processo, sem a varredura do disco).

## Testes

`smartclean.rs`: regra dos baldes (todo opt-in/alto fica em Revisar), ordenação e soma sem contar duas vezes (vazio e ausente fora, flush fica, handler sem admin oferecido), `split_ids` contra entrada hostil (vazio, repetido, `../`, nome de chave do registro, 65 ids), fila de duas pernas, selos (léxico nos dois sentidos, prefixo de nome não é pasta, caminhos hostis sem selo), raízes reais (minúsculas, sem `\\?\`, sem categoria de arquivos soltos), caminhada de arquivos grandes em sandbox (recente, pequeno e atrás de junção não contam; prazo vencido e cancelamento), `tight`, ícones só de apps que existem. `analyze_live` (ignorado) mede a máquina. e2e: `e2e/cleanup.spec.ts` (estado vazio honesto fora do app, Modo detalhado, aba de sistema, movimento reduzido) e `e2e/smart-clean.spec.ts` (plano fabricado: baldes, nenhum token antes da confirmação, ids e token enviados, próximo passo com disco apertado; fila de fontes ao vivo).

**Não verificado pelo agente:** a execução real no app (só sandbox e bridge falsa), o balde Sistema elevado, a busca de handlers ocupada por um reparo, o selo de pasta sem dono no mapa depois de uma análise real.
