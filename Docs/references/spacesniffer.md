# SpaceSniffer 2.1.0.21 — spec funcional (clean-room)

**Última revisão:** 2026-10-14 · Fonte: `SpaceSniffer User Manual.pdf` (30 págs.), `Release Notes.txt`, `Disclaimer.txt` do pacote em `Referencias/Downloads/`.

> **Licença:** "This software must not be decompiled, disassembled, reverse engineered or otherwise modified." Freeware do Uderzo Umberto. Aqui só usamos a **documentação** para listar comportamento; a implementação é do zero (squarified treemap é algoritmo público — Bruls/Huizing/van Wijk 2000; WinDirStat/WizTree fazem o mesmo).

## O que ele faz (a replicar)
- **Scan** de drive/pasta (vários caminhos no diálogo inicial), multi-thread, mostra o treemap **enquanto varre**; título da janela mostra progresso e espaço livre; detector de mudanças no filesystem atualiza a vista em tempo real (desligável).
- **Treemap** com elementos aninhados (pasta contém filhos), elemento "espaço livre" só na raiz, "espaço desconhecido", barra vertical à esquerda indicando a fração do disco visível no zoom atual; animação de zoom; duplo clique = zoom na pasta; teclas Back/Forward/Home; **sub-scan** só da vista com zoom; "DynaSmooth" (detalhe adaptativo); flashing opcional dos elementos durante o scan.
- **Estilos**: cores planas vs **classes de arquivo** (multimídia, arquivos compactados, etc., com cores configuráveis); sombra no elemento selecionado; tooltip com caminho, tamanho lógico/em disco, datas, nº de filhos.
- **Filtro** (campo de texto, Enter) — ver DSL abaixo; filtro "gray-out" (apaga o que não casa em vez de esconder).
- **Tags** (4 cores) em arquivos e pastas para marcar e depois filtrar/excluir; **exportar** relatório com linguagem de script própria (campos: nome, tamanho lógico/disco, data, nível) e preview; **snapshot** (.sns) salva/carrega a varredura; linha de comando (`scan`, `load`, `save`, filtro, export).
- Ações no elemento: abrir no Explorer, **deletar** (diálogo), menu de contexto do shell.
- Mostra arquivos de tamanho zero e ADS (Alternate Data Streams) opcionalmente; trata caminhos longos e arquivos comprimidos pelo SO.

## DSL de filtro (reproduzir fielmente — é o diferencial)
Vários termos separados por `;`. `|` na frente nega o termo.
- Máscara de arquivo: `*.jpg`, `?` e `*` como no shell; `|*.jpg` exclui.
- Máscara de pasta: `\temp` (prefixo `\`), aplica ao caminho; `|\windows` exclui.
- Tamanho: `(disksize|clustersize|filesize|logicalsize|size)[<|>]N[b|kb|mb|gb|tb]` — ex.: `>1mb`, `disksize>500mb`.
- Idade: `(creation|modify|access)[<|>]N[seconds|minutes|hours|days|weeks|months|years]` — ex.: `<3months`, `access>1y`. Padrão: `modify`.
- Atributos: `archive`, `hidden`, `system`, `readonly`, `compressed`… e `nolink`/ADS.
- Tags: `:tag:red`, `:1`..`:4`, `:all`; negação `|:tag:green`.
- Classes: `:class:multimedia` etc. Fuzzy match de palavras-chave (tolerante a abreviação).

## Decisões para a nossa versão
- Layout: `d3-hierarchy` `treemapSquarify` (dependência já conhecida); render em `<canvas>` com LOD (não desenhar retângulos < 2 px), labels só quando cabem.
- Scan em Rust (`jwalk`), árvore enviada à UI em chunks (eventos Tauri) para pintar enquanto varre; tamanho em disco via `GetCompressedFileSizeW` + cluster size (`GetDiskFreeSpaceW`); reparse points não seguidos.
- Snapshot em formato próprio (JSON gz ou SQLite) — não ler `.sns` do SpaceSniffer.
- Extra nosso: painel "arquivos essenciais grandes" (hiberfil, pagefile, WinSxS, vhdx, caches) com explicação e ação segura; integração com o catálogo de limpeza; ação "explicar com Claude" via MCP.
