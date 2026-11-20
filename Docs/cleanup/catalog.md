# Limpeza — catálogo, prévia, execução, histórico

**Última revisão:** 2026-11-20 (rodada 11: Limpeza inteligente na frente, abas viram Modo detalhado com uma seleção só para catálogo e Arquivos do sistema; revisão do catálogo: `driver_installers`, `adobe_media_cache`, `LiveKernelReports` em `dumps`; ver [smart-clean.md](./smart-clean.md)) · 2026-11-15 (temporários com menos de 24 h ficam) · 2026-11-14 (progresso ao vivo, Parar, componentes do Windows esperados) · 2026-10-22 (aba Arquivos do sistema) · Código: `crates/core/src/cleanup.rs`, `tasksched.rs`, `safety.rs` · UI: `src/pages/cleanup.tsx` · Origem: [legacy-bat-scripts.md](../references/legacy-bat-scripts.md).

## Fluxo

A página abre na **Limpeza inteligente** ([smart-clean.md](./smart-clean.md)): a análise é a prévia de todas as fontes, e o botão Limpar segue o passo 2 e 3 abaixo pelos comandos `smart_*` (catálogo e handlers numa fila só). O **Modo detalhado** (as abas) usa os mesmos comandos com a seleção da pessoa. Os comandos `cleanup_preview/confirm/execute` continuam para o Painel e para quem os chama.


1. **Prévia** (`cleanup_preview`): para cada categoria selecionada, resolve os caminhos (variáveis de ambiente, perfis do Chromium, pacotes do Spotify), valida cada raiz com `SafePath` (absoluta, sem `..`, sem UNC, fora de `System32`/`WinSxS`/perfis de outros usuários, não é reparse point) e **mede** bytes/arquivos sem apagar nada. Lixeira via `SHQueryRecycleBinW`. DNS e DISM não têm medida ("sem medição"). Evento `cleanup:measure` (`cleanup::Progress`) no início de cada categoria e a cada 150 ms dentro dela.
2. **Confirmação humana** (`cleanup_confirm`): a UI mostra o painel com tamanho total, itens de risco alto em vermelho e o que precisa de admin; ao clicar "Apagar agora" o core emite um `ConfirmToken` (aleatório, uso único, 2 min, amarrado à lista exata de ids).
3. **Execução** (`cleanup_execute`): `execute(ids, "manual")` só com token válido (`bad_token` caso contrário). Apaga arquivos com `remove_file`, depois subpastas vazias (bottom-up), **nunca** a raiz da categoria; reparse points (junctions/symlinks) não são seguidos nem removidos; arquivo em uso = pulado e contado. Progresso por evento `cleanup:progress` = `cleanup::Progress {done, total, current, bytes, files}`: no início de cada categoria, a cada 150 ms dentro dela (bytes/arquivos já apagados, a pasta grande não congela o contador) e no fim (`current = null`).
   - **Parar** (`cleanup_cancel`): flag checada entre arquivos, na prévia e na execução da página. A categoria em andamento fica com o que já apagou e termina `cancelled`, como as que não começaram; a prévia parada omite o que não terminou de medir. Rotinas e MCP usam flag própria (o Parar da página não as alcança).
   - **Componentes do Windows** (`dism`): `tasksched::run_task_wait` dispara `StartComponentCleanup` e espera enquanto a tarefa tem instância (até 30 min, batimento a cada 500 ms). `LastTaskResult` negativo = erro. Parar ou o limite = `background` (`segue em segundo plano no Windows`), nunca um "pronto" falso.
   - **UI**: `src/components/cleanup/live-panel.tsx` (também na aba Arquivos do sistema): posição N de M, relógio que anda a cada segundo, bytes ao vivo, trilho por bytes (varredura no item sem medida), item atual e há quanto tempo, aviso de item grande após 20 s, fila de categorias com estado, Parar.
   - **Temporários recentes ficam** (`temp_user`, `temp_windows`, 2026-11-15): arquivo criado ou modificado nas últimas 24 h não é medido nem apagado. Antes a limpeza apagava o que programas abertos tinham acabado de gravar (caso real: os arquivos de trabalho de uma sessão do Claude Code em `%TEMP%`).
   - **Rotina agendada** roda a limpeza em thread própria: não segura o laço de 5 s (Modo Jogo, prioridade, monitor de exposição).
4. **Histórico** (`cleanup_history`): `%LOCALAPPDATA%\G2 Boost\history.json`, últimas 100 execuções: data, gatilho (`manual` / `routine` / `game`), duração, por categoria bytes/arquivos/pulados/erro. Sem caminhos, sem PII.

Categorias com `admin` retornam `needs_admin` sem elevação; a UI mostra "Elevar (administrador)" (relança o app via `runas` nesta rota).

## Catálogo

| id | Grupo | Risco | Admin | Caminhos | Ação | Fechar antes |
|---|---|---|---|---|---|---|
| `temp_windows` | diária | baixo | sim | `%SystemRoot%\Temp` | conteúdo, só arquivos com mais de 24 h (criação e modificação) | — |
| `temp_user` | diária | baixo | não | `%TEMP%`, `%TMP%` | conteúdo, só arquivos com mais de 24 h (criação e modificação) | — |
| `wer` | diária | baixo | sim | `%LOCALAPPDATA%\Microsoft\Windows\WER\{ReportArchive,ReportQueue,Temp}`, `%PROGRAMDATA%\Microsoft\Windows\WER\{…}` | conteúdo | — |
| `shaders` | diária | médio | não | NVIDIA `DXCache`, `GLCache`, `NV_Cache` (Local e ProgramData); AMD `DxCache`, `DXCache`, `GLCache`, `VkCache`; `D3DSCache`; Intel `ShaderCache` | conteúdo | jogos (1º load recompila) |
| `thumbs` | diária | baixo | não | `%LOCALAPPDATA%\Microsoft\Windows\Explorer` + `%LOCALAPPDATA%` | só `thumbcache_*.db`, `iconcache_*.db`, `IconCache.db` | Explorer (em uso = pulado) |
| `dns_flush` | diária | baixo | não | — | `DnsFlushResolverCache` (dnsapi, carregado dinamicamente) | — |
| `recycle` | diária | **alto** | não | todas as unidades | `SHEmptyRecycleBinW` silencioso | — |
| `wu_cache` | mensal | médio | sim | `%SystemRoot%\SoftwareDistribution\Download`, `ServiceProfiles\NetworkService\…\DeliveryOptimization\Cache` | conteúdo (em uso = pulado; sem parar serviços) | — |
| `nvidia_downloads` | mensal | baixo | sim | `%PROGRAMDATA%\NVIDIA Corporation\NVIDIA app\UpdateFramework\ota-artifacts`, `…\Downloader` | conteúdo | NVIDIA app |
| `inetcache` | mensal | baixo | não | `%LOCALAPPDATA%\Microsoft\Windows\INetCache` | conteúdo | Edge |
| `chrome_cache` | mensal | baixo | não | `Google\Chrome\User Data\{Default, Profile *, Guest Profile}\{Cache\Cache_Data, Code Cache, GPUCache}` | conteúdo | Chrome |
| `edge_cache` | mensal | baixo | não | idem em `Microsoft\Edge\User Data` | conteúdo | Edge |
| `discord_cache` | mensal | baixo | não | `%APPDATA%\discord\{Cache, Code Cache, GPUCache}` | conteúdo | Discord |
| `spotify_cache` | mensal | baixo | não | `%LOCALAPPDATA%\Spotify\Data`, `Packages\SpotifyAB.SpotifyMusic_*\LocalCache\Spotify\Data` | conteúdo | Spotify |
| `firefox_cache` | mensal | baixo | não | `%LOCALAPPDATA%\Mozilla\Firefox\Profiles\*\cache2` (o perfil fica no Roaming, intocado) | conteúdo | Firefox |
| `brave_cache` | mensal | baixo | não | `BraveSoftware\Brave-Browser\User Data\{Default, Profile *, Guest Profile}\{Cache\Cache_Data, Code Cache, GPUCache}` | conteúdo | Brave |
| `opera_cache` | mensal | baixo | não | `Opera Software\{Opera Stable, Opera GX Stable}\{Cache, Code Cache, GPUCache}` e o mesmo dentro de `Default\` (layout novo) | conteúdo | Opera |
| `vivaldi_cache` | mensal | baixo | não | `Vivaldi\User Data\{Default, Profile *, Guest Profile}\{Cache\Cache_Data, Code Cache, GPUCache}` | conteúdo | Vivaldi |
| `steam_cache` | mensal | baixo | não | `%LOCALAPPDATA%\Steam\htmlcache\{Default, Profile *}\{Cache\Cache_Data, Code Cache, GPUCache}` (a raiz `htmlcache` guarda cookies/login da loja: só as pastas de cache) | conteúdo | Steam |
| `epic_cache` | mensal | baixo | não | `%LOCALAPPDATA%\EpicGamesLauncher\Saved\webcache*` | conteúdo | Epic Games Launcher |
| `battlenet_cache` | mensal | baixo | não | `%PROGRAMDATA%\Battle.net\Agent\data\cache`, `%LOCALAPPDATA%\Battle.net\{Cache, BrowserCache}` | conteúdo (sem permissão = pulado) | Battle.net |
| `ea_cache` | mensal | baixo | não | `%LOCALAPPDATA%\Electronic Arts\EA Desktop\{cache, CEF\BrowserCache}` | conteúdo | EA app |
| `ubisoft_cache` | mensal | baixo | não | `%LOCALAPPDATA%\Ubisoft Game Launcher\cache` | conteúdo | Ubisoft Connect |
| `teams_cache` | mensal | baixo | não | Teams clássico: `%APPDATA%\Microsoft\Teams\{Cache, Code Cache, GPUCache}` | conteúdo | Teams |
| `slack_cache` | mensal | baixo | não | `%APPDATA%\Slack\{Cache, Code Cache, GPUCache}` | conteúdo | Slack |
| `vscode_cache` | mensal | baixo | não | `%APPDATA%\Code\{Cache, CachedData, Code Cache, GPUCache}` | conteúdo | VS Code |
| `driver_installers` | mensal | baixo | não | `%SystemDrive%\AMD`, `%SystemDrive%\NVIDIA` (pastas que os instaladores de driver extraem e deixam) | conteúdo, só arquivos com mais de 24 h; raiz que é junção = pulada | — |
| `adobe_media_cache` | mensal | baixo | não | `%APPDATA%\Adobe\Common\{Media Cache Files, Media Cache, Peak Files}` | conteúdo (recriado ao abrir o projeto) | Premiere Pro, After Effects, Media Encoder, Audition |
| `dev_npm` | avançado | médio | não | `%LOCALAPPDATA%\npm-cache` | conteúdo (baixa de novo no próximo `npm install`) | — |
| `dev_pip` | avançado | médio | não | `%LOCALAPPDATA%\pip\cache` | conteúdo | — |
| `dev_nuget` | avançado | médio | não | `%LOCALAPPDATA%\NuGet\v3-cache` | conteúdo | — |
| `dev_yarn` | avançado | médio | não | `%LOCALAPPDATA%\Yarn\Cache` | conteúdo | — |
| `dism` | mensal | médio | sim | — | roda a tarefa do Windows `\Microsoft\Windows\Servicing\StartComponentCleanup` (sem `/ResetBase`, sem `DISM.exe`) | — |
| `logs_windows` | avançado | médio | sim | `%SystemRoot%\Logs\{CBS, WindowsUpdate, DISM}` | só `*.log`, `*.etl` | — |
| `dumps` | avançado | médio | sim | `%LOCALAPPDATA%\CrashDumps`, `%SystemRoot%\Minidump`, `%SystemRoot%\LiveKernelReports` (despejos de travamento de driver/GPU, às vezes GBs) | conteúdo | — |

Perfis multi-pasta (Chromium, Firefox, Opera, Epic) são resolvidos com `read_dir` (`child_dirs`): só diretórios reais, pasta de perfil que seja junction/symlink é ignorada, e só as subpastas de cache entram na lista; `Cookies`, `Login Data`, `Preferences`, `Local State` nunca. Caches de desenvolvimento são opt-in (baixam de novo). Cargo e Bun ficam de fora (cache em D:).

**Já cobertos, não duplicados:** cache de shaders NVIDIA/AMD/Intel (`shaders`) e o cache do Delivery Optimization (dentro de `wu_cache`, mesma checagem de `SafePath`). Steam `steamapps\shadercache` fica com a Steam.

**Fora, de propósito:** Riot Client (layout incerto), Zoom (sem pasta de cache clara), Teams novo (`MSTeams_8wekyb3d8bbwe`, EBWebView: layout não garantido), `prefetch` (0 MB com SysMain off; só piora; anti-forense), `spool` (`System32\spool\PRINTERS` fica atrás da barreira do `SafePath`; ganho ~0), `memory.dmp` na raiz do Windows (arquivo solto no `%SystemRoot%`, que o `SafePath` bloqueia como raiz — fica para quando houver seleção explícita no treemap).

**Revisão de especialista (2026-11-20).** Conferido contra o que a Limpeza de Disco/Armazenamento do Windows e as ferramentas de limpeza mais usadas oferecem: temporários com folga de 24 h, caches de navegador só nas pastas de cache, caches de launcher e de shader opt-in, Windows Update + Delivery Optimization (já em `wu_cache`), relatórios de erro, miniaturas, Lixeira opt-in e handlers do Windows para o que é do sistema. Faltavam, e entraram: instaladores de driver extraídos na raiz (`C:\AMD`, `C:\NVIDIA`, comuns e grandes em PC de jogo), cache de mídia da Adobe (GBs em quem edita vídeo) e `LiveKernelReports` dentro de `dumps`. Avaliados e deixados de fora: `Service Worker\CacheStorage` do Chromium (dados offline de sites e apps web), cache de arquivos do Office (envios pendentes), `C:\Intel` (pode ter mais que instaladores), `Windows\Installer` e `Package Cache` (quebram reparo e desinstalação), Prefetch. Arquivos grandes do usuário (Downloads antigos) não são categoria: aparecem no balde Revisar da análise e vão pelo Analisador de disco, com seleção explícita.

Bug de precedência dos scripts (`$L+'\x|'+$L+'\y'`) não foi portado: cada caminho é um item da lista.

## Aba "Arquivos do sistema" (2026-10-22)

Quarta aba do Modo detalhado (desde 2026-11-20 na mesma seleção do catálogo: uma prévia e um Limpar para tudo): os handlers da Limpeza de Disco do próprio Windows (Limpeza de atualizações/WinSxS, Windows.old, pacotes de driver antigos, logs de instalação/atualização, `MEMORY.DMP`…), no mesmo fluxo prévia → token → execução → histórico, admin obrigatório. Ids `sys_*`, allowlist e defesa contra handler plantado em [system-files.md](./system-files.md). Onde um handler e o catálogo cobrem o mesmo caminho (temporários, miniaturas, shaders, Delivery Optimization, WER, minidumps, INetCache), **o catálogo vence** e o handler fica fora. Essa aba substitui, para quem quer ir além, o `dism` (que só agenda o `StartComponentCleanup`).

## Rotinas (fase seguinte)

`execute(ids, trigger)` aceita `trigger = "routine"` / `"game"` para o agendador e o gatilho de jogo; o catálogo marca `default_on` para a seleção inicial. Shaders e Lixeira são opt-in nas rotinas (limpar shader a cada hora = recompilar = stutter).

## Testes (`cleanup.rs`)

- Sandbox real em `%TEMP%` com **junction NTFS criada no teste** (`FSCTL_SET_REPARSE_POINT`): mede e apaga o que é da pasta, remove subpastas esvaziadas, mantém a raiz, **não atravessa a junction** (o arquivo-alvo sobrevive).
- Árvore fabricada de perfis: cada padrão multi-perfil (Chromium/Brave/Vivaldi/Steam, Firefox, Opera, Epic) acha só as pastas de cache; apagar por elas preserva `Cookies`, `Login Data`, `Preferences`, `Local State`; perfil que é junction é ignorado e junction dentro do cache não é seguida.
- Medição read-only na máquina (`measure_new_categories`, `#[ignore]`).
- Glob dos top-files (`thumbcache_*.db`, `*.log`, case-insensitive) e arquivo aberto sem compartilhamento = pulado, não erro.
- Raízes fora da allowlist recusadas: `System32\spool`, perfil de outro usuário, UNC, `..`. Todo caminho do catálogo classifica sem cair em área crítica.
- Categorias da rodada 11: instaladores de driver em sandbox (arquivo recente fica, junção na raiz é pulada, junção dentro não é seguida), todo caminho novo classifica fora das áreas críticas, `dumps` inclui `LiveKernelReports`.
- Prévia de todas as categorias sem apagar; token de uso único amarrado à seleção; execução sem admin devolve `needs_admin` por categoria e grava histórico.
- `tasksched`: tarefa `StartComponentCleanup` visível; round-trip de registro/remoção de tarefa de logon como usuário atual (ignored, rodado à mão em 2026-10-14: ok sem admin).

**Não verificado pelo agente:** deleção real nas pastas da máquina do dono (só sandbox), categorias admin, DISM, Lixeira. Testar com o app elevado.
