# Scripts legados → catálogo de limpeza

**Última revisão:** 2026-10-14 · Fonte: `Referencias/Limpador Diario Windows 2026.bat` (v2.1) e `Referencias/Limpador Mensal Windows  2026.bat` (v2.1). Histórico real: `Limpador - Historico.txt` (1 entrada: 2026-07-19 diário, "8,32 GB" — número inflado, ver bug abaixo).

## Como os scripts funcionam
auto-elevação → mede tamanho das pastas com `robocopy /L /S /XJ /BYTES` (truque bom: rápido, ignora junctions) → apaga por categoria → mede de novo → relatório "liberado por categoria" + linha no histórico. O mensal ainda para `wuauserv/bits/dosvc/spooler`, roda `DISM /StartComponentCleanup /ResetBase` e, opcionalmente, tweaks de registro.

## Catálogo de categorias (portar a intenção)
| id | Caminhos | Freq. | Precisa fechar | Risco | Decisão no app |
|---|---|---|---|---|---|
| `temp.windows` | `C:\Windows\Temp\*` | diário | — | baixo | manter; pular arquivos em uso |
| `temp.user` | `%TEMP%\*` | diário | — | baixo | manter |
| `logs.windows` | `Windows\Logs\CBS\*.log`, `Logs\WindowsUpdate\*.etl`, `Logs\DISM\*.log` | diário | — | médio (diagnóstico) | **avançado, off por padrão** (anti-forense + pouco espaço) |
| `dumps` | `%LOCALAPPDATA%\CrashDumps`, `Windows\Minidump`, `Windows\memory.dmp` | diário/mensal | — | médio | avançado, mostrar tamanho; dumps grandes valem a pena |
| `wer` | `%LOCALAPPDATA%\Microsoft\Windows\WER`, `ProgramData\…\WER\ReportArchive\|ReportQueue` | diário | — | baixo | manter via handler "Windows Error Reports" |
| `shaders` | NVIDIA `GLCache\|DXCache\|D3DSCache\|NV_Cache`, AMD `DxCache\|GLCache`, `%LOCALAPPDATA%\D3DSCache`, Intel `ShaderCache` | diário no script | jogos | médio (recompila shaders = 1º load lento) | manter, **sugerir mensal**, avisar |
| `thumbs` | `Explorer\thumbcache_*.db`, `iconcache_*.db`, `IconCache.db` | diário | Explorer | baixo | manter via handler "Thumbnails" |
| `spool` | `System32\spool\PRINTERS\*` | diário | Spooler | baixo | parar/retomar serviço por API |
| `recycle` | `C:\$Recycle.Bin` | diário | — | **alto** (irreversível) | `SHEmptyRecycleBin` com confirmação própria |
| `dns.flush` | — | diário | — | nulo | `DnsFlushResolverCache` |
| `prefetch` | `C:\Windows\Prefetch\*` | mensal | — | **não limpar** | fora do catálogo (perf pior + anti-forense) |
| `wu.cache` | `SoftwareDistribution\Download`, `DeliveryOptimization` | mensal | serviços WU/BITS/DO | médio | handlers "Windows Update Cleanup" + "Delivery Optimization Files" (sem `net stop`) |
| `nvidia.downloads` | `ProgramData\NVIDIA Corporation\NVIDIA app\UpdateFramework\ota-artifacts`, `…\Downloader` | mensal | NVIDIA app | baixo | manter |
| `inetcache` | `%LOCALAPPDATA%\Microsoft\Windows\INetCache` | mensal | Edge/IE | baixo | manter |
| `chrome.cache` | `Chrome\User Data\Default\{Cache\Cache_Data, Code Cache, GPUCache}` | mensal | Chrome | baixo | manter; detectar processo aberto; **todos os perfis**, não só `Default` |
| `edge.cache` | idem Edge + `ExtensionActivityEdge*` | mensal | Edge | baixo | manter |
| `discord.cache` | `%APPDATA%\discord\{Cache, Code Cache, GPUCache}` | mensal | Discord | baixo | manter |
| `spotify.cache` | `Packages\SpotifyAB.SpotifyMusic_*\LocalCache\Spotify\Data` | mensal | Spotify | baixo | manter (Store); cobrir versão desktop também |
| `dism.componentcleanup` | `DismApi` StartComponentCleanup | mensal | — | médio | manter **sem** `/ResetBase` por padrão (ResetBase impede desinstalar updates) — opção avançada |

Candidatos novos (o SpaceSniffer-like vai mostrar): `hiberfil.sys` (desligar hibernação, só com explicação), `pagefile.sys` (não mexer), WinSxS (só via DISM), WSL/Docker `ext4.vhdx` (compactar), caches de IDE (`.gradle`, `.nuget`, `node_modules` órfãos, `%LOCALAPPDATA%\pip`, `cargo\registry`), `Downloads` antigos, `Windows.old`, pontos de restauração.

## Tweaks do mensal (`:ApplyGamerTweaks`) → módulo Boost
| Tweak | Chaves | Veredito |
|---|---|---|
| GameDVR off + Game Mode on | `HKCU\System\GameConfigStore` (`GameDVR_Enabled=0`, FSE…), `HKLM\…\Policies\…\GameDVR\AllowGameDVR=0`, `HKCU\…\GameBar` | manter (preferir HKCU, evitar Policies) |
| HAGS | `HKLM\SYSTEM\CCS\Control\GraphicsDrivers\HwSchMode=2` | manter, "não recomendado se" GPU antiga |
| MMCSS Games | `…\Multimedia\SystemProfile\Tasks\Games` (GPU Priority 8, Priority 6, High) | manter |
| Reserva de banda | `HKLM\…\Policies\…\Psched\NonBestEffortLimit=0` | manter (efeito pequeno) |
| TCP ack/NoDelay/DelAckTicks em todas as interfaces | `Tcpip\Parameters\Interfaces\*` | manter com revert por interface |
| Mouse sem aceleração | `HKCU\Control Panel\Mouse` MouseSpeed/Threshold = 0 | manter |
| Plano Ultimate Performance | `powercfg -duplicatescheme e9a42b02…` | manter via `PowerDuplicateScheme`/`PowerSetActiveScheme` |
| "Desativar Nagle" | `HKLM\SOFTWARE\Microsoft\MSMQ\Parameters\TCPNoDelay` | **descartar** — só afeta MSMQ (cargo cult) |
| Dicas/sugestões off | `ContentDeliveryManager` | manter |

## Lições já aprendidas pelos scripts (não repetir)
- v2.0: WMIC sumiu no 24H2 → medição via robocopy. v2.0 removeu "liberar working set" (**piorava** desempenho). v2.1: Prefetch saiu do diário (deixa apps/jogos mais lentos).
- Relatório mede antes/depois com diferença, então arquivos criados durante a limpeza distorcem — no app, somar bytes realmente apagados.
- `del /s` com `>nul 2>&1` engole erro de arquivo em uso — no app, contar "pulados" e mostrar.

## Bug: precedência vírgula × `+`
`@('nome',$L+'\x|'+$L+'\y')` → `@('nome',$L,'\x|',$L,'\y')` (teste 2026-10-14: `count=5`, `e1=$L`). Categorias com `$L+`/`$R+` medem `AppData\Local`/`Roaming` inteiras (64 GB / 7 GB no `.tmp` de referência); "Logs" perde os dumps. Explica medição lenta e totais inflados. Correção nos scripts: parênteses — `@('nome',($L+'\x'))`.
