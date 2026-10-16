# Painel, Meu PC e Monitor

**Última revisão:** 2026-10-15 · Código: `crates/core/src/hardware.rs`, `monitor.rs`, `restore.rs`, `recommend.rs`; shell `src-tauri/src/lib.rs` (`Dashboard`); UI `src/pages/dashboard.tsx`, `src/components/ui/kpi-card.tsx`, `live-line.tsx` · Origem da ideia: [../references/galdino-optimizer.md](../references/galdino-optimizer.md).

## Painel (rota inicial)

| KPI | Fonte | Ação |
|---|---|---|
| **Pontuação de otimização** | `recommend::score` = tweaks recomendados **para este PC** ativos ÷ recomendados (indisponíveis não contam) | **Boost agora · N** aplica os pendentes pelo fluxo normal (backup, admin); sem admin, pula os admin e diz quantos. Com `restore_before_preset` e elevado, cria ponto de restauração antes. |
| **Pontos de restauração** | `restore::status` (WMI `root\default:SystemRestore` + `RPSessionInterval`/`DisableSR`) | **Criar ponto** (`SRSetRestorePointW`, admin). Windows limita a 1 por `SystemRestorePointCreationFrequency` min (padrão 1440): a UI diz "pulado" quando a contagem não sobe. Proteção desligada → `protection_off` com instrução. |
| **Arquivos para limpar** | `cleanup::preview` das categorias `default_on` (só medição) | **Limpeza** → rota Limpeza |
| **Memória RAM** | último `monitor:tick` | **Limpar memória** (purge, admin) |

**Ações recomendadas:** `score.pending` (até 6) com categoria e Aplicar; "Ver todas" → Boost. **Monitor em tempo real:** card com abas CPU/GPU/RAM/Disco/Rede, gauge + linha de 60 s + chips (clock, núcleos, temp. GPU, temp. CPU "exige driver", disco MB/s, rede ↓↑, RAM, GPU).

## Recomendação por hardware (`recommend.rs`)

`Facts` = SSD do sistema (seek penalty), notebook (chassi SMBIOS ou bateria), vendor da GPU, `HwSchMode` presente, Game Pass/Xbox app (pacotes em `%LOCALAPPDATA%\Packages`), Outlook (registro). Regras:

| Tweak | Não recomendado quando | Motivo (i18n `boost.reason.*`) |
|---|---|---|
| `svc_sysmain` | HD mecânico / tipo desconhecido | `hdd` / `unknown_disk` |
| `power_plan` | notebook | `laptop` |
| `hags` | driver sem `HwSchMode` | `hags_unsupported` |
| `svc_xbox` | sempre (já Manual; Game Pass) | `game_pass` / `preference` |
| `svc_search` | Outlook instalado | `outlook` |
| `mouse`, `svc_dps`, `svc_geolocation` | sempre (preferência) | `preference` |
| `hvci_off` | sempre (segurança) | `security` |
| betas | sempre | `beta` |

O motivo aparece como badge no card do Boost. Evidência: [../references/windows-services-research.md](../references/windows-services-research.md).

## Monitor (`monitor.rs`)

PDH por nome em inglês (`PdhAddEnglishCounterW`): `\Processor Information(_Total)\% Processor Utility` (fallback `% Processor Time`), `\GPU Engine(*)\Utilization Percentage` (soma por `engtype`, maior = GPU %, como o Gerenciador de Tarefas), `\GPU Adapter Memory(*)\Dedicated Usage`, `\PhysicalDisk(_Total)\% Disk Time` e `Disk Bytes/sec`. Clock: `CallNtPowerInformation(ProcessorInformation)` (máx. `CurrentMhz`). Rede: delta de `GetIfTable2` (interfaces up, sem loopback). Temp. GPU: NVML (`nvml.dll`, user-mode, só NVIDIA; AMD/Intel = backlog ADLX/IGCL). **Temp. de CPU não existe sem driver de kernel** (MSR) — mostrado como "—" com o motivo. Thread única no shell, 1 amostra/s, refcount por página (`monitor_start/stop`).

## Meu PC (`hardware.rs`)

| Dado | API |
|---|---|
| Computador | `GetComputerNameExW` |
| Sistema | `RtlGetVersion` (família) + registro `EditionID`/`DisplayVersion`/`CurrentBuild`/`UBR` (ProductName mente no Win11) |
| CPU, núcleos | registro `ProcessorNameString`; `GetLogicalProcessorInformationEx(RelationProcessorCore)`; `GetActiveProcessorCount` |
| RAM | `GetPhysicallyInstalledSystemMemory` |
| GPU | DXGI `EnumAdapters1`/`GetDesc1` (sem adaptador de software) |
| Placa-mãe / modelo / chassi | SMBIOS via `GetSystemFirmwareTable('RSMB')` tipos 2 / 1 / 3 (parser próprio, testado com blob sintético) |
| Discos | `\\.\PhysicalDriveN` com acesso 0: `IOCTL_STORAGE_QUERY_PROPERTY` (modelo, barramento), `StorageDeviceSeekPenaltyProperty` (**SSD/HD**), `IOCTL_DISK_GET_DRIVE_GEOMETRY_EX` (tamanho); disco do sistema por `IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS` em `\\.\C:` |
| Secure Boot | registro `SecureBoot\State\UEFISecureBootEnabled` |
| TPM | `Tbsi_GetDeviceInfo` |
| Notebook | chassi SMBIOS (8–11, 14, 30–32) ou bateria (`GetSystemPowerStatus`) |

Tudo sem admin.

### Meu PC completo (fase 6.5, subconjunto user-mode do CPU-Z/Speccy — [../references/speccy-cpuz.md](../references/speccy-cpuz.md))

| Dado | API |
|---|---|
| Fabricante, família/modelo/stepping (hex, como o CPU-Z), instruções (MMX…AVX2/AVX-512, AES, SHA, BMI), virtualização, HT | `__cpuid` (folhas 0, 1, 7, 0x80000001) |
| Caches L1d/L1i/L2/L3 (tamanho × instâncias) | `GetLogicalProcessorInformationEx(RelationCache)` |
| Microcode | registro `CentralProcessor\0\Update Revision` (REG_BINARY) |
| Pentes de RAM: slot, tamanho, tipo (DDR4/DDR5…), formato, velocidade SPD × configurada, fabricante, part number; slots totais | SMBIOS tipos 17 e 16 |
| "XMP/EXPO desligado?" | configurada < nominal inferida do part number (`CMK16GX4M1E3200C16` → 3200, `KF560…` → 6000) |
| BIOS (marca, versão, data) | SMBIOS tipo 0 |
| Driver da GPU (versão + data) | `Control\Class\{4d36e968…}` (`DriverDesc`/`DriverVersion`/`DriverDate`) ou NVML |
| PCIe atual, limite de potência, clocks, potência, temperatura (NVIDIA) | NVML (`nvml.rs`) |
| Monitores (nome, resolução, Hz) | `QueryDisplayConfig` + `DisplayConfigGetDeviceInfo(GET_TARGET_NAME)` |
| WinSAT (nota base + CPU/RAM/gráficos/D3D/disco) | WMI `Win32_WinSAT` (`wmi.rs`, COM, sem admin); "Rodar avaliação" = COM `CInitiateWinSAT::InitiateFormalAssessment` (admin, 2–5 min). Nunca `winsat.exe`. |
| Potência do pacote da CPU | PDH `\Energy Meter(RAPL_Package0_PKG)\Power` (mW) |
| Clock efetivo (turbo) | PDH `Processor Frequency` × `% Processor Performance` ÷ 100 (fallback `CallNtPowerInformation`) |
| Relatório | `report_save`: `Documentos\G2 Boost\g2boost-<stamp>.json` + `.txt` (perfil, WinSAT, estado dos tweaks), abre no Explorer |

**Não fazemos (exigem driver de kernel, como o `cpuz.sys` dos dois apps):** temperatura de CPU, VID/multiplicador/bus, voltagens e ventoinhas (Super I/O), tabela SPD/XMP e timings reais de RAM, registradores de chipset. Bench próprio de CPU e S.M.A.R.T. (elevado) = backlog.

## Testes
`hardware`: parser SMBIOS com blob sintético (tipos 1/2/3), perfil ao vivo (núcleos, RAM, discos com disco do sistema, GPU). `monitor`: duas amostras em faixa (CPU/GPU/disco 0–100, clock > 0). `restore`: parse de `CreationTime`, status sem admin, `create` recusa sem admin. `recommend`: regras com perfis fictícios (SSD desktop, HD notebook com Game Pass/Outlook) e cálculo de score. `tasksched`: Compatibility Appraiser legível ("… Exp" no 24H2+).

**Não verificado pelo agente:** criar ponto de restauração (admin), Boost agora com admin, NVML em máquina AMD/Intel (deve ficar `None`).
