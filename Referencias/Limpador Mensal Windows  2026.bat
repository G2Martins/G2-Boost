@echo off
setlocal
title Limpador Windows Mensal 2026
color 0a
mode con: cols=100 lines=45 >nul 2>&1

REM ============================================================
REM  LIMPADOR WINDOWS MENSAL 2026 - v2.1
REM  Limpeza profunda + otimizacao gamer. Rodar 1x por mes ou
REM  apos grandes updates do Windows.
REM
REM  v2.1: medicao via ROBOCOPY (muito mais rapida) e progresso
REM        na tela categoria por categoria - nunca mais parece
REM        "travado". Aviso claro na etapa DISM (a mais lenta).
REM  v2.0: - Corrigido relatorio ZERADO (WMIC removido do
REM          Windows 11 24H2+ era a causa)
REM        - Relatorio REAL por categoria (KB / MB / GB) com
REM          linha propria para o ganho do DISM
REM        - Auto-elevacao (duplo clique direto)
REM        - Limpa cache do Chrome, Edge, Discord, Spotify e
REM          drivers NVIDIA + poda log de extensao do Edge
REM        - Historico em "Limpador - Historico.txt"
REM        - Removido "liberar working set" (piorava performance)
REM ============================================================

REM ----- Auto-elevacao (pede Administrador sozinho) -----
net session >nul 2>&1
if %errorLevel% NEQ 0 (
    echo.
    echo  Solicitando permissao de Administrador...
    powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs"
    exit /b
)

set "DATAF=%~dp0limpador_dados.tmp"
set "LOGF=%~dp0Limpador - Historico.txt"

cls
echo ===============================================================
echo           LIMPADOR WINDOWS MENSAL 2026  -  v2.1
echo ===============================================================
echo.
echo  Este script ira:
echo    - Limpar temporarios, caches, logs e crash dumps
echo    - Limpar Prefetch (1x por mes, sem prejudicar o dia a dia)
echo    - Limpar cache do Windows Update + Delivery Optimization
echo    - Limpar cache de shaders NVIDIA / AMD / Intel / DirectX
echo    - Limpar cache do Chrome, Edge, Discord e Spotify
echo      (feche-os antes para limpar tudo!)
echo    - Limpar cache de download de drivers NVIDIA
echo    - Esvaziar a Lixeira
echo    - Rodar DISM /ResetBase (libera GBs de updates antigos)
echo    - Aplicar tweaks gamer no registro (opcional)
echo.
echo  Tempo estimado: 5 a 20 minutos (DISM e a parte mais lenta).
echo  NAO FECHE A JANELA no meio - o resumo aparece no final.
echo  Recomendado: feche jogos, navegadores e Discord antes.
echo ===============================================================
echo.
choice /C SN /N /M "Deseja continuar? (S/N): "
if errorlevel 2 (
    echo Operacao cancelada pelo usuario.
    pause
    exit /b 0
)

echo.
choice /C SN /N /M "Aplicar tambem os TWEAKS GAMER (registro)? (S=sim / N=so limpeza): "
if errorlevel 2 (
    set "APLICAR_TWEAKS=N"
) else (
    set "APLICAR_TWEAKS=S"
)

echo.
echo  Medindo tamanho das pastas ANTES da limpeza...
powershell -NoProfile -ExecutionPolicy Bypass -Command "function SizeOf([string]$d){ if(-not (Test-Path -LiteralPath $d)){ return [long]0 }; if(Test-Path -LiteralPath $d -PathType Leaf){ return [long](Get-Item -LiteralPath $d -Force).Length }; $o=robocopy $d NULL /L /S /XJ /BYTES /NFL /NDL /NJH /NP /R:0 /W:0; foreach($ln in $o){ if($ln -match '^\s*Bytes\s*:\s*(\d+)'){ return [long]$matches[1] } }; return [long]0 }; $L=$env:LOCALAPPDATA; $R=$env:APPDATA; $m=@(); $m+=,@('Temp do Windows','C:\Windows\Temp'); $m+=,@('Temp do usuario',$env:TEMP); $m+=,@('Prefetch','C:\Windows\Prefetch'); $m+=,@('Cache do Windows Update','C:\Windows\SoftwareDistribution\Download|C:\Windows\SoftwareDistribution\DeliveryOptimization'); $m+=,@('Logs e crash dumps','C:\Windows\Logs\CBS|C:\Windows\Logs\WindowsUpdate|C:\Windows\Logs\DISM|'+$L+'\CrashDumps|C:\Windows\Minidump|C:\Windows\memory.dmp|'+$L+'\Microsoft\Windows\WER|C:\ProgramData\Microsoft\Windows\WER\ReportArchive|C:\ProgramData\Microsoft\Windows\WER\ReportQueue'); $m+=,@('Cache de shaders',$L+'\NVIDIA\GLCache|'+$L+'\NVIDIA\DXCache|'+$L+'\NVIDIA\D3DSCache|'+$L+'\NVIDIA Corporation\NV_Cache|'+$L+'\AMD\DxCache|'+$L+'\AMD\GLCache|'+$L+'\D3DSCache|'+$L+'\Intel\ShaderCache'); $m+=,@('Cache de drivers NVIDIA','C:\ProgramData\NVIDIA Corporation\NVIDIA app\UpdateFramework\ota-artifacts|C:\ProgramData\NVIDIA Corporation\Downloader'); $m+=,@('Thumbnails e cache IE/Edge',$L+'\Microsoft\Windows\Explorer|'+$L+'\Microsoft\Windows\INetCache'); $m+=,@('Cache do Chrome',$L+'\Google\Chrome\User Data\Default\Cache\Cache_Data|'+$L+'\Google\Chrome\User Data\Default\Code Cache|'+$L+'\Google\Chrome\User Data\Default\GPUCache'); $m+=,@('Cache do Edge',$L+'\Microsoft\Edge\User Data\Default\Cache\Cache_Data|'+$L+'\Microsoft\Edge\User Data\Default\Code Cache|'+$L+'\Microsoft\Edge\User Data\Default\GPUCache|'+$L+'\Microsoft\Edge\User Data\Default\ExtensionActivityEdge'); $m+=,@('Cache do Discord',$R+'\discord\Cache|'+$R+'\discord\Code Cache|'+$R+'\discord\GPUCache'); $m+=,@('Cache do Spotify',$L+'\Packages\SpotifyAB.SpotifyMusic_zpdnekdrzrea0\LocalCache\Spotify\Data'); $m+=,@('Fila de impressao','C:\Windows\System32\spool\PRINTERS'); $m+=,@('Lixeira','C:\$Recycle.Bin'); $out=@(); foreach($e in $m){ Write-Host ('    medindo: '+$e[0]+'...') -ForegroundColor DarkGray; $s=[long]0; foreach($d in ($e[1] -split '\|')){ if($d.Trim().Length -eq 0){ continue }; $s+=SizeOf $d }; $out+=($e[0]+';'+$s+';'+$e[1]) }; $out+=('__free__;'+(Get-PSDrive C).Free+';'); $out+=('__start__;'+(Get-Date).Ticks+';'); Set-Content -LiteralPath '%DATAF%' -Value $out -Encoding UTF8" 2>nul
echo.

REM ============================================================
REM  ETAPA 1 - Parar servicos que travam arquivos
REM ============================================================
echo [1/13] Parando servicos (Windows Update / BITS / Spooler / DO)...
net stop wuauserv >nul 2>&1
net stop bits >nul 2>&1
net stop dosvc >nul 2>&1
net stop spooler >nul 2>&1

REM ============================================================
REM  ETAPA 2 - Temp do Windows e do Usuario
REM ============================================================
echo [2/13] Limpando Temp do Windows e do Usuario...
del /s /f /q "C:\Windows\Temp\*.*" >nul 2>&1
for /d %%x in ("C:\Windows\Temp\*") do rd /s /q "%%x" >nul 2>&1
del /s /f /q "%TEMP%\*.*" >nul 2>&1
for /d %%x in ("%TEMP%\*") do rd /s /q "%%x" >nul 2>&1

REM ============================================================
REM  ETAPA 3 - Prefetch (so no mensal, de proposito)
REM ============================================================
echo [3/13] Limpando Prefetch...
del /s /f /q "C:\Windows\Prefetch\*.*" >nul 2>&1

REM ============================================================
REM  ETAPA 4 - Cache Windows Update + Delivery Optimization
REM ============================================================
echo [4/13] Limpando cache do Windows Update e Delivery Optimization...
del /s /f /q "C:\Windows\SoftwareDistribution\Download\*.*" >nul 2>&1
for /d %%x in ("C:\Windows\SoftwareDistribution\Download\*") do rd /s /q "%%x" >nul 2>&1
del /s /f /q "C:\Windows\SoftwareDistribution\DeliveryOptimization\*.*" >nul 2>&1

REM ============================================================
REM  ETAPA 5 - Logs, crash dumps, WER
REM ============================================================
echo [5/13] Limpando logs, crash dumps e relatorios de erro...
del /s /f /q "C:\Windows\Logs\CBS\*.log" >nul 2>&1
del /s /f /q "C:\Windows\Logs\WindowsUpdate\*.etl" >nul 2>&1
del /s /f /q "C:\Windows\Logs\DISM\*.log" >nul 2>&1
del /s /f /q "%LOCALAPPDATA%\CrashDumps\*.*" >nul 2>&1
del /s /f /q "C:\Windows\Minidump\*.*" >nul 2>&1
del /f /q "C:\Windows\memory.dmp" >nul 2>&1
rmdir /s /q "%LOCALAPPDATA%\Microsoft\Windows\WER" >nul 2>&1
rmdir /s /q "C:\ProgramData\Microsoft\Windows\WER\ReportArchive" >nul 2>&1
rmdir /s /q "C:\ProgramData\Microsoft\Windows\WER\ReportQueue" >nul 2>&1

REM ============================================================
REM  ETAPA 6 - Cache de Shaders + cache de drivers NVIDIA
REM ============================================================
echo [6/13] Limpando cache de shaders (NVIDIA / AMD / Intel / DirectX)...
if exist "%LOCALAPPDATA%\NVIDIA\GLCache" rmdir /s /q "%LOCALAPPDATA%\NVIDIA\GLCache" >nul 2>&1
if exist "%LOCALAPPDATA%\NVIDIA\DXCache" rmdir /s /q "%LOCALAPPDATA%\NVIDIA\DXCache" >nul 2>&1
if exist "%LOCALAPPDATA%\NVIDIA\D3DSCache" rmdir /s /q "%LOCALAPPDATA%\NVIDIA\D3DSCache" >nul 2>&1
if exist "%LOCALAPPDATA%\NVIDIA Corporation\NV_Cache" rmdir /s /q "%LOCALAPPDATA%\NVIDIA Corporation\NV_Cache" >nul 2>&1
if exist "%LOCALAPPDATA%\AMD\DxCache" rmdir /s /q "%LOCALAPPDATA%\AMD\DxCache" >nul 2>&1
if exist "%LOCALAPPDATA%\AMD\GLCache" rmdir /s /q "%LOCALAPPDATA%\AMD\GLCache" >nul 2>&1
if exist "%LOCALAPPDATA%\D3DSCache" rmdir /s /q "%LOCALAPPDATA%\D3DSCache" >nul 2>&1
if exist "%LOCALAPPDATA%\Intel\ShaderCache" rmdir /s /q "%LOCALAPPDATA%\Intel\ShaderCache" >nul 2>&1
echo         ...e cache de download de drivers NVIDIA...
if exist "C:\ProgramData\NVIDIA Corporation\NVIDIA app\UpdateFramework\ota-artifacts" (
    rd /s /q "C:\ProgramData\NVIDIA Corporation\NVIDIA app\UpdateFramework\ota-artifacts" >nul 2>&1
    md "C:\ProgramData\NVIDIA Corporation\NVIDIA app\UpdateFramework\ota-artifacts" >nul 2>&1
)
del /s /f /q "C:\ProgramData\NVIDIA Corporation\Downloader\*.*" >nul 2>&1

REM ============================================================
REM  ETAPA 7 - Thumbnails, Icon Cache, cache IE/Edge antigo
REM ============================================================
echo [7/13] Limpando thumbnails, icon cache e INetCache...
del /s /f /q "%LOCALAPPDATA%\Microsoft\Windows\Explorer\thumbcache_*.db" >nul 2>&1
del /s /f /q "%LOCALAPPDATA%\Microsoft\Windows\Explorer\iconcache_*.db" >nul 2>&1
del /s /f /q "%LOCALAPPDATA%\IconCache.db" >nul 2>&1
del /s /f /q "%LOCALAPPDATA%\Microsoft\Windows\INetCache\*.*" >nul 2>&1

REM ============================================================
REM  ETAPA 8 - Cache do Chrome (feche o Chrome antes!)
REM ============================================================
echo [8/13] Limpando cache do Chrome...
if exist "%LOCALAPPDATA%\Google\Chrome\User Data\Default\Cache\Cache_Data" rd /s /q "%LOCALAPPDATA%\Google\Chrome\User Data\Default\Cache\Cache_Data" >nul 2>&1
if exist "%LOCALAPPDATA%\Google\Chrome\User Data\Default\Code Cache" rd /s /q "%LOCALAPPDATA%\Google\Chrome\User Data\Default\Code Cache" >nul 2>&1
if exist "%LOCALAPPDATA%\Google\Chrome\User Data\Default\GPUCache" rd /s /q "%LOCALAPPDATA%\Google\Chrome\User Data\Default\GPUCache" >nul 2>&1

REM ============================================================
REM  ETAPA 9 - Cache do Edge (feche o Edge antes!)
REM ============================================================
echo [9/13] Limpando cache do Edge...
if exist "%LOCALAPPDATA%\Microsoft\Edge\User Data\Default\Cache\Cache_Data" rd /s /q "%LOCALAPPDATA%\Microsoft\Edge\User Data\Default\Cache\Cache_Data" >nul 2>&1
if exist "%LOCALAPPDATA%\Microsoft\Edge\User Data\Default\Code Cache" rd /s /q "%LOCALAPPDATA%\Microsoft\Edge\User Data\Default\Code Cache" >nul 2>&1
if exist "%LOCALAPPDATA%\Microsoft\Edge\User Data\Default\GPUCache" rd /s /q "%LOCALAPPDATA%\Microsoft\Edge\User Data\Default\GPUCache" >nul 2>&1
del /f /q "%LOCALAPPDATA%\Microsoft\Edge\User Data\Default\ExtensionActivityEdge*" >nul 2>&1

REM ============================================================
REM  ETAPA 10 - Cache do Discord e do Spotify (feche-os antes!)
REM ============================================================
echo [10/13] Limpando cache do Discord e do Spotify...
if exist "%APPDATA%\discord\Cache" rd /s /q "%APPDATA%\discord\Cache" >nul 2>&1
if exist "%APPDATA%\discord\Code Cache" rd /s /q "%APPDATA%\discord\Code Cache" >nul 2>&1
if exist "%APPDATA%\discord\GPUCache" rd /s /q "%APPDATA%\discord\GPUCache" >nul 2>&1
del /s /f /q "%LOCALAPPDATA%\Packages\SpotifyAB.SpotifyMusic_zpdnekdrzrea0\LocalCache\Spotify\Data\*.*" >nul 2>&1
for /d %%x in ("%LOCALAPPDATA%\Packages\SpotifyAB.SpotifyMusic_zpdnekdrzrea0\LocalCache\Spotify\Data\*") do rd /s /q "%%x" >nul 2>&1

REM ============================================================
REM  ETAPA 11 - Spooler de impressao + Lixeira
REM ============================================================
echo [11/13] Limpando fila de impressao e Lixeira...
del /f /q /s "C:\Windows\System32\spool\PRINTERS\*.*" >nul 2>&1
powershell -NoProfile -Command "Clear-RecycleBin -Force -ErrorAction SilentlyContinue" >nul 2>&1

REM ============================================================
REM  ETAPA 12 - DNS + reinicio dos servicos
REM ============================================================
echo [12/13] Limpando cache DNS e reiniciando servicos...
ipconfig /flushdns >nul 2>&1
net start wuauserv >nul 2>&1
net start bits >nul 2>&1
net start dosvc >nul 2>&1
net start spooler >nul 2>&1

REM ============================================================
REM  ETAPA 13 - DISM (a etapa mais lenta)
REM ============================================================
echo.
echo [13/13] Rodando DISM /StartComponentCleanup /ResetBase
echo         ============================================
echo         PACIENCIA: leva de 5 a 15 minutos e a barra
echo         abaixo fica um tempao parada em alguns pontos
echo         (30%%, 70%%). NAO FECHE A JANELA - o resumo
echo         com o total liberado aparece depois disso.
echo         ============================================
echo.
DISM.exe /Online /Cleanup-Image /StartComponentCleanup /ResetBase

REM ============================================================
REM  TWEAKS GAMER (opcional)
REM ============================================================
if /I "%APLICAR_TWEAKS%"=="S" (
    echo.
    echo [EXTRA] Aplicando tweaks GAMER no registro...
    call :ApplyGamerTweaks
) else (
    echo.
    echo [EXTRA] Tweaks gamer ignorados (escolha do usuario).
)

REM ----- Relatorio final: mede DEPOIS e mostra o que foi liberado -----
echo.
echo  Conferindo resultado (rapido)...
cls
echo.
echo ===============================================================
echo            LIMPEZA MENSAL CONCLUIDA COM SUCESSO
echo ===============================================================
powershell -NoProfile -ExecutionPolicy Bypass -Command "function SizeOf([string]$d){ if(-not (Test-Path -LiteralPath $d)){ return [long]0 }; if(Test-Path -LiteralPath $d -PathType Leaf){ return [long](Get-Item -LiteralPath $d -Force).Length }; $o=robocopy $d NULL /L /S /XJ /BYTES /NFL /NDL /NJH /NP /R:0 /W:0; foreach($ln in $o){ if($ln -match '^\s*Bytes\s*:\s*(\d+)'){ return [long]$matches[1] } }; return [long]0 }; function F([long]$b){ if($b -ge 1GB){ return ('{0:N2} GB' -f ($b/1GB)) }; if($b -ge 1MB){ return ('{0:N1} MB' -f ($b/1MB)) }; if($b -ge 1KB){ return ('{0:N0} KB' -f ($b/1KB)) }; return ($b.ToString()+' B') }; $rows=@(); $free0=[long]0; $t0=[long]0; foreach($ln in (Get-Content -LiteralPath '%DATAF%')){ if($ln.Trim().Length -eq 0){ continue }; $p=$ln -split ';',3; if($p[0] -eq '__free__'){ $free0=[long]$p[1]; continue }; if($p[0] -eq '__start__'){ $t0=[long]$p[1]; continue }; $s=[long]0; foreach($d in ($p[2] -split '\|')){ if($d.Trim().Length -eq 0){ continue }; $s+=SizeOf $d }; $freed=[Math]::Max([long]0,[long]$p[1]-$s); $rows+=,@($p[0],$freed) }; $tot=[long]0; foreach($r in $rows){ $tot+=$r[1] }; $free1=(Get-PSDrive C).Free; $ganho=[Math]::Max([long]0,$free1-$free0); $extra=[Math]::Max([long]0,$ganho-$tot); $dur=[int](((Get-Date).Ticks-$t0)/600000000); Write-Host ''; Write-Host '  ESPACO LIBERADO POR CATEGORIA:'; Write-Host '  -------------------------------------------------------------'; foreach($r in $rows){ Write-Host ('   {0,-30} {1,12}' -f $r[0],(F $r[1])) }; Write-Host ('   {0,-30} {1,12}' -f 'DISM / WinSxS e outros',(F $extra)); Write-Host '  -------------------------------------------------------------'; Write-Host ('   {0,-30} {1,12}' -f 'TOTAL LIBERADO',(F ($tot+$extra))) -ForegroundColor Green; Write-Host ''; Write-Host ('   Espaco livre no C: antes  : '+(F $free0)); Write-Host ('   Espaco livre no C: agora  : '+(F $free1)); Write-Host ('   Duracao da limpeza        : '+$dur+' minutos'); Add-Content -LiteralPath '%LOGF%' -Value ((Get-Date -Format 'yyyy-MM-dd HH:mm')+'  [MENSAL]  Liberado: '+(F ($tot+$extra))+'  -  Livre no C: '+(F $free1))" 2>nul
del "%DATAF%" >nul 2>&1
echo.
echo ===============================================================
echo.
echo  Observacoes:
echo   - Os primeiros jogos abertos podem demorar um pouco mais
echo     (shaders sendo recompilados).
echo   - Parte do ganho do DISM so aparece apos REINICIAR o PC.
echo   - Se aplicou os tweaks gamer, REINICIE o PC.
echo   - Historico completo em: "Limpador - Historico.txt"
echo.
pause
exit /b 0


REM ===================================================================
REM  SUB-ROTINA: Aplica tweaks de performance gamer
REM ===================================================================
:ApplyGamerTweaks
echo.
echo    -^> Desativando GameDVR / Game Bar (reduz overhead em jogos)...
reg add "HKCU\System\GameConfigStore" /v GameDVR_Enabled /t REG_DWORD /d 0 /f >nul 2>&1
reg add "HKCU\System\GameConfigStore" /v GameDVR_FSEBehaviorMode /t REG_DWORD /d 2 /f >nul 2>&1
reg add "HKCU\System\GameConfigStore" /v GameDVR_HonorUserFSEBehaviorMode /t REG_DWORD /d 1 /f >nul 2>&1
reg add "HKCU\System\GameConfigStore" /v GameDVR_DXGIHonorFSEWindowsCompatible /t REG_DWORD /d 1 /f >nul 2>&1
reg add "HKLM\SOFTWARE\Policies\Microsoft\Windows\GameDVR" /v AllowGameDVR /t REG_DWORD /d 0 /f >nul 2>&1
reg add "HKCU\Software\Microsoft\GameBar" /v AutoGameModeEnabled /t REG_DWORD /d 1 /f >nul 2>&1
reg add "HKCU\Software\Microsoft\GameBar" /v ShowStartupPanel /t REG_DWORD /d 0 /f >nul 2>&1

echo    -^> Ativando GPU Hardware Scheduling (HAGS)...
reg add "HKLM\SYSTEM\CurrentControlSet\Control\GraphicsDrivers" /v HwSchMode /t REG_DWORD /d 2 /f >nul 2>&1

echo    -^> Priorizando jogos no agendador de multimidia (MMCSS)...
reg add "HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile\Tasks\Games" /v "GPU Priority" /t REG_DWORD /d 8 /f >nul 2>&1
reg add "HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile\Tasks\Games" /v "Priority" /t REG_DWORD /d 6 /f >nul 2>&1
reg add "HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile\Tasks\Games" /v "Scheduling Category" /t REG_SZ /d "High" /f >nul 2>&1
reg add "HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Multimedia\SystemProfile\Tasks\Games" /v "SFIO Priority" /t REG_SZ /d "High" /f >nul 2>&1

echo    -^> Otimizando reserva de rede (libera 100%% da banda para jogos)...
reg add "HKLM\SOFTWARE\Policies\Microsoft\Windows\Psched" /v NonBestEffortLimit /t REG_DWORD /d 0 /f >nul 2>&1

echo    -^> Reduzindo latencia TCP (TcpAckFrequency / TCPNoDelay)...
for /f "tokens=*" %%i in ('reg query "HKLM\SYSTEM\CurrentControlSet\Services\Tcpip\Parameters\Interfaces" /s ^| findstr "HKEY"') do (
    reg add "%%i" /v TcpAckFrequency /t REG_DWORD /d 1 /f >nul 2>&1
    reg add "%%i" /v TCPNoDelay /t REG_DWORD /d 1 /f >nul 2>&1
    reg add "%%i" /v TcpDelAckTicks /t REG_DWORD /d 0 /f >nul 2>&1
)

echo    -^> Otimizando Mouse (remove aceleracao - melhor para FPS)...
reg add "HKCU\Control Panel\Mouse" /v MouseSpeed /t REG_SZ /d "0" /f >nul 2>&1
reg add "HKCU\Control Panel\Mouse" /v MouseThreshold1 /t REG_SZ /d "0" /f >nul 2>&1
reg add "HKCU\Control Panel\Mouse" /v MouseThreshold2 /t REG_SZ /d "0" /f >nul 2>&1

echo    -^> Ativando plano de energia ULTIMATE PERFORMANCE...
powercfg /getactivescheme | findstr /I "Ultimate" >nul 2>&1
if errorlevel 1 (
    powercfg -duplicatescheme e9a42b02-d5df-448d-aa00-03f14749eb61 >nul 2>&1
    for /f "tokens=2 delims=:(" %%a in ('powercfg /list ^| findstr /I "Ultimate"') do (
        for /f "tokens=1" %%g in ("%%a") do powercfg /setactive %%g >nul 2>&1
    )
)

echo    -^> Desativando Nagle Algorithm (reduz lag em jogos online)...
reg add "HKLM\SOFTWARE\Microsoft\MSMQ\Parameters" /v TCPNoDelay /t REG_DWORD /d 1 /f >nul 2>&1

echo    -^> Desativando dicas/sugestoes do Windows (menos popups)...
reg add "HKCU\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager" /v SubscribedContent-338389Enabled /t REG_DWORD /d 0 /f >nul 2>&1
reg add "HKCU\Software\Microsoft\Windows\CurrentVersion\ContentDeliveryManager" /v SystemPaneSuggestionsEnabled /t REG_DWORD /d 0 /f >nul 2>&1

echo.
echo    Tweaks gamer aplicados. REINICIE o PC para efeito completo.
goto :eof
