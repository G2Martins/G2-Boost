# Análise estática — ExitLag e SpaceSniffer
**Última revisão:** 2026-10-18 (adendo §2.12)

> Método: somente análise estática (parser PE próprio em Python + extração de strings ASCII/UTF-16LE + leitura do `.inf`). Nenhum binário foi executado, nada em `D:\ExitLag` foi alterado, nenhum asset/código foi copiado. Documentamos só **mecanismos** (APIs, chaves, serviços, drivers). Onde algo é inferência (não confirmado em disassembly) está marcado **[inferido]**.
> Alvos: ExitLag 5.x (`D:\ExitLag`, 1537 arquivos, ~282 MB, build `exitlag-client-qml`) e SpaceSniffer 2.1.0.21 x64.

## 1. Inventário de arquivos

### 1.1 ExitLag — arquivos próprios (raiz de `D:\ExitLag`)
| Arquivo | Tamanho (B) | Assinante | Tipo / tecnologia |
|---|---|---|---|
| `ExitLag.exe` | 114.441.408 | Skowsand Tecnologia Ltda (válida) | GUI **Qt 6 / QML** (C++ MSVC, vcpkg). QML compilado AOT + recursos (QML, fontes, `.qm` zlib) embutidos no exe. Usa Boost.Asio, Qt6Network/NetworkAuth/Charts/Sql/Svg, WebView2Loader (reCAPTCHA), PresentMonAPI2 |
| `libexitlag.dll` | 3.811.008 | Skowsand | Core de rede (protobuf `ExitLag_WebService_Protocol_L2`, bridges/rotas, OpenSSL, zlib) |
| `exitlag.dll` | 352.256 | **não assinada** | Pinger de bridges (ICMP v4/v6, TCP, UDP: `[BridgePinger]`, `[BridgePool]`), OpenSSL, `IcmpSendEcho2`/`Icmp6SendEcho2` |
| `ExitLagPmService.exe` | 3.595.968 | Skowsand | **Serviço** `ExitLagPmService` = fork do **Intel PresentMon** (`...\exitlagpresentmon\IntelPresentMon\PresentMonService`): ETW (`StartTraceW`, `EnableTraceEx2`, `ProcessTrace`, `OpenTraceW`), TDH, PDH, named pipe; alimenta FPS/frametime do overlay |
| `handler.exe` | 613.568 | Skowsand | **Crashpad** handler (WinHTTP, zlib) |
| `ndisapi.dll` | 47.288 | Skowsand | Wrapper **Windows Packet Filter / NDISAPI** (NT Kernel Resources): `CNdisApi::SetPacketFilterTable/ReadPackets/SendPacketsToMstcp/SendPacketsToAdapter/SetAdapterMode/GetTcpipBoundAdaptersInfo`; fala com `\\.\NDEXTLAG` via `DeviceIoControl` |
| `nfapi.dll` | 602.808 | Skowsand | **NetFilter SDK** (`...\netfiltersdk\nfapi`): `nf_init`, `nf_setRulesEx`, `nf_addFlowCtl/modifyFlowCtl/getFlowCtlStat`, `nf_setTCPFlowCtl/UDPFlowCtl`, `nf_udpPostSend/Receive`, `nf_getProcessNameFromKernel`; instala driver com `CreateServiceW` (`SYSTEM\CurrentControlSet\Services\%s`, `system32\drivers\%S.sys`) |
| `nfregdrv.exe` | 56.000 | Skowsand | CLI: `nfregdrv.exe <driver_name>` registra / `-u` remove `system32\drivers\<name>.sys` |
| `snetcfg.exe` | 15.360 | **não assinada** | Sample MS `netcfg` (INetCfg + SetupAPI): `-l <inf> -c s -i <comp-id>`; usado p/ instalar o LWF NDIS |
| `exitlag_autostart.exe` | 114.360 | Skowsand | Lançador (`ShellExecuteW` verbo `RunAs` -> `ExitLag.exe`) |
| `exitlag_reboot_required.exe` | 113.848 | Skowsand | Mutex `Global\ExitLag_IsRebootRequired` |
| `unins000.exe/.dat/.msg` | 4,3 MB / 8,1 MB / 25 KB | Skowsand | Inno Setup uninstaller |
| `drivers\ndextlag.sys` | 59.696 | **Microsoft Windows Hardware Compatibility Publisher** (WHQL) | **Driver NDIS 6 Lightweight Filter** ("ExitLag LightWeight Filter") |
| `drivers\ndextlag_lwf.inf` / `.cat` | 2.864 / 11.860 | — | Classe `NetService`, `DriverVer = 03/16/2023,3.4.0.1` |
| ~60 `Qt6*.dll`, `icu*`, `abseil_dll`, `libprotobuf(-lite)`, `libcrypto/ssl-3`, `boost_*` (vc143 e vc145), `fmt`, `zip`, `zlib*`, `d3dcompiler_47`, `dxcompiler`, `dxil`, `opengl32sw`, `msvcp140*`, `vcruntime*`, `concrt140`, `WebView2Loader` | — | MS/Qt/terceiros | Runtime e dependências |
| `platforms\ styles\ tls\ sqldrivers\ imageformats\ iconengines\ networkinformation\qnetworklistmanager.dll`, `qml\` (493 `.qml` + 804 `.png` = estilos Qt Quick Controls, **não** são telas do ExitLag) | — | Qt | Plugins Qt |
| `THIRD_PARTY_LICENSES.txt` | 28.330 | — | Cita boost, Crashpad, libpatricia, libzip, OpenSSL, **Presentmon**, **ADL (AMD)**, **NVAPI**, DirectX error lib, ICU, Protobuf, Qt, SHA RFC6234, **Windows Packet Filter** |

Totais: 143 `.dll`, 8 `.exe`, 1 `.sys`, 1 `.inf`, 1 `.cat`, 493 `.qml`, 42 `.qmltypes`, 804 `.png`.

Observações:
- **Não é Electron/CEF**: não há `app.asar`, `.js` bundles, JSON/YAML de config nem arquivos de localização soltos. Toda a UI (telas QML, 8 catálogos `.qm`, fontes Ubuntu/Organetto/Font Awesome 7 Pro) está **dentro de `ExitLag.exe`**; QML parcialmente legível, o resto como bytecode AOT.
- Telas QML (nomes de recursos): `PerformanceBoost/{SystemTweaksTab,RamCleanerTab,DNSOptimizerTab,RamStatusCard}`, `ConnectionDiagnostics/{ScannerTab,RealTimeTab,PingMonitoringTab,ToolsTab/{MTRTool,PingTcpingTool}}`, `TrafficShaper/{TrafficShaper,BandwidthDrawer,Exceptions}`, `MultiInternet/*`, `GamePage/*` (rotas/diagrama), `OverlayHud`, `CommunityServers`, `GeneralSettings`.
- Os UI strings no exe são em inglês; o catálogo pt-BR foi lido só em memória (blob zlib em `0x633ded9`). Os rótulos pt-BR citados abaixo vêm dele.

### 1.2 Drivers / serviços / integrações
| Item | Evidência | Função |
|---|---|---|
| Driver `ndextlag` (NDIS LWF) | `.inf`: `ServiceType=1`, `StartType=1 (SYSTEM_START)`, `LoadOrderGroup=NDIS`, `FilterClass=compression`, `FilterRunType=1`, `NetCfgInstanceId={8FCEE7DF-95A6-484F-8049-2A751634C774}`; `.sys` importa só `ntoskrnl` + `NDIS.SYS` (`NdisFRegisterFilterDriver`, `NdisFSendNetBufferLists`, `NdisFIndicateReceiveNetBufferLists`, `NdisRegisterDeviceEx`...); cria `\Device\NDEXTLAG` / `\DosDevices\NDEXTLAG`; lê `MTUDecrement`, `StartupMode` em `Parameters` | "NDIS (legado)": interceptação/redirecionamento de pacotes. É o driver do **WinpkFilter** renomeado |
| Driver NetFilter SDK (`nfsdk`) | `nfapi.dll`/`nfregdrv.exe`; `nfsdk_available`, `Firewall blocks Proxy (nfsdk)`. **`.sys` NÃO está em `D:\ExitLag`** | Fallback de redirecionamento + **flow control** (Traffic Shaper) |
| "WFP driver" (modo recomendado) | `Beta - WFP driver is not available`, `settings.toggle.wfp_routes.config`, `isWinDivertRunning`, aviso "Conflict with WinDivert detected! ... GoodbyeDPI" | Modo **WFP** (Multi Internet só funciona nele). `.sys` ausente do diretório; provável callout WFP/WinDivert-like **[inferido]** |
| Serviço `ExitLagPmService` | `[SystemMonitor] ExitLagPmService not running (state=%1%), starting it` | FPS/frametime via ETW |
| Hotkey | `[Overlay] Registered CTRL+SHIFT+I hotkey` (`RegisterHotKey`) | HUD só em jogos suportados |
| Firewall | `netsh advfirewall firewall add rule name="%1" program="%2" dir=%3 action=allow enable=yes` e `... del rule name="%1"` | Libera o exe |
| WMI recovery | `winmgmt /resetrepository`, `/salvagerepository %windir%\System32\wbem`, `sc config winmgmt start= disabled|auto`, `net stop/start winmgmt`; `SOFTWARE\Microsoft\Wbem\CIMOM` (`Autorecover MOFs`), `...\CIMOM\SecuredHostProviders` (`WMIRefresher`) | Auto-reparo do WMI (EEE e NICs dependem dele) |

## 2. ExitLag — por recurso

### 2.0 APIs importadas / resolvidas dinamicamente (`ExitLag.exe`)
- `IPHLPAPI`: `IcmpCreateFile/Icmp6CreateFile/IcmpSendEcho2Ex/Icmp6SendEcho2/IcmpParseReplies`, `GetExtendedTcpTable/GetExtendedUdpTable`, `GetOwnerModuleFromTcp/Udp(6)Entry`, `SetTcpEntry`, `GetAdaptersAddresses`, `GetNetworkParams`, `Get/Set/InitializeIpInterfaceEntry`, `GetIpInterfaceTable`, `GetIpForwardTable2`, `ResolveIpNetEntry2`, `ConvertInterfaceLuidToIndex/Guid`, **`GetInterfaceDnsSettings/SetInterfaceDnsSettings/FreeInterfaceDnsSettings`**.
- `ADVAPI32`: `Reg{Open,Create}KeyExW/RegSetValueExW/RegQueryValueExW/RegEnumKeyExW/RegEnumValueW/RegDeleteValueW`, `OpenSCManagerW/OpenServiceW/StartServiceW/QueryServiceStatus`, `AdjustTokenPrivileges/LookupPrivilegeValueW/OpenProcessToken/GetTokenInformation`, `InitiateSystemShutdownW`.
- `POWRPROF`: `PowerGetActiveScheme/PowerSetActiveScheme/PowerDuplicateScheme/PowerEnumerate/PowerReadFriendlyName/PowerWriteFriendlyName/PowerWriteACValueIndex/PowerWriteDCValueIndex`.
- `PDH`: `PdhOpenQueryW/PdhAddEnglishCounterW/PdhCollectQueryData/PdhGetFormattedCounterArrayW`.
- `KERNEL32`: **`K32EmptyWorkingSet`**, `K32GetProcessMemoryInfo`, `GlobalMemoryStatusEx`, `Process32FirstW/NextW`, `OpenProcess`, `Get/SetPriorityClass`, `SetThreadPriority`, `QueryFullProcessImageName*`, `DeviceIoControl`, `CreateNamedPipe*`, `Wow64DisableWow64FsRedirection`.
- `USER32`: `SystemParametersInfoW`, `RegisterHotKey`, `EnumWindows`, `GetForegroundWindow`, `ExitWindowsEx`.
- Por `GetProcAddress` (strings): `ntdll!NtQuerySystemInformation`, **`ntdll!NtSetSystemInformation`**, `NtQueryTimerResolution`, `NtSetTimerResolution`, `kernel32!SetProcessInformation`, `dnsapi!DnsFlushResolverCache`, `NsiSetAllParameters`, `user32!GetWindowBand`, `nvml.dll`, `atiadlxx.dll`.
- NÃO importa: `SetProcessWorkingSetSize`, `SetSystemFileCacheSize`, `CreateServiceW` (esta só em `nfapi.dll`).

### 2.1 Monitor de rede / Interfaces / Multi Internet
- Evidência: logs `[NetworkInterfaces]`, `[NLM]` (INetworkListManager + `IConnectionPoint`), `[ConnectivityProbe]` (probes `8.8.8.8` e `2001:4860:4860::8888`), `[IpInfo]`; `GetAdaptersAddresses`, `GetIpForwardTable2`. Ignora adaptadores "Virtual", "Hyper-V", "Loopback", "Tunnel", "Bluetooth".
- Multi Internet: mín. 2 interfaces com internet ("fewer than 2 active multi-internet interfaces"); **só no modo WFP** ("Cannot enable multi-internet without WFP"). Prioridade por interface: `NsiSetAllParameters` ("Setting interface #%1% metric to %3%") e persistência no registro `System\CurrentControlSet\Services\Tcpip\Parameters\Interfaces\<GUID>` valor **`InterfaceMetric`** ("Cannot persist interface priority across reboots"). IP público por interface via HTTP (host não está em claro).
- pt-BR: "Analisador de rede", "Multi internet habilitada", "Definir ordem de prioridade da rede".
- **G2 Boost**: `GetAdaptersAddresses` + `GetIpInterfaceEntry/SetIpInterfaceEntry` (`UseAutomaticMetric=FALSE`, `Metric`) pelo crate `windows` (IpHelper); eventos via `NotifyIpInterfaceChange` ou `INetworkListManager`. Sem `netsh`.

### 2.2 Scanner / Speedtest (+ bufferbloat)
- Evidência (`[Scanner]`, `[GenericScanner]`): "Fetching servers list...", "Selecting best server based on ping...", "Starting download pre-test", "Testing upload speed...", "Initializing bufferbloat test", "Bufferbloat grade: %1"; campos JSON `serverName, serverCountry, serverDistance, downloadVolume, uploadVolume, noloadRtt/Jitter, downloadRtt/Jitter, uploadRtt/Jitter, bufferbloat`; parâmetros `client`, `ignoreids`, `upload`, `socket-upload`, `maxthreads` (padrão tipo Ookla/speedtest.net **[inferido]**). Streams escalam dinamicamente ("adding upload stream (%1 -> %2, %3 Mbps/stream)").
- **Endpoint do speedtest NÃO está em claro** no binário (vem de RemoteConfig/ServiceWS). Hosts fixos encontrados: `https://www.exitlag.com`, `https://stg-portal-web.exitlag.net`, `https://b2b.exitlag.com/community-network`, `https://exitl.ag/...`, `https://discord.com/invite/exitlag`, CDN de assets `pub-695f0e2f2e8445ee8bc81d178ba819a6.r2.dev` (Cloudflare R2), rota `/v2/ping_for_games_feature_data` (Bearer JWT) e `/event.v1.EventService/AppConnReport`.
- pt-BR: aba "Scanner"; "Para resultados mais precisos, pause downloads e evite usar outros dispositivos na rede."
- **G2 Boost**: HTTP com `reqwest`; download/upload multi-stream medindo RTT sob carga (bufferbloat = RTT carregado − RTT ocioso, nota por faixa). Servidor próprio ou público (ex. Cloudflare `speed.cloudflare.com/__down|__up`, LibreSpeed).

### 2.3 Ping para jogos / Monitoramento em tempo real
- Evidência: `[GamesPing]`, `[PingMonitor]`, `[UdpPing]` (pong com sequência), `[PingTest] Start %1 test (host, count, interval ms, timeout ms)`, `[QualityMeasurement]` (ping direto ao DS vs. via bridge, regras por app com port range), `[ConnectionReport]` (`orig_min/max/avg_rtt`, `improvement_rtt`); resposta de `/v2/ping_for_games_feature_data`: `regionsByAppId -> regions -> datacenterIds`. Continentes `CONTINENT_{NORTH_AMERICA,SOUTH_AMERICA,EUROPE,ASIA,AFRICA,OCEANIA}`. Lista de jogos/servidores vem do servidor (protobuf), não do exe.
- pt-BR: "Ping para jogos", "Monitoramento em tempo real", "Ping médio", "Ping atual", "Ping mais baixo", "Ping estimado".
- **G2 Boost**: `IcmpSendEcho2` (sem raw socket) ou tempo de connect TCP/UDP; amostras de RTT/jitter/perda.

### 2.4 Ferramentas MTR / Ping / TCPing
- Evidência: "Starting MTR for", "[MTR] TTL out of hops range", "Refined server TTL", "Intermediate hop %1 TTL changed", reverse lookup; `[PingTest]` com `proto, received, minRtt, maxRtt, avgRtt, jitter, packetLossPerc, pingSize`. Tudo **in-process** (`IcmpSendEcho2Ex` com TTL crescente; sem `tracert.exe`/`ping.exe`). TCPing = connect TCP cronometrado.
- pt-BR: "Estatísticas MTR", "Executando teste MTR", "Salvar resultados MTR", "Este recurso mostra o caminho que seus pacotes percorrem pela rede."
- **G2 Boost**: loop de TTL com `IcmpSendEcho2Ex` (`IP_OPTION_INFORMATION.Ttl`), hop identificado em `IP_TTL_EXPIRED_TRANSIT` (11013); IPv6 via `Icmp6SendEcho2`.

### 2.5 Rota de jogo / driver / redirecionamento
- Métodos (UI "Método de redirecionamento de pacotes"): **WFP** (moderno), **NDIS (legado)** = `ndextlag.sys` + `ndisapi.dll`, **NFSDK** (fallback automático: "Firewall blocked ExitLag. Switching to NFSDK"). Modos de rota: `auto_routes`, `manual_routes`, `dual_routes`, `ndis_routes`, `wfp_routes`, "Preferir rotas IPv6", "Usar rotas locais primeiro", Multipath.
- Arquitetura (`[Proxy]`, `libexitlag`): proxy local (`AcceptEx`), tunelamento TCP/UDP por processo para bridge servers (BS) e final server (FS) ExitLag, TLS (OpenSSL), retransmissão/keep-alive de UDP, `SetTcpEntry` p/ derrubar TCP existente. Ignora faixas privadas (`10/8, 100.64/10, 127/8, 169.254/16, 172.16/12, 192.168/16, 224/4`). Conflitos detectados: Dr.Web, WinDivert/GoodbyeDPI. Há detecção de MUX switch de GPU (LUID do adaptador de render vs. monitor).
- **G2 Boost**: exige driver próprio assinado (WFP callout) ou WinDivert (avaliar licença). Fora do MVP; alternativa user-mode: `IP_UNICAST_IF` por socket.

### 2.6 Ajustes de sistema (PC Boost -> System Tweaks) — chaves exatas
Chaves de telemetria/config: `PC_BOOST_SYSTEM_TWEAKS_{CORE_PARKING, ENERGY_ETHERNET, GAME_BAR, GAME_PRIORITY, GPU_PERFORMANCE_PROFILE, INPUT_LAG, KEYBOARD_RESPONSE, MAX_GAME_PRIORITY, NTFS_LAST_ACCESS, PERF_COUNTER, SUPERFETCH, ULTIMATE_POWER_PLAN, UNREAL_ENGINE_CONFIG, WINDOWS_SEARCH}_CONFIG`; estado persistido: `win_perf_counters`, `ntfs_last_access`, `win_update_delivery_optimiz(ation)`.

| Tweak (rótulo no exe) | Evidência exata | Mecanismo | G2 Boost |
|---|---|---|---|
| **Turn off Windows performance counters** | wide `SOFTWARE\Microsoft\Windows NT\CurrentVersion\Perflib` + valor `Disable Performance Counters`; "Failed to change registry key to disable Windows Performance Counters". Overlay depende dos contadores | `HKLM\...\Perflib\Disable Performance Counters = DWORD 1` (dado **[inferido]**) | `RegSetValueExW` (admin), guardar original |
| **Turn off Game Bar background recordings** | wide `Software\Microsoft\Windows\CurrentVersion\GameDVR`, valores `AppCaptureEnabled`, `HistoricalCaptureEnabled`; `[GameDVR] Cannot enable GameBar/GameDVR`, "Changed status of Game Bar/Game DVR" | `HKCU\...\GameDVR\AppCaptureEnabled=0` e `HistoricalCaptureEnabled=0` (hive HKCU **[inferido]**) | `RegSetValueExW` HKCU, sem admin |
| **Turn off NTFS Last Access Time stamp** | strings `fsutil.exe`, `behavior`, `query`, `disableLastAccess`, "DisableLastAccess = 1/0" | **spawn** `fsutil behavior set disableLastAccess 1` | Sem spawn: `HKLM\SYSTEM\CurrentControlSet\Control\FileSystem\NtfsDisableLastAccessUpdate` (é o que o fsutil grava; **não** aparece no exe) |
| **Turn off SuperFetch** | wide `SysMain`, `sc.exe`, `queryex`, `config`, `start=disabled`, `start=auto`, `start`; saída `RUNNING/START_PENDING/STOP_PENDING`; "Command to %1% SuperFetch returned %2%" | **spawn** `sc.exe config SysMain start=disabled` (+ stop) via `cmd.exe` | `OpenSCManagerW` + `ChangeServiceConfigW(SERVICE_DISABLED)` + `ControlService(STOP)` |
| **Turn off Windows file indexing** | wide `WSearch`, `start=delayed-auto`, `start=disabled`; "Command to %1% Windows Search returned %2%" | `sc config WSearch start=disabled`; reverte p/ `delayed-auto` | `ChangeServiceConfigW` (+ `SERVICE_CONFIG_DELAYED_AUTO_START_INFO`) |
| **Enable Ultimate performance power plan** | wide `Ultimate Performance (ExitLag)`; GUIDs `{e9a42b02-d5df-448d-aa00-03f14749eb61}` (Ultimate) e `{381b4222-f694-41f0-9685-ff5bb260df2e}` (Balanced); `PowerDuplicateScheme`, `PowerWriteFriendlyName`, `PowerSetActiveScheme`, `PowerGetActiveScheme`; `powercfg /lastwake` só diagnóstico de wake source | Duplica o esquema Ultimate com o nome "Ultimate Performance (ExitLag)", ativa, salva o esquema anterior ("Save power plan failed") e restaura ao desligar | `PowerDuplicateScheme` -> `PowerWriteFriendlyName` -> `PowerSetActiveScheme`; restaurar GUID salvo. Sem `powercfg` |
| **Core Parking** | wide `SYSTEM\%1%\Control\Power\PowerSettings\54533251-82be-4824-96c1-47b60b740d00\0cc5b647-c1df-4637-891a-dec35c318583` (`%1%` = `CurrentControlSet`/`ControlSetNNN`), valor `ValueMax`; "Found Core Parking key", "Cores Parking... Enabling/Disabling" | Setting `CPMINCORES` (processor/min cores): mexe em `ValueMax` no registro (dado **não determinado**) | Preferir `PowerWriteACValueIndex/DCValueIndex` (subgrupo `54533251-...`, setting `0cc5b647-...` = 100) + `PowerSetActiveScheme` |
| **Game priority / Max game priority** ("Prioritizes processing for the game") | "Priority of process %1% is set to high / already high / reverted to original value", "Process %1% is not a game, so priority won't be changed"; `Get/SetPriorityClass` | `SetPriorityClass(HIGH_PRIORITY_CLASS)` no jogo, restaura depois | `SetPriorityClass` |
| **Lower background services priority** | lista wide: `backgroundtransferhost.exe`, `core.exe`, `tiworker.exe`, `trustedinstaller.exe`, `compattelrunner.exe`, `searchindexer.exe`, `msmpeng.exe`, `onedrive.exe`; "decreased to IDLE", "Access to process ... is denied"; `CreateToolhelp32Snapshot` | `SetPriorityClass(IDLE_PRIORITY_CLASS)` nesses processos | Idem; opcional EcoQoS (`SetProcessInformation(ProcessPowerThrottling)`) |
| **Reduce input lag** | `[InputLagReducer]` "Original timer resolution: %1% ms", "Timer resolution set"; `NtQueryTimerResolution/NtSetTimerResolution`; `SetProcessInformation`; `GetWindowBand` (z-band do overlay) | Timer resolution mínima + desliga power throttling do processo | `NtSetTimerResolution` / `timeBeginPeriod`; `SetProcessInformation` |
| **Reduce keyboard input time** | `[FastKeys] Cannot get original filter keys / set custom filter keys`; `SystemParametersInfoW` | `SPI_GETFILTERKEYS` / `SPI_SETFILTERKEYS` (valores da `FILTERKEYS` **não determinados**) | `SystemParametersInfoW(SPI_SETFILTERKEYS)`, guardar original |
| **Turn off Network power-saving (EEE)** | wide WMI `ROOT\StandardCimv2`, `SELECT Name FROM MSFT_NetAdapterAdvancedPropertySettingData WHERE RegistryKeyword = "*EEE"`; `SYSTEM\CurrentControlSet\Control\Class\{4d36e972-e325-11ce-bfc1-08002be10318}\`; `Win32_NetworkAdapter WHERE PhysicalAdapter=1` (`Enable`/`Disable`); "[EEE] Supported at least by...", "Cannot set registry key for device" | Grava `*EEE`=0 na subchave de classe da NIC física e reinicia o adaptador via WMI | Registro direto (`*EEE`=`0`) + `SetupDiSetClassInstallParams(DICS_PROPCHANGE)` p/ reiniciar sem WMI |
| **Unreal Engine booster** | `Config/{Windows,WindowsClient,WindowsNoEditor}/GameUserSettings.ini`, pastas `Saved`, backup `.exitlag-backup.json`, tmp `.exitlag-tmp`, `[IniPatcher]`; preset JSON `section/key/value` + `scalability_overrides` por `app_id`; "Refusing to apply: an un-reverted backup already exists" | Edita INI UE4/5 com backup/revert; **preset vem do servidor** | Parser INI + backup atômico, tocar só chaves presentes |
| **GPU performance profile** | NVIDIA `nvml.dll` (`nvmlDeviceSetPowerManagementLimit`, `nvmlDeviceSetGpuLockedClocks`/`ResetGpuLockedClocks`, `GetMaxClockInfo`); AMD `atiadlxx.dll` (`ADL2_Overdrive8_Setting_Set`, `ADL2_OverdriveN_PowerLimit_Set`, `SystemClocksX2_Set`) | Power limit no máximo + clock travado; admin; reset ao desligar | Carga dinâmica de `nvml.dll`; opt-in (risco) |
| **Windows Update Delivery Optimization** (desliga P2P ao conectar) | wide `SOFTWARE\Microsoft\Windows\CurrentVersion\DeliveryOptimization\Settings` (`DownloadMode`) e `...\DeliveryOptimization\Config` (`DODownloadMode`); "Only Microsoft's servers will be used instead" | `DownloadMode`/`DODownloadMode = 0` (HTTP only) **[inferido]** | `RegSetValueExW` HKLM |

### 2.7 Limpeza de RAM (RAM Cleaner)
- Evidência (`[RamCleanup]`, `[RamOptimizer]`, na ordem do binário): `SeIncreaseQuotaPrivilege` -> `SeProfileSingleProcessPrivilege` -> lista de processos -> "Whitelist: %1% items" -> **"Optimizing processes working set"** (`K32EmptyWorkingSet` por PID; `QueryFullProcessImageName/GetModuleFileNameEx` p/ casar whitelist/exceções) -> **"Optimizing system working set"** -> **"Flushing the modified pages list"** -> **"Combining pages"** -> "Completed: success, N MB saved" (delta de `GlobalMemoryStatusEx`). Usa `NtQuerySystemInformation` + `NtSetSystemInformation`.
- Mapeamento **[inferido; convenção Sysinternals/RAMMap, classes não confirmadas em disassembly]**: "system working set" = `NtSetSystemInformation(SystemFileCacheInformation=0x15)` (equiv. `SetSystemFileCacheSize(-1,-1)`); "modified pages list" = `SystemMemoryListInformation=0x50` + `MemoryFlushModifiedList=3`; "Combining pages" = `SystemCombinePhysicalMemoryInformation=0x82`. **Purge da standby list (`MemoryPurgeStandbyList=4`) NÃO aparece nos logs.**
- Gatilhos (`routineType`): `GameConnection` (ao conectar jogo), `NeedsCleaningDetection` (automático), manual; "Periodic working set scan", frequência configurável; allowlist por app (`PC_BOOST_RAM_CLEANER_ALLOWLIST_APP_ADD/REMOVE`). pt-BR: "Limpeza de RAM", "%1 MB de RAM agora estão livres.", "Limpeza automática concluída!", "Limpar Exceções".
- **G2 Boost**: habilitar `SeProfileSingleProcessPrivilege` e `SeIncreaseQuotaPrivilege` (`AdjustTokenPrivileges`); `EnumProcesses` + `OpenProcess(PROCESS_SET_QUOTA|PROCESS_QUERY_INFORMATION)` + `EmptyWorkingSet`; `NtSetSystemInformation` via `GetProcAddress("ntdll")` (ou crate `ntapi`). Medir com `GlobalMemoryStatusEx`. Aviso: trim agressivo gera page-faults depois.

### 2.8 DNS Optimizer
Lista embutida (ordem do binário; campos `primaryIp`, `secondaryIp`, `isAnyCast`, `description`, `location`, `isSystemDns`, `queriesSent/Success/Error`, `systemDnsSpeedPerc`):

| Provedor | Primário | Secundário |
|---|---|---|
| Google | 8.8.8.8 | 8.8.4.4 |
| Cloudflare | 1.1.1.1 | 1.0.0.1 |
| OpenDNS | 208.67.222.222 | 208.67.220.220 |
| Verisign | 64.6.64.6 | 64.6.65.6 |
| Adguard | 94.140.14.14 | 94.140.15.15 |
| NextDNS | 45.90.28.0 | 45.90.30.0 |
| Control D | 76.76.2.0 | 76.76.10.0 |
| Comodo Secure | 8.26.56.26 | 8.20.247.20 |
| Neustar | 156.154.70.5 | 156.154.71.5 |
| Quad9 | 9.9.9.9 | 149.112.112.112 |

Rótulos soltos: "Australia (APNIC)", "California (Google LLC)", "California (Cisco)", "Berkeley", "Local DNS server" (o DNS atual do Windows entra no ranking).
- Teste: resolve o domínio `exitlag.net` em cada servidor com socket próprio (`[DNS] Start test (domain: %1)`, "Waiting for remaining DNS replies"), mede tempo e erros, ranqueia, aplica o melhor. Há também `[DnsOverride]` (redireciona DNS de apps otimizados; desabilitado se o Optimizer estiver em uso ou por país) e sniff de respostas (`[DNS] Sniffed IPv4/IPv6`).
- Aplicação: `GetInterfaceDnsSettings/SetInterfaceDnsSettings` (iphlpapi, Win10 2004+/Win11; suporta **DoH**: "DoH is not supported in this Windows version") nos adaptadores IPv4 ativos ("Set DNS servers to %1%,%2%"); reset: "Reset DNS servers of %1%". Não funciona em IPv6-only. `DnsFlushResolverCache` (dnsapi) depois; string solta `/flushdns` (fallback `ipconfig /flushdns`).
- pt-BR: "DNS Optimizer", "Classificar servidores DNS", "Sua conexão agora está usando o servidor DNS mais rápido disponível.", "As configurações de DNS foram redefinidas para o padrão do Windows."
- **G2 Boost**: medir com UDP próprio (`hickory-resolver`/`DnsQueryEx`), aplicar com `SetInterfaceDnsSettings` (`DNS_INTERFACE_SETTINGS`, flag `DNS_SETTING_NAMESERVER`) e `DnsFlushResolverCache`; guardar o NameServer anterior p/ reset.

### 2.9 Limitador de tráfego (Traffic Shaper)
- Evidência: `[TrafficShaper]` (27 logs; "Disable to prevent Dr.Web conflict"), telas `TrafficShaper/{BandwidthDrawer,Exceptions}`, `traffic_shaper.priorities.reset`, `traffic_shaper.button.speedtest_retry_clicked`. API NetFilter SDK: `nf_addFlowCtl/modifyFlowCtl/deleteFlowCtl/getFlowCtlStat/setFlowCtlQueueParams/setTCPFlowCtl/setUDPFlowCtl`. Roda speedtest p/ calibrar banda, prioridade por app ("Prioridade %1", reset p/ secundária) e "exceções". Requer driver nfsdk.
- pt-BR: "Traffic Shaper desativado", "Escolher aplicativos para ignorar limites de velocidade", "Reinicie seu jogo para aplicar as alterações".
- **G2 Boost**: sem driver, só QoS do Windows (`HKLM\SOFTWARE\Policies\Microsoft\Windows\QoS\<nome>`: `Application Name`, `Throttle Rate`, `DSCP Value`) ou API QoS/qWAVE; limitação real por app exige WFP/driver.

### 2.10 Reset de rede do Windows
`[ResetNetworkConfig]`; strings `netsh %1`, `ipconfig`, `/release`, `/renew` ("Renewing IP addresses"); exige reinício. Argumentos exatos do `netsh` são montados em runtime (não em claro). **G2**: `IpReleaseAddress/IpRenewAddress`, `DnsFlushResolverCache`; reset de winsock/IP stack não tem API simples (único caso aceitável de spawn).

### 2.11 Outros achados úteis
- Checks de segurança do cliente: flag `RUNASADMIN` em `HKCU/HKLM\Software\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Layers`, exe em local seguro (`%ProgramFiles%`/`System32`), token `uiAccess/elevated`.
- Reboot: `SeShutdownPrivilege` + `InitiateSystemShutdownW` (após reparo de WMI).
- Auth: OAuth2 loopback (`http://localhost:{{PORT}}/callback`), Google/Discord, reCAPTCHA (WebView2), credenciais protegidas com **DPAPI**.
- Overlay: CTRL+SHIFT+I; jogos suportados na lista (ex.: `TL.exe`, `Aion2.exe`, `PioneerGame.exe`, `blackdesert32/64.exe`, `marathon.exe`).

### 2.12 Adendo 2026-10-18 (fases 3A/3B) — busca de bytes, sem disassembly
- **Energia** (`[PowerConfig]`): além do Ultimate e do Core Parking, grava **wake timers AC e DC** ("Failed to write AC/DC wake timer setting") num `applyPlanSettings`; `FpsBoostController::disableCoreParking`, flag `throttleOnSlow`. GUIDs presentes (texto UTF-16): subgrupo processador, `CPMINCORES`, Balanced, Ultimate. **Ausentes** (texto e binário): USB selective suspend, PCIe ASPM, sleep/hibernar, desligar vídeo/disco, min/max processor state, EPP, boost mode — o ExitLag não edita esses (limite: só o exe; config remota não vista).
- **Processos**: `NtSuspendProcess`/`TerminateProcess` vêm do Crashpad (`ScopedProcessSuspend`). Não há gerenciador de tarefas nem "encerrar processo" — só prioridade (§2.6).
- **Motion/paleta do QML embutido**: `NumberAnimation` 300 ms `Easing.OutCubic`; hover = `Behavior on color { ColorAnimation { Easing.OutQuad } }`; 800 ms `InOutQuad` só em animação decorativa (busca/onboarding). Paleta = rampa de ~15 neutros azul-acinzentados (fundo quase preto → texto quase branco), **1 accent** (cor da marca) e cada cor de status pareada com um fundo tingido muito escuro (~10%) — padrão "chip tingido". Para a 3A vale o padrão, **não os valores** (o accent é marca deles).

## 3. SpaceSniffer 2.1.0.21 (x64)

| Campo | Valor |
|---|---|
| Arquivo | `SpaceSniffer.exe`, 8.690.994 B, **sem assinatura** |
| VersionInfo | ProductName=SpaceSniffer, FileVersion=2.1.0.21, CompanyName=Uderzo Software |
| Tecnologia | **Delphi/C++Builder (VCL)**: exports `_ZTR10TfrmMain`, `TfrmView`, `TFilterThread`, `TWatchThread`, `TSnifThread`; tinyxml embutido; ZStream |
| Docs na pasta | `Release Notes.txt`, `User Manual.pdf`, `Quick Start.pdf`, `Disclaimer.txt` |

**Scan (APIs)**
- Enumeração: `FindFirstFileExW`/`FindFirstFileW`/`FindNextFileW`; tamanho em disco: **`GetCompressedFileSizeW`** ("Unable to get compressed file size for"); `GetFileAttributesW`, `GetFileTime`, `GetDiskFreeSpaceExW`, `GetLogicalDrives`, `GetDriveTypeA`, `GetVolumeInformationA`; prefixos `\\?\` e `\\?\UNC\`; reparse points sinalizados ("reparse point," no log) para não segui-los.
- Dois tamanhos: **disk/cluster** (padrão) e **logical** (`logicalsize`, `clustersize`).
- **ADS**: `ntdll!NtQueryInformationFile` ("Unable to obtain ADS info from NtQueryInformationFile"), sufixo `::$DATA`, opção `scanADS`.
- **Não usa MFT/USN** (sem `DeviceIoControl`/`FSCTL_*`): varredura de diretórios, por isso mais lento que WizTree. Threads: `threads.snif` (`pool.snif.maxconcurrency`), `threads.filter`, `threads.watch`. `AdjustTokenPrivileges`/`LookupPrivilegeValueA` importados (privilégio exato **não determinado**).
- Watch: `ReadDirectoryChangesW` (eventos `ADDED/REMOVED/MODIFIED/RENAMED`); opção `watchDog` desativável (pede restart).
- Nós: `TDriveNode`, `TFolderNode`, `TFileNode`, `TFreeSpaceNode` ("Free Space"), `TUnknownSpaceNode` ("Unknown (not yet scanned) Space"), `TMissingDataNode`. `TRecycleCan` é pool interno de objetos (não importa `SHEmptyRecycleBin`/`IFileOperation`); abrir/excluir via `ShellExecuteW`.

**Render**
- Imports: **`D2D1!D2D1CreateFactory` + `DWRITE!DWriteCreateFactory`** (Direct2D/DirectWrite; as release notes dizem "DirectDraw" desde 2.0.1) com GDI de apoio (`BitBlt`, `StretchBlt`, `CreateDIBSection`, `AlphaBlend`, `GradientFill`, `TransparentBlt`), `DWMAPI`, `UXTHEME` buffered paint, `WINDOWSCODECS`. Sem ddraw/d3d9/d3d11.
- Layout treemap próprio (`treemap.layout`, "computed chunk size"; 2.0.5 corrigiu erro de ponto flutuante). Config em `SpaceSnifferConfig.xml` (tinyxml, UTF-8): `refreshDelayMs, zoomTimeMs, minElementSizePx, showFreeSpace, showUnknownSpace, initialStyle, useRealZoom, useDynaSmooth, useSorting, proportions, initialDetailLevel, levelContrast, borderContrast, hilightShadow, hilightHaloLevels, flashElement, driveColor, folderColor, fileColor, freeColor, unknownColor, fileGroups`.
- Grupos padrão: Video/Movies (`qt;mov;mpeg;mpg;mpe;m1s;mp4;avi;mts;vob;wmv;rm;3gp;mkv`), Executables/System files (`exe;dll;sys;ini;msi;log`), Archives/Disk images (`zip;7zip;rar;iso`), Pictures/Images (`jpg;jpeg;gif;png;tif;tiff;nef;raw;rw2;cr2;bmp;psd`), Audio/Music (`mp2;mp3;wav;ogg;aif;alp;au;3gp;ac3;amr;aac;m4a;wma;aiff;flac;voc`).

**Filtros (sintaxe nas strings)**: máscara `*.jpg` (`?*`, `;` separa, `|` inicial nega); tamanho `>100kb` (`b|kb|mb|gb|tb`, prefixos `disksize|clustersize|filesize|logicalsize|size`); idade `<10s`, `>1month` (`seconds|minutes|hours|days|weeks|months|years`, prefixos `create|access|modify`, padrão modify); tag `:red|:yellow|:green|:blue|:all`, `:tag:r+y`; atributo `:attr:[archive|system|readonly|hidden|compressed|encrypted|offline|temporary|notindexed|sparse|alternatedatastream]` (abrev. `a|s|r|h|c|e|o|t|ni|sp|ads`, com `+`/`-`); classe `:class:Audio/Music`; pasta `\temp`, `|\progr*`. Exemplos: `*.jpg;>100kb;>2weeks`, `|*.exe;|*.dll;\*temp*;|\*windows*`. Tags com CTRL+1..4, CTRL+0 limpa.

**Snapshot / export**
- `.sns` = "SpaceSniffer binary snapshot", versionado ("Bad encoding version", "Bad structure (parent node must be a drive)", "Wrong path segment (corrupted snapshot file)"); v1.3.0.2: textos em **Base64**; v2.0.3: Unicode; v2.1.0.21 endureceu o loader contra `.sns` malicioso. Também: "SpaceSniffer scan dump", "Disk Usage output (only folders)", export XML, export configs nomeados.
- CLI: `scan "<path>"`, `load "<file>"`, `filter "<filter>"`, `save "<path>"` (snapshot), `export "<name>" "<path>"`, `autoclose`, meta `@delim=X`, `@parser=OS`.
- **G2 Boost (mapa de disco)**: `FindFirstFileExW(FindExInfoBasic, FIND_FIRST_EX_LARGE_FETCH)` em pool de threads; com admin, MFT/USN (`FSCTL_ENUM_USN_DATA`) como WizTree; tamanho em disco via `GetCompressedFileSizeW` ou `NtQueryInformationFile(FileStandardInformation.AllocationSize)`; ADS via `FindFirstStreamW`; watch via `ReadDirectoryChangesW`; treemap squarified em Direct2D (crate `windows`).

## 4. O que NÃO foi possível determinar
1. **Dados exatos** gravados: `Perflib\Disable Performance Counters` (esperado 1), `DownloadMode`/`DODownloadMode`, hive e demais valores de `GameDVR`, valor de `ValueMax` do Core Parking, struct `FILTERKEYS`, valor de `*EEE`. Só os nomes de chaves/valores estão em texto; os dados são imediatos de código (exigiria disassembly).
2. Classes exatas de `NtSetSystemInformation` do RAM Cleaner (0x15/0x50/0x82 são convenção) e se há purge de standby list.
3. Endpoint real do **speedtest**, de IP-info e do probe de conectividade (vêm de RemoteConfig/servidor); lista de **servidores de jogo**, jogos e regras de porta (protobuf via rede).
4. Driver do modo **WFP** (nome do `.sys`, GUIDs de callout) e driver **nfsdk** (`.sys` ausente de `D:\ExitLag`).
5. Comandos `netsh` exatos do "Reset de rede".
6. Rótulos pt-BR do tab "Ajustes do sistema" por tweak (catálogo lido parcialmente; os títulos EN do exe batem com as chaves `PC_BOOST_*`).
7. Protocolo bridge/FS (formato dos pacotes, cripto) — fora do escopo.
8. SpaceSniffer: privilégio exato solicitado, layout byte-a-byte do `.sns`; ausência de MFT inferida só pela falta de `DeviceIoControl`.
9. Nada foi executado: ordem real de chamadas e valores de retorno em runtime não verificados.
