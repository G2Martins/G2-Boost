# Kaspersky × "Limpador Diario/Mensal Windows 2026.bat" — análise do alerta

**Última revisão:** 2026-10-14 · Analisado: `Referencias/Limpador Diario Windows 2026.bat` (v2.1, 9.3 KB) e `Referencias/Limpador Mensal Windows  2026.bat` (v2.1, 20.3 KB). Kaspersky instalado na máquina: 21.26 (linha 2026).

## Verdict confirmado (relatório exportado do Kaspersky, 2026-10-14)
`PDM:Trojan.Win32.Generic` · severidade Alto · tecnologia **"Análise de comportamento"** (System Watcher; PDM = Proactive Defense Module) · objeto `cmd.exe` PID 16180 iniciado por `Limpador Mensal Windows  2026.bat` em `C:\Users\Windows\Desktop` · 13/10/2026 23:45:03 detectado → processo encerrado → cópia de backup → 23:45:29 **arquivo excluído**. Segunda tentativa 23:45:42 (PID 13216): detectado e encerrado de novo, "não pode ser excluído". Ou seja: não foi assinatura de arquivo, foi a **sequência de ações em execução** (elevação → PowerShell → deleção em massa → `net stop` de serviços) que bateu no limiar comportamental. O `.bat` do Desktop foi removido pelo AV; a cópia em `Referencias/` está intacta. Recuperar: Kaspersky → Quarentena → restaurar + adicionar às exclusões, ou usar a cópia.

## Veredito
**Não há código malicioso nos scripts.** Tudo que eles fazem é o que anunciam (limpar caches/logs, parar serviços para liberar arquivos, aplicar tweaks de registro). O alerta é **heurístico/comportamental**: o conjunto de padrões abaixo é o mesmo de droppers e wipers em `.bat`, e o Kaspersky não tem como saber a intenção. Verdict confirmado acima.

## Gatilhos encontrados (com a linha)
| # | Padrão | Onde | Por que pontua |
|---|---|---|---|
| 1 | Auto-elevação: `powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs"` | ambos, bloco "Auto-elevacao" | script que relança **a si mesmo** elevado via PowerShell = assinatura clássica de dropper (UAC prompt automático + LOLBin). |
| 2 | `powershell … -ExecutionPolicy Bypass -Command "<1.900–2.600 chars em uma linha>"` | medição antes/depois | `Bypass` + comando gigante em uma linha = padrão de script ofuscado (`Powecod`-like). Detalhe: com `-Command` inline a execution policy **nem se aplica** — o `Bypass` é supérfluo e só chama atenção. |
| 3 | `del /s /f /q` em `C:\Windows\Prefetch`, `C:\Windows\Logs\CBS`, `Minidump`, `memory.dmp`, `WER\ReportArchive/ReportQueue`, `CrashDumps` | mensal etapas 3 e 5; diário etapa 2 | apagar Prefetch, logs e dumps é **anti-forense** (apaga evidência de execução). Verdicts tipo `Trojan.BAT.KillFiles` / `Wiper` pontuam exatamente isso. |
| 4 | `net stop wuauserv`, `net stop bits`, `net stop dosvc` | mensal etapa 1 | desligar Windows Update/BITS = comportamento de malware que impede patch (`RiskTool/Disabler`). |
| 5 | `reg add "HKLM\SOFTWARE\Policies\Microsoft\Windows\GameDVR"`, `…\Policies\…\Psched`, `HKLM\SYSTEM\CurrentControlSet\Control\GraphicsDrivers`, loop `reg query … Tcpip\Parameters\Interfaces` + `reg add` em todas | sub-rotina `:ApplyGamerTweaks` | escrita em **Policies** e em parâmetros TCP de todas as interfaces = "tampering" de política/rede. |
| 6 | `Clear-RecycleBin -Force`, `rmdir /s /q` em `%LOCALAPPDATA%\Microsoft\Windows\WER` e `ProgramData\…\WER` | ambos | deleção recursiva silenciosa em massa. |
| 7 | `rd /s /q` nos caches de Chrome/Edge/Discord/Spotify + `del` de `ExtensionActivityEdge*` | mensal 8–10 | apagar dados de navegador/log de extensão também lê como anti-forense. |
| 8 | `@echo off` + `color 0a` + `title` + `>nul 2>&1` em toda linha | ambos | cosmético, mas o "tudo silenciado" soma na heurística. |

O **Proteção contra ameaças de arquivo** pontua o texto (1, 2, 3, 4); o **System Watcher (PDM)** pontua a execução (elevação → PowerShell → deleção em massa → serviços parados). O `limpador_dados.tmp` que sobrou na pasta (o script apaga ele no fim) sugere que uma execução do mensal **não chegou ao fim** — compatível com o Kaspersky encerrando o processo no meio.

## Verdict
`PDM:Trojan.Win32.Generic` (comportamental) — confirmado pelo relatório. Nenhuma detecção estática (`HEUR:*`): o arquivo em si passou; a execução não. Consequência para o G2 Boost: o que o System Watcher pontua é a cadeia de comportamento, logo **um exe assinado que faz as mesmas deleções em massa + para serviços também pode cair** — usar os handlers do Disk Cleanup e APIs oficiais, e nunca parar `wuauserv`/`bits` por conta própria.

## Como lidar com os scripts (curto prazo)
1. Confirmar o verdict e, se for só heurístico, **adicionar à lista de confiáveis** (Kaspersky → Configurações → Exclusões e ações em objetos detectados).
2. Reportar falso positivo em `opentip.kaspersky.com` (upload do .bat) — resposta costuma vir em dias.
3. Reduzir os gatilhos sem perder função: tirar `-ExecutionPolicy Bypass` (inútil com `-Command`); trocar a auto-elevação por um atalho "Executar como administrador" ou tarefa agendada com privilégio máximo; mover os one-liners para um `.ps1` ao lado; **parar de limpar Prefetch** (também piora desempenho); não apagar `CBS.log`/minidumps (valor forense e de diagnóstico, ganho de espaço pequeno).

## O que isso impõe ao app novo
- **Zero spawn de `cmd`/`powershell`/`reg`/`net`.** Tudo por API Win32 (`DeleteFileW`, `IFileOperation`, `RegSetValueExW`, `ControlService`, `DnsFlushResolverCache`, `DismApi`). Um exe que chama `powershell -Command` com string montada é o mesmo padrão em roupa nova.
- **Elevação por manifesto/UAC**, nunca auto-relançamento.
- **Catálogo sem anti-forense**: Prefetch fora; logs/dumps como categoria "avançado", desligada por padrão, com explicação.
- **Serviços**: em vez de `net stop wuauserv` + `del SoftwareDistribution`, usar os handlers oficiais do Disk Cleanup (`IEmptyVolumeCache2` via `HKLM\…\Explorer\VolumeCaches`: "Windows Update Cleanup", "Delivery Optimization Files", "Temporary Files", "DirectX Shader Cache", "Windows Error Reports") — é o que a Microsoft sanciona e o que o AV reconhece.
- **Assinatura de código** + VirusTotal + submissão prévia aos fornecedores antes de distribuir; teste na própria máquina com Kaspersky 21.26 como gate de release.
- Registro: backup `.reg` + reverter; nada em `Policies` sem necessidade (GameDVR tem equivalente em `HKCU\System\GameConfigStore`).

## Bug paralelo encontrado (não é segurança, mas invalida o relatório)
`$m+=,@('Cache de shaders',$L+'\NVIDIA\GLCache|'+…)` — em PowerShell a vírgula tem precedência maior que `+`, logo o elemento vira `@('Cache de shaders', $L, '\NVIDIA\GLCache|', $L, …)` e `$e[1]` é só `$L` (`AppData\Local`). O `limpador_dados.tmp` prova: shaders/thumbnails/Chrome/Edge/Spotify = 64.420.406.808 bytes (a pasta Local inteira), Discord = `Roaming`, "Logs" perde os dumps. Efeitos: números de "liberado" duplicados/inflados e a etapa "medindo" varre 64 GB várias vezes. Correção trivial: parênteses `@('nome',($L+'\x'))`. Confirmado com teste em 2026-10-14.
