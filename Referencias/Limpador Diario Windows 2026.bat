@echo off
setlocal
title Limpador Windows Diario 2026
color 0a
mode con: cols=100 lines=42 >nul 2>&1

REM ============================================================
REM  LIMPADOR WINDOWS DIARIO 2026 - v2.1
REM  Limpeza leve para rodar diariamente (~30s a 1min).
REM
REM  v2.1: medicao via ROBOCOPY (muito mais rapida que a v2.0)
REM        e progresso na tela categoria por categoria - nunca
REM        mais parece "travado".
REM  v2.0: - Corrigido relatorio ZERADO (WMIC removido do
REM          Windows 11 24H2+ era a causa)
REM        - Relatorio REAL por categoria (KB / MB / GB)
REM        - Auto-elevacao (duplo clique direto)
REM        - Historico em "Limpador - Historico.txt"
REM        - Prefetch NAO e mais limpo diariamente (limpar todo
REM          dia deixa apps/jogos MAIS lentos; o mensal limpa)
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
echo            LIMPADOR WINDOWS DIARIO 2026  -  v2.1
echo ===============================================================
echo.
echo  Limpeza rapida para uso diario. Vai limpar:
echo    - Arquivos temporarios (Windows e usuario)
echo    - Logs de sistema e crash dumps
echo    - Cache de shaders (NVIDIA / AMD / Intel / DirectX)
echo    - Thumbnails e icon cache
echo    - Fila de impressao e Lixeira
echo    - Cache DNS
echo.
echo  Tempo estimado: 30 segundos a 1 minuto.
echo ===============================================================
echo.
echo  Medindo tamanho das pastas ANTES da limpeza...
powershell -NoProfile -ExecutionPolicy Bypass -Command "function SizeOf([string]$d){ if(-not (Test-Path -LiteralPath $d)){ return [long]0 }; if(Test-Path -LiteralPath $d -PathType Leaf){ return [long](Get-Item -LiteralPath $d -Force).Length }; $o=robocopy $d NULL /L /S /XJ /BYTES /NFL /NDL /NJH /NP /R:0 /W:0; foreach($ln in $o){ if($ln -match '^\s*Bytes\s*:\s*(\d+)'){ return [long]$matches[1] } }; return [long]0 }; $L=$env:LOCALAPPDATA; $m=@(); $m+=,@('Temp do Windows','C:\Windows\Temp'); $m+=,@('Temp do usuario',$env:TEMP); $m+=,@('Logs e crash dumps','C:\Windows\Logs\CBS|C:\Windows\Logs\WindowsUpdate|C:\Windows\Logs\DISM|'+$L+'\CrashDumps|C:\Windows\Minidump|'+$L+'\Microsoft\Windows\WER|C:\ProgramData\Microsoft\Windows\WER\ReportArchive|C:\ProgramData\Microsoft\Windows\WER\ReportQueue'); $m+=,@('Cache de shaders',$L+'\NVIDIA\GLCache|'+$L+'\NVIDIA\DXCache|'+$L+'\NVIDIA\D3DSCache|'+$L+'\NVIDIA Corporation\NV_Cache|'+$L+'\AMD\DxCache|'+$L+'\AMD\GLCache|'+$L+'\D3DSCache|'+$L+'\Intel\ShaderCache'); $m+=,@('Thumbnails e icones',$L+'\Microsoft\Windows\Explorer'); $m+=,@('Fila de impressao','C:\Windows\System32\spool\PRINTERS'); $m+=,@('Lixeira','C:\$Recycle.Bin'); $out=@(); foreach($e in $m){ Write-Host ('    medindo: '+$e[0]+'...') -ForegroundColor DarkGray; $s=[long]0; foreach($d in ($e[1] -split '\|')){ if($d.Trim().Length -eq 0){ continue }; $s+=SizeOf $d }; $out+=($e[0]+';'+$s+';'+$e[1]) }; $out+=('__free__;'+(Get-PSDrive C).Free+';'); $out+=('__start__;'+(Get-Date).Ticks+';'); Set-Content -LiteralPath '%DATAF%' -Value $out -Encoding UTF8" 2>nul
echo.

REM ============================================================
REM  ETAPA 1 - Temp do Windows e do Usuario
REM ============================================================
echo [1/6] Limpando Temp do Windows e do Usuario...
del /s /f /q "C:\Windows\Temp\*.*" >nul 2>&1
for /d %%x in ("C:\Windows\Temp\*") do rd /s /q "%%x" >nul 2>&1
del /s /f /q "%TEMP%\*.*" >nul 2>&1
for /d %%x in ("%TEMP%\*") do rd /s /q "%%x" >nul 2>&1

REM ============================================================
REM  ETAPA 2 - Logs e crash dumps
REM ============================================================
echo [2/6] Limpando logs do sistema e crash dumps (WER)...
del /s /f /q "C:\Windows\Logs\CBS\*.log" >nul 2>&1
del /s /f /q "C:\Windows\Logs\WindowsUpdate\*.etl" >nul 2>&1
del /s /f /q "C:\Windows\Logs\DISM\*.log" >nul 2>&1
del /s /f /q "%LOCALAPPDATA%\CrashDumps\*.*" >nul 2>&1
del /s /f /q "C:\Windows\Minidump\*.*" >nul 2>&1
rmdir /s /q "%LOCALAPPDATA%\Microsoft\Windows\WER" >nul 2>&1
rmdir /s /q "C:\ProgramData\Microsoft\Windows\WER\ReportArchive" >nul 2>&1
rmdir /s /q "C:\ProgramData\Microsoft\Windows\WER\ReportQueue" >nul 2>&1

REM ============================================================
REM  ETAPA 3 - Cache de Shaders (NVIDIA / AMD / Intel)
REM ============================================================
echo [3/6] Limpando cache de shaders (NVIDIA / AMD / Intel / DirectX)...
if exist "%LOCALAPPDATA%\NVIDIA\GLCache" rmdir /s /q "%LOCALAPPDATA%\NVIDIA\GLCache" >nul 2>&1
if exist "%LOCALAPPDATA%\NVIDIA\DXCache" rmdir /s /q "%LOCALAPPDATA%\NVIDIA\DXCache" >nul 2>&1
if exist "%LOCALAPPDATA%\NVIDIA\D3DSCache" rmdir /s /q "%LOCALAPPDATA%\NVIDIA\D3DSCache" >nul 2>&1
if exist "%LOCALAPPDATA%\NVIDIA Corporation\NV_Cache" rmdir /s /q "%LOCALAPPDATA%\NVIDIA Corporation\NV_Cache" >nul 2>&1
if exist "%LOCALAPPDATA%\AMD\DxCache" rmdir /s /q "%LOCALAPPDATA%\AMD\DxCache" >nul 2>&1
if exist "%LOCALAPPDATA%\AMD\GLCache" rmdir /s /q "%LOCALAPPDATA%\AMD\GLCache" >nul 2>&1
if exist "%LOCALAPPDATA%\D3DSCache" rmdir /s /q "%LOCALAPPDATA%\D3DSCache" >nul 2>&1
if exist "%LOCALAPPDATA%\Intel\ShaderCache" rmdir /s /q "%LOCALAPPDATA%\Intel\ShaderCache" >nul 2>&1

REM ============================================================
REM  ETAPA 4 - Thumbnails e Icon Cache
REM ============================================================
echo [4/6] Limpando thumbnails e icon cache...
del /s /f /q "%LOCALAPPDATA%\Microsoft\Windows\Explorer\thumbcache_*.db" >nul 2>&1
del /s /f /q "%LOCALAPPDATA%\Microsoft\Windows\Explorer\iconcache_*.db" >nul 2>&1
del /s /f /q "%LOCALAPPDATA%\IconCache.db" >nul 2>&1

REM ============================================================
REM  ETAPA 5 - Spooler de impressao + Lixeira
REM ============================================================
echo [5/6] Limpando fila de impressao e Lixeira...
del /f /q /s "C:\Windows\System32\spool\PRINTERS\*.*" >nul 2>&1
powershell -NoProfile -Command "Clear-RecycleBin -Force -ErrorAction SilentlyContinue" >nul 2>&1

REM ============================================================
REM  ETAPA 6 - Flush DNS
REM ============================================================
echo [6/6] Limpando cache DNS...
ipconfig /flushdns >nul 2>&1

REM ----- Relatorio final: mede DEPOIS e mostra o que foi liberado -----
echo.
echo  Conferindo resultado...
cls
echo.
echo ===============================================================
echo              LIMPEZA DIARIA CONCLUIDA
echo ===============================================================
powershell -NoProfile -ExecutionPolicy Bypass -Command "function SizeOf([string]$d){ if(-not (Test-Path -LiteralPath $d)){ return [long]0 }; if(Test-Path -LiteralPath $d -PathType Leaf){ return [long](Get-Item -LiteralPath $d -Force).Length }; $o=robocopy $d NULL /L /S /XJ /BYTES /NFL /NDL /NJH /NP /R:0 /W:0; foreach($ln in $o){ if($ln -match '^\s*Bytes\s*:\s*(\d+)'){ return [long]$matches[1] } }; return [long]0 }; function F([long]$b){ if($b -ge 1GB){ return ('{0:N2} GB' -f ($b/1GB)) }; if($b -ge 1MB){ return ('{0:N1} MB' -f ($b/1MB)) }; if($b -ge 1KB){ return ('{0:N0} KB' -f ($b/1KB)) }; return ($b.ToString()+' B') }; $rows=@(); $free0=[long]0; $t0=[long]0; foreach($ln in (Get-Content -LiteralPath '%DATAF%')){ if($ln.Trim().Length -eq 0){ continue }; $p=$ln -split ';',3; if($p[0] -eq '__free__'){ $free0=[long]$p[1]; continue }; if($p[0] -eq '__start__'){ $t0=[long]$p[1]; continue }; $s=[long]0; foreach($d in ($p[2] -split '\|')){ if($d.Trim().Length -eq 0){ continue }; $s+=SizeOf $d }; $freed=[Math]::Max([long]0,[long]$p[1]-$s); $rows+=,@($p[0],$freed) }; $tot=[long]0; foreach($r in $rows){ $tot+=$r[1] }; $free1=(Get-PSDrive C).Free; $dur=[int](((Get-Date).Ticks-$t0)/10000000); Write-Host ''; Write-Host '  ESPACO LIBERADO POR CATEGORIA:'; Write-Host '  -------------------------------------------------------------'; foreach($r in $rows){ Write-Host ('   {0,-30} {1,12}' -f $r[0],(F $r[1])) }; Write-Host '  -------------------------------------------------------------'; Write-Host ('   {0,-30} {1,12}' -f 'TOTAL LIBERADO',(F $tot)) -ForegroundColor Green; Write-Host ''; Write-Host ('   Espaco livre no C: antes  : '+(F $free0)); Write-Host ('   Espaco livre no C: agora  : '+(F $free1)); Write-Host ('   Duracao da limpeza        : '+$dur+' segundos'); Add-Content -LiteralPath '%LOGF%' -Value ((Get-Date -Format 'yyyy-MM-dd HH:mm')+'  [DIARIO]  Liberado: '+(F $tot)+'  -  Livre no C: '+(F $free1))" 2>nul
del "%DATAF%" >nul 2>&1
echo.
echo ===============================================================
echo.
echo  Observacao: o primeiro jogo aberto apos a limpeza pode
echo  demorar um pouco mais para carregar (shaders recompilando).
echo  Historico completo em: "Limpador - Historico.txt"
echo.
echo  Para limpeza profunda (Update + DISM + tweaks gamer):
echo  rode o Limpador Mensal Windows 2026.bat
echo.
pause
exit /b 0
