# Limpeza — catálogo, prévia, execução, histórico

**Última revisão:** 2026-10-14 · Código: `crates/core/src/cleanup.rs`, `tasksched.rs`, `safety.rs` · UI: `src/pages/cleanup.tsx` · Origem: [legacy-bat-scripts.md](../references/legacy-bat-scripts.md).

## Fluxo

1. **Prévia** (`cleanup_preview`): para cada categoria selecionada, resolve os caminhos (variáveis de ambiente, perfis do Chromium, pacotes do Spotify), valida cada raiz com `SafePath` (absoluta, sem `..`, sem UNC, fora de `System32`/`WinSxS`/perfis de outros usuários, não é reparse point) e **mede** bytes/arquivos sem apagar nada. Lixeira via `SHQueryRecycleBinW`. DNS e DISM não têm medida ("sem medição").
2. **Confirmação humana** (`cleanup_confirm`): a UI mostra o painel com tamanho total, itens de risco alto em vermelho e o que precisa de admin; ao clicar "Apagar agora" o core emite um `ConfirmToken` (aleatório, uso único, 2 min, amarrado à lista exata de ids).
3. **Execução** (`cleanup_execute`): `execute(ids, "manual")` só com token válido (`bad_token` caso contrário). Apaga arquivos com `remove_file`, depois subpastas vazias (bottom-up), **nunca** a raiz da categoria; reparse points (junctions/symlinks) não são seguidos nem removidos; arquivo em uso = pulado e contado. Progresso por evento `cleanup:progress`.
4. **Histórico** (`cleanup_history`): `%LOCALAPPDATA%\G2 Boost\history.json`, últimas 100 execuções: data, gatilho (`manual` / `routine` / `game`), duração, por categoria bytes/arquivos/pulados/erro. Sem caminhos, sem PII.

Categorias com `admin` retornam `needs_admin` sem elevação; a UI mostra "Elevar (administrador)" (relança o app via `runas` nesta rota).

## Catálogo

| id | Grupo | Risco | Admin | Caminhos | Ação | Fechar antes |
|---|---|---|---|---|---|---|
| `temp_windows` | diária | baixo | sim | `%SystemRoot%\Temp` | conteúdo | — |
| `temp_user` | diária | baixo | não | `%TEMP%`, `%TMP%` | conteúdo | — |
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
| `dism` | mensal | médio | sim | — | roda a tarefa do Windows `\Microsoft\Windows\Servicing\StartComponentCleanup` (sem `/ResetBase`, sem `DISM.exe`) | — |
| `logs_windows` | avançado | médio | sim | `%SystemRoot%\Logs\{CBS, WindowsUpdate, DISM}` | só `*.log`, `*.etl` | — |
| `dumps` | avançado | médio | sim | `%LOCALAPPDATA%\CrashDumps`, `%SystemRoot%\Minidump` | conteúdo | — |

**Fora, de propósito:** `prefetch` (0 MB com SysMain off; só piora; anti-forense), `spool` (`System32\spool\PRINTERS` fica atrás da barreira do `SafePath`; ganho ~0), `memory.dmp` na raiz do Windows (arquivo solto no `%SystemRoot%`, que o `SafePath` bloqueia como raiz — fica para quando houver seleção explícita no treemap).

Bug de precedência dos scripts (`$L+'\x|'+$L+'\y'`) não foi portado: cada caminho é um item da lista.

## Rotinas (fase seguinte)

`execute(ids, trigger)` aceita `trigger = "routine"` / `"game"` para o agendador e o gatilho de jogo; o catálogo marca `default_on` para a seleção inicial. Shaders e Lixeira são opt-in nas rotinas (limpar shader a cada hora = recompilar = stutter).

## Testes (`cleanup.rs`)

- Sandbox real em `%TEMP%` com **junction NTFS criada no teste** (`FSCTL_SET_REPARSE_POINT`): mede e apaga o que é da pasta, remove subpastas esvaziadas, mantém a raiz, **não atravessa a junction** (o arquivo-alvo sobrevive).
- Glob dos top-files (`thumbcache_*.db`, `*.log`, case-insensitive) e arquivo aberto sem compartilhamento = pulado, não erro.
- Raízes fora da allowlist recusadas: `System32\spool`, perfil de outro usuário, UNC, `..`. Todo caminho do catálogo classifica sem cair em área crítica.
- Prévia de todas as categorias sem apagar; token de uso único amarrado à seleção; execução sem admin devolve `needs_admin` por categoria e grava histórico.
- `tasksched`: tarefa `StartComponentCleanup` visível; round-trip de registro/remoção de tarefa de logon como usuário atual (ignored, rodado à mão em 2026-10-14: ok sem admin).

**Não verificado pelo agente:** deleção real nas pastas da máquina do dono (só sandbox), categorias admin, DISM, Lixeira. Testar com o app elevado.
