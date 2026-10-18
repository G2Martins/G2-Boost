# CPU-Z 3.01 e Speccy 1.34 — análise estática (clean-room)

**Última revisão:** 2026-10-18 (§1.1 correções da segunda passada + zona ACPI medida; §4.1 energia/processos) · Método: inventário de arquivos, cabeçalhos PE (imports, version info, recursos), strings e eventos do System log; **nada executado, nada desmontado, nenhum driver gravado em disco**. Fontes: `C:\Program Files\CPUID\CPU-Z`, `C:\Program Files\Speccy`, prints do dono (2026-10-15). Feito por sub-agente; palpites marcados. O que extrair como *ideia* para o Meu PC está na seção 4.

## 1. O fato central: os dois usam driver de kernel

| App | Driver embutido (RT_RCDATA) | Assinatura | Serviço | Instalado em |
|---|---|---|---|---|
| CPU-Z 3.01 (`cpuz.exe`, manifesto `highestAvailable`) | `cpuz.sys` 1.0.6.5 (2026-05) | Microsoft WHCP (x86/x64) | `cpuz165` | `C:\ProgramData\CPUID Software\cpu-z\<nome aleatório sem extensão>` (apaga ao sair) |
| Speccy 1.34 (`Speccy64.exe` + `cpuidsdk.dll` 1.2.9.9 "CPUID DLL SDK") | `cpuz.sys` 1.0.5.8 / 1.0.5.2 / 1.0.4.9 | WHCP / DigiCert EV | `cpuz158` | `C:\Windows\Temp\cpuz158\cpuz158_x64.sys` |

Imports do driver: `MmMapIoSpace`, `HalGetBusDataByOffset`/`HalSetBusDataByOffset` (config space PCI), `READ/WRITE_PORT_*` (x86), `rdmsr` por instrução [palpite]. Ou seja: memória física, portas de I/O e MSR — a mesma classe de primitiva de BYOVD. Na máquina do dono HVCI + blocklist de drivers vulneráveis + Secure Boot estão ativos e os drivers carregaram (evento 7045, sem erro CodeIntegrity). Evidência no exe: `CreateServiceW`/`StartServiceA`/`DeleteService`, string `SeLoadDriverPrivilege`, mensagens "CPUID init driver error %u"; o readme do CPU-Z diz que `PCI=0` desliga chipset/SPD/sensores.

**Decisão G2 Boost: sem driver.** Exigiria certificado EV + atestação WHCP, é o padrão que o Kaspersky flagra, e viola menor privilégio. HWiNFO/LibreHardwareMonitor não estão instalados aqui (integração opcional só quando existirem).

### 1.1 Correções da segunda passada (2026-10-18, REA `inspect_pe_resources` + strings + busca de bytes; nada executado)

- **Assinatura:** `Speccy64.exe`, `Speccy.exe` e `cpuidsdk.dll` são assinados por **Gen Digital Inc.** (dona atual da Piriform). Manifesto `asInvoker`: abre sem admin.
- **Drivers em dois lugares:** `Speccy64.exe` RT_RCDATA 107..111 = `cpuz149` (x86/x64/ia64, assinado por CPUID S.A.R.L.U., EV, OriginalFilename `cpuz.sys`); `cpuidsdk.dll` RT_RCDATA 106..111 = `cpuz158` e `cpuz152`. Quem instala o serviço é a **DLL** (`CreateServiceW`, `OpenSCManagerA`, `StartServiceA`, `DeleteService`, `DeviceIoControl`, `SeLoadDriverPrivilege`); IOCTLs na família `0x9C40xxxx` (mesma do CPU-Z), sem mapear qual é MSR/PCI/memória.
- **Temperatura da CPU = driver** (strings `CPU DTS`, `Package`, `SIO PECI`, rótulos de Super I/O ITE/Nuvoton/Fintek). Os MSRs não aparecem como imediato de código (vão como dado ao driver). AMD: bytes de `0x59800`/`0x59954` na DLL sugerem o caminho SMN do Tctl [palpite fraco]. GPU por NVAPI/ADL, disco por SMART. **Nenhum uso de WMI/ACPI térmico.** Sem admin o driver não carrega, então sem temperatura de CPU/placa [inferido, não executado].
- **Sem licença de redistribuição** do `cpuz.sys` na pasta: reusar está fora (também é primitiva de BYOVD).
- **Zona térmica ACPI medida nesta máquina (i5-10400, sem admin):** o contador PDH tem nome localizado (`\Informações de Zona Termal(*)\Temperatura`; em código usar `PdhAddEnglishCounterW` com `\Thermal Zone Information(*)\Temperature`). Valores **constantes** em leituras seguidas: `_TZ.TZ00` = 28 °C, `_TZ.TZ10` = 17 °C. São zonas falsas da placa, **inúteis como temperatura de CPU** aqui; `MSAcpi_ThermalZoneTemperature` exige admin e lê a mesma zona; `Win32_TemperatureProbe` é um sensor fictício sem leitura.
- Notas completas fora do repo: `D:\dev\g2boost-experiments\speccy-rea\notes.md`.
- **Opções para mostrar a temperatura do pacote** (decisão do dono pendente): (a) seguir com HWiNFO/LibreHardwareMonitor abertos (hoje, sem driver nosso); (b) **PawnIO** (driver assinado do namazso, usado pelo LibreHardwareMonitor desde 2025) como opt-in com admin: Intel MSR `0x1B1` + TjMax de `0x1A2`, AMD SMN; checar licença dos módulos e se redistribuímos o instalador; (c) zona ACPI só como último recurso e só se o valor variar. `cpuz.sys` e WinRing0 descartados.

## 2. Funcionalidade × mecanismo × o que fazemos

| Funcionalidade (CPU-Z / Speccy) | Como eles fazem | Driver? | G2 Boost |
|---|---|---|---|
| CPU: nome, família/modelo/stepping, instruções, caches, núcleos/threads, HT, virtualização | CPUID + `GetLogicalProcessorInformationEx` + registro `CentralProcessor\0` | não | **feito** (`hardware::CpuDetail`, 6.5) |
| Microcode | registro `Update Revision` | não | **feito** (0xF6 aqui) |
| Clock efetivo / bus / multiplicador / VID | MSR [palpite]; PDH `% Processor Utility` | bus/multi/VID: sim | clock via `CallNtPowerInformation` (**feito**); bus/multi/VID não |
| Temperatura de CPU (por núcleo), TJMAX | MSR/PECI | **sim** | **não** (`MSAcpi_ThermalZoneTemperature` = acesso negado sem elevação; PDH Thermal Zone inexistente aqui) |
| Potência do pacote (Package Power) | RAPL via MSR | sim | **feito via PDH `\Energy Meter(RAPL_Package0_PKG)\Power`** (user-mode, ~38–42 W aqui) |
| Voltagens, ventoinhas, temp. da placa (Super I/O ITE/Nuvoton/Fintek) | portas LPC/SMBus | **sim** | não |
| SPD / XMP / EXPO / PMIC / tabela JEDEC / timings reais | SMBus + config PCI | **sim** | parcial: SMBIOS tipo 17 (tamanho, tipo, velocidade SPD × configurada, fabricante, part number, slots) — **feito**; velocidade nominal inferida do part number (`CMK16GX4M1E3200C16` → 3200) para sugerir "XMP desligado?" |
| Placa-mãe, BIOS (marca/versão/data), chassi | SMBIOS tipos 0/1/2/3 | não | **feito** |
| Chipset / southbridge / LPCIO | config PCI + tabela CPUID | config bruto: sim; IDs: não | backlog: SetupAPI `PCI\VEN_&DEV_` + tabela própria pequena |
| Link PCIe da GPU (atual/máximo) | `SetupDiGetDevicePropertyW` `DEVPKEY_PciDevice_CurrentLinkSpeed/Width` (verificado: RTX 3050 = 3.0 x8 atual, 4.0 x16 máx) | não | NVML `nvmlDeviceGetCurrPcieLink*` **feito** (NVIDIA); DEVPKEY genérico = backlog |
| GPU: clocks, temp, potência, driver, VRAM, uso | NVML/NVAPI, ADL, IGCL, Level Zero; DXGI/DXCore; PDH GPU | não (driver do fabricante) | NVML **feito** (temp, clocks core/mem, potência, limite, driver); AMD ADLX / Intel IGCL = backlog |
| Monitores (nome, resolução, Hz) | `EnumDisplayDevices`/`EnumDisplaySettings` | não | `QueryDisplayConfig` **feito** |
| Discos: modelo, barramento, SSD/HD, tamanho | `DeviceIoControl` em `\\.\PhysicalDriveN` | não | **feito** (fase 6) |
| S.M.A.R.T. (atributos, temp, horas, TBW), NVMe health | Speccy: `AtaSmart.cpp` derivado do CrystalDiskInfo (`IOCTL_ATA_PASS_THROUGH`, SAT, pontes USB) | não, mas **admin** | backlog (elevado): `IOCTL_STORAGE_QUERY_PROPERTY` + ATA/NVMe pass-through |
| Áudio, periféricos, rede, Wi-Fi | WMI `Win32_SoundDevice`, SetupAPI, `GetAdaptersAddresses`, WlanAPI | não | backlog user-mode |
| TPM, Secure Boot, AV/Firewall, UAC | `Tbsi_GetDeviceInfo`, `GetFirmwareEnvironmentVariable`, `root\SecurityCenter2` | não | TPM/Secure Boot **feito**; AV/Firewall backlog |
| WinSAT | **nenhum dos dois usa** | — | **feito** (`winsat.rs`: WMI `Win32_WinSAT` para ler, COM `CInitiateWinSAT` para rodar, admin) |
| Bench CPU (single/multi) + Stress + comparar | algoritmo e base de referência proprietários (`valid.cpuid.com/bench`) | não | backlog: bench próprio (kernel fixo, "pontos G2", referências medidas por nós) — nunca comparável aos números do CPU-Z |
| Relatório TXT/HTML/XML, snapshot `.speccy` | formatos próprios | não | relatório próprio (TXT/JSON) **6.5** |
| Validação / Publish Snapshot online | `valid.cpuid.com`, `license.piriform.com` | não | **não replicar** (serviço proprietário) |


## 4. Strings úteis (registro/WMI/PDH que eles consultam)
`HARDWARE\DESCRIPTION\System\CentralProcessor\`, `HARDWARE\DESCRIPTION\System\BIOS`, `Control\Class\{4d36e968…}` (`DriverVersion`), `Control\SecureBoot\State`, `SOFTWARE\Khronos\OpenCL\Vendors`, `SOFTWARE\Khronos\Vulkan\Drivers`, `Enum\DISPLAY\…\Device Parameters\EDID`; WMI `Win32_PhysicalMemory`, `Win32_VideoController`, `Win32_DiskDrive`, `Win32_NetworkAdapter(Configuration)`, `Win32_Printer`, `root\wmi MSSmBios_RawSMBiosTables`, `root\SecurityCenter2`; PDH `\GPU Adapter Memory(*)\Dedicated Usage`, `\GPU Engine(*)\Running time`, `\Processor Information(*)\% Processor Utility`; DLLs `nvapi64`, `nvml`, `atiadlxx`, `ControlLib`, `ze_loader`, `DXCore`, `hid`, `wlanapi`, `tbs`.

## 4.1 Energia e processos (insumo da fase 3B) — 2026-10-18, busca de bytes
- **Speccy** lê o plano ativo e seus timeouts: `PowerGetActiveScheme` + `PowerReadFriendlyName/PowerReadDescription` + `PowerReadACValue/PowerReadDCValue` com GUIDs binários de suspensão (`STANDBYIDLE`), desligar vídeo (`VIDEOIDLE`) e disco (`DISKIDLE`). Só leitura. Lista processos (Toolhelp, "Unable to take a snapshot of the running processes!") e serviços (`EnumServicesStatusExW`), também só leitura.
- **CPU-Z**: `PowerEnumerate/PowerGetActiveScheme/PowerReadFriendlyName` → só o nome do plano ativo.
- Nenhum dos dois grava setting de energia; `TerminateProcess` aparece no import, sem UI conhecida de encerrar processo **[não verificado]**.

## 5. Estimativa de FPS e IA (pedido do dono)
Nem CPU-Z nem Speccy estimam FPS. Desenho em [../core/ai-advisor.md](../core/ai-advisor.md): classe de hardware local + números com fonte via busca web restrita a sites de benchmark, dentro do chat com IA (fase 9). Temperaturas para a IA: GPU (NVML) e discos (SMART, elevado); CPU só se HWiNFO/LHM existirem.
