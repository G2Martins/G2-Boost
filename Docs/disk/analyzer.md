# Analisador de disco

**Última revisão:** 2026-10-16 · Código: `crates/core/src/disk.rs`, `safety.rs`; shell `src-tauri/src/lib.rs` (`DiskState`, `disk_*`); UI `src/pages/disk.tsx`, `src/components/disk/treemap.tsx` · Spec funcional de origem: [../references/spacesniffer.md](../references/spacesniffer.md) (clean-room).

## Varredura

`disk::scan` percorre a raiz em paralelo (rayon), **nunca segue reparse points** (junções/symlinks — evita contar duas vezes e laços como `All Users`), conta erros de permissão sem parar, aceita cancelamento (devolve a árvore parcial) e recusa raiz relativa ou UNC. Um `Node` por arquivo (~150 MB para 1,4 M arquivos; C: em ~64 s sem admin). A árvore fica no shell (`Arc<Node>`); a UI pede recortes (`disk_tree(ids, depth, max_children)`), identificados por **caminho de índices** (`ids`), estáveis durante uma varredura.

## Treemap

Squarified (`d3-hierarchy`), canvas próprio, cor por classe de arquivo, duplo clique entra na pasta, breadcrumb. Na barra: **Mostrar na pasta** (pasta atual) e **Lixeira** (pasta atual, com confirmação).

## Arquivos e filtros

`disk::search(root, filtro, agora, n)` percorre a árvore uma vez: total de arquivos/bytes que casam + os `n` maiores (min-heap; caminho montado só para candidatos). Sintaxe (`parse_filter`, termos combinados com E; extensões com OU):

| Termo | Significado |
| --- | --- |
| `*.mp4` ou `.mp4` | extensão |
| `>1gb`, `<200mb` (b, kb, mb, gb, tb) | tamanho |
| `>6m`, `<7d` (d, w, m = meses, y) | sem modificação há mais de / modificado nos últimos (data desconhecida nunca casa) |
| qualquer palavra | nome contém |

Botões de exemplo na UI; resultado com caixa de seleção, **mostrar na pasta** por linha e **Enviar N para a Lixeira**.

## Lixeira (`disk::recycle`)

1. Seleção vem como `ids` (nunca caminho cru da UI); entradas aninhadas em outra selecionada são descartadas; no máximo 500.
2. Para cada item: caminho reconstruído da árvore, `symlink_metadata` (existe, **não é reparse point**) e `recyclable()`: `SafePath::classify` precisa dar **perfil do usuário** ou **outro local** (bloqueia `C:\Windows`, System32/WinSxS, ProgramData, perfis de outros usuários, UNC, `..`) e o caminho não pode ser raiz de disco, `C:\Users`, `Program Files`/`(x86)`, a raiz do perfil nem `AppData`/`Local`/`Roaming`/`LocalLow` em si (o conteúdo abaixo pode).
3. `SHFileOperationW(FO_DELETE, FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_WANTNUKEWARNING)`: vai para a Lixeira; **se algo não couber nela (ou o drive não tiver Lixeira), o próprio Windows pergunta antes de apagar de vez**. Arquivo em uso/cancelado = fica e é contado como falha.
4. O que sumiu é removido da árvore (tamanhos dos ancestrais atualizados) e registrado no histórico de limpeza (`trigger: disk_recycle`). A UI confirma antes (lista + total) e recarrega o recorte.

No shell, a árvore é retirada do estado durante a operação (os outros comandos não esperam um diálogo do Windows) e recusada durante uma varredura.

## Mostrar na pasta (`disk::reveal`)

`SHParseDisplayName` + `SHOpenFolderAndSelectItems` (COM no thread) — o Explorer abre com o item selecionado, sem spawn de `explorer.exe`.

## Essenciais grandes (`disk::essentials`)

Itens conhecidos relativos à raiz varrida — `pagefile.sys`, `hiberfil.sys`, `swapfile.sys`, `Windows\WinSxS`, `Windows.old`, `$Recycle.Bin`, `System Volume Information`, `Windows\SoftwareDistribution\Download`, `Windows\Installer`, `ProgramData\Package Cache` — com tamanho (0 = sem permissão para medir) e explicação i18n do que é e do **jeito seguro** de reduzir (desligar hibernação nas Opções de energia, limpeza de componentes, Windows Update cache, desinstalar o programa dono…). Nada aqui apaga; são ponteiros.

## Testes

Sandbox em pasta temporária: tamanhos, ordenação, recorte com "outros", maiores arquivos, cancelamento, raiz inválida; sintaxe do filtro; busca + caminho por `ids` + remoção com totais; guarda de reciclagem (permitidos × bloqueados, incluindo UNC e `..`); recusa da raiz e de `ids` inexistentes; essenciais por caminho relativo. `recycle_live` (ignorado, rodar à mão) leva dois arquivos descartáveis para a Lixeira de verdade — verificado em 2026-10-16.

## Pendente

MFT direto quando elevado (varredura muito mais rápida), snapshots/comparação, tamanho em disco × lógico (compressão/clusters), filtro por pasta (`\temp`).
