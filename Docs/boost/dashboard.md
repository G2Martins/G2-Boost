# Painel, Meu PC e Monitor

**Última revisão:** 2026-10-25 (Meu PC ganhou a aba Check-up e há uma página Monitor de hardware: [hardware-monitor.md](./hardware-monitor.md); regras de recomendação da rodada 9: debloat e ajustes do Windows; categoria do card de ações lida do catálogo) · 2026-10-23 (monitor: cadência fixa de 1 s, disco = tempo ativo, gráfico que rola e gauge que desliza) · 2026-10-22 (guia da BIOS + Reiniciar na BIOS; Spooler na recomendação) · 2026-10-16 · Código: `crates/core/src/hardware.rs`, `monitor.rs`, `firmware.rs`, `sensors.rs`, `restore.rs`, `recommend.rs`; shell `src-tauri/src/lib.rs` (`Dashboard`); UI `src/pages/dashboard.tsx`, `src/components/ui/kpi-card.tsx`, `live-line.tsx` · Origem da ideia: [../references/galdino-optimizer.md](../references/galdino-optimizer.md).

## Painel (rota inicial)

| KPI | Fonte | Ação |
|---|---|---|
| **Pontuação de otimização** | `recommend::score` = tweaks recomendados **para este PC** ativos ÷ recomendados (indisponíveis não contam) | **Boost agora · N** aplica os pendentes pelo fluxo normal (backup, admin); sem admin, pula os admin e diz quantos. Com `restore_before_preset` e elevado, cria ponto de restauração antes. |
| **Pontos de restauração** | `restore::status` (WMI `root\default:SystemRestore` + `RPSessionInterval`/`DisableSR`) | **Criar ponto** (`SRSetRestorePointW`, admin). Windows limita a 1 por `SystemRestorePointCreationFrequency` min (padrão 1440): a UI diz "pulado" quando a contagem não sobe. Proteção desligada → `protection_off` com instrução. |
| **Arquivos para limpar** | `cleanup::preview` das categorias `default_on` (só medição) | **Limpeza** → rota Limpeza |
| **Memória RAM** | último `monitor:tick` | **Limpar memória** (purge, admin) |

**Ações recomendadas:** `score.pending` (até 6) com categoria e Aplicar; "Ver todas" → Boost. **Monitor em tempo real:** card com abas CPU/GPU/RAM/Disco/Rede, gauge + linha de 60 s + chips (clock, núcleos, temp. GPU, potência + temp. da CPU quando há sensor externo, disco MB/s, rede ↓↑, RAM, GPU).

## Recomendação por hardware (`recommend.rs`)

`Facts` = impressoras reais (`EnumPrintersW` local + conexões, sem PDF/XPS/OneNote/Fax), SSD do sistema (seek penalty), notebook (chassi SMBIOS ou bateria), vendor da GPU, `HwSchMode` presente, Game Pass/Xbox app (pacotes em `%LOCALAPPDATA%\Packages`), Outlook (registro), **layouts de teclado** carregados (`HKCU\Keyboard Layout\Preload`) e **Ryzen X3D de dois CCDs** (nome do CPU com "X3D" e mais de 8 núcleos físicos) (2026-10-25). Regras:

| Tweak | Não recomendado quando | Motivo (i18n `boost.reason.*`) |
|---|---|---|
| `svc_sysmain` | HD mecânico / tipo desconhecido | `hdd` / `unknown_disk` |
| `power_plan` | notebook | `laptop` |
| `hags` | driver sem `HwSchMode` | `hags_unsupported` |
| `svc_xbox` | sempre (já Manual; Game Pass) | `game_pass` / `preference` |
| `svc_search` | Outlook instalado | `outlook` |
| `svc_spooler` | impressora real instalada / lista ilegível (Spooler já parado) | `printer` / `unknown` |
| `fast_startup_off` | HD mecânico / tipo desconhecido (no HD a inicialização rápida ganha segundos de verdade) | `hdd` / `unknown_disk` |
| `alt_shift_off` | um só layout de teclado (o atalho não troca nada) | `single_layout` |
| `core_parking_off` | notebook / Ryzen X3D de dois CCDs (o estacionamento manda o jogo para o CCD com cache) / demais | `laptop` / `x3d` / `preference` |
| `wifi_power_saving_off` | notebook / demais | `laptop` / `preference` |
| `mouse`, `svc_dps`, `svc_geolocation` e, da rodada 9: toda a IA (`copilot_off`, `recall_off`, `click_to_do_off`, `recall_feature_off`, `paint_ai_off`, `notepad_ai_off`, `edge_ai_off`), `teams_chat_off`, `delivery_p2p_off`, `spotlight_off`, `ntfs_last_access_off`, `storage_sense_off`, `network_throttling_off`, `transparency_off`, `classic_context_menu`, `taskbar_left`, `notifications_off`, `error_reporting_off`, `narrator_shortcut_off` | sempre (gosto, não ganho) | `preference` |
| `hvci_off` | sempre (segurança) | `security` |
| betas | sempre | `beta` |

Da rodada 9 entram na pontuação só os que têm ganho real e quase nenhum custo: `edge_background_off`, `widgets_policy_off`, `consumer_features_off`, `telemetry_min`, `diagtrack_manual` (fica ativo junto com `svc_telemetry`: Desativado conta como manual), `start_web_search_off`, `account_notifications_off`, `office_telemetry_off` e `nvidia_telemetry_off` (só onde existem; senão ficam indisponíveis e não contam), `accessibility_shortcuts_off`, `alt_shift_off` e `fast_startup_off` (pelas regras acima). Um teste (`round9_rules_keep_the_score_meaningful`) segura no máximo 12 recomendações novas, para a pontuação de quem já estava otimizado não despencar.

O motivo aparece como badge no card do Boost. Evidência: [../references/windows-services-research.md](../references/windows-services-research.md).

## Monitor (`monitor.rs`)

PDH por nome em inglês (`PdhAddEnglishCounterW`): `\Processor Information(_Total)\% Processor Utility` (fallback `% Processor Time`), `\GPU Engine(*)\Utilization Percentage` (soma por `engtype`, maior = GPU %, como o Gerenciador de Tarefas), `\GPU Adapter Memory(*)\Dedicated Usage`, `\PhysicalDisk(_Total)\% Idle Time` (disco = 100 − ocioso, o "Tempo ativo" do Gerenciador de Tarefas; `% Disk Time` passa de 100 e só fica de reserva) e `Disk Bytes/sec`. Clock: `CallNtPowerInformation(ProcessorInformation)` (máx. `CurrentMhz`). Rede: delta de `GetIfTable2` (interfaces up, sem loopback). Temp. GPU: NVML (`nvml.dll`, user-mode, só NVIDIA; AMD/Intel = backlog ADLX/IGCL). **Temp. de CPU:** sem driver nosso. `sensors.rs` lê o que o usuário já roda — HWiNFO (memória compartilhada `Global\HWiNFO_SENS_SM2`, layout validado por assinatura `HWiS` + `VirtualQuery`, parser testado com blob sintético) ou LibreHardwareMonitor (WMI `root\LibreHardwareMonitor`, classe `Sensor`) — e escolhe "CPU Package"/"Tctl/Tdie"/"Core Max". Consultado a cada 5 amostras (WMI é caro); sem ferramenta aberta, o chip diz como ligar. Decisão completa: [../security/kernel-driver-options.md](../security/kernel-driver-options.md). Thread única no shell com **cadência fixa de 1 s** (dorme até o próximo prazo, não 1 s depois de uma amostra lenta: antes os ticks escorregavam para 1,1–1,8 s e saíam do passo do Gerenciador de Tarefas), refcount por página (`monitor_start/stop`). Cada amostra leva `seq` (1, 2, 3…): o `LiveLine` rola um passo por amostra em 1 s (`.live-scroll`) e o `Gauge` desliza ponteiro (rotate) e arco (dash) juntos (`.gauge-anim`), tudo desligado com movimento reduzido.

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

### BIOS / firmware (`firmware.rs`, card "Placa-mãe e BIOS")

| Configuração | Como o Windows revela |
|---|---|
| Modo de boot UEFI × legado (CSM) | `GetFirmwareType` |
| Secure Boot, TPM | registro / `Tbsi_GetDeviceInfo` (já no perfil) |
| Virtualização (VT-x/AMD-V) | `IsProcessorFeaturePresent(PF_VIRT_FIRMWARE_ENABLED)` **ou** hypervisor presente (CPUID.1:ECX[31]) — com Hyper-V/VBS o Windows vira convidado, o flag e o bit VMX do CPUID leem falso; a UI mostra "ligada (Hyper-V/VBS ativo)" |
| Resizable BAR | tamanho da janela BAR1 pela NVML (`nvmlDeviceGetBAR1MemoryInfo`): 256 MB = desligado, ≈ VRAM = ligado (só NVIDIA por ora) |
| RAM no XMP/EXPO | configurada ≥ nominal do part number em todos os pentes |
| Idade da BIOS | data SMBIOS tipo 0 (`MM/DD/YYYY`) |
| Inicialização rápida, hibernação | `HiberbootEnabled`, `HibernateEnabled` |

Viram achados do conselheiro (`legacy_boot`, `rebar_off`, `bios_old`). **Não visíveis pelo Windows:** C-states, limites de potência, curvas de ventoinha, overclock — a UI diz isso. Na máquina do dono: UEFI, Secure Boot, TPM 2.0, VBS ativo, ReBAR **desligado** (256 MB numa RTX 3050), XMP ok, BIOS F10 de 3,4 anos.

### Guia da BIOS (`firmware::guide`, card "Guia da BIOS" no Meu PC)

Checklist só com o que se aplica a este PC, na ordem de fazer, a partir do `Firmware` lido acima: `csm` (boot legado; avisa converter o disco com MBR2GPT antes), `xmp` (RAM abaixo do nominal), `rebar` (BAR1 de 256 MB em GPU RTX 30+/RX 6000+/Arc, mesma lista do achado `rebar_off`), `secure_boot`, `tpm` (sem TPM), `bios_update` (> 3 anos, como `bios_old`), `virtualization` (opcional: VMs/WSL/emuladores). Cada item traz o caminho do menu **do fabricante da placa** (SMBIOS tipo 2: ASUS, MSI, Gigabyte/AORUS, ASRock; notebook ou outro fabricante = caminho genérico), variante Intel/AMD pelo vendor do CPUID, e a tecla de entrada/modo avançado. Os caminhos são **dado** (`firmware::PATHS`, rótulos da BIOS em inglês, sem tradução; o texto explicativo é i18n `bios.item.*`). Teste: toda combinação item × fabricante tem caminho; o guia só lista o que se aplica; Intel × AMD.

**Reiniciar na BIOS** (`firmware::reboot_to_firmware`, comando `bios_reboot`): o que o `shutdown /r /fw` faz, sem chamar o `shutdown.exe`. Só UEFI (`GetFirmwareType`; legado = `unsupported`) e só elevado (`needs_admin` + botão Elevar). Habilita `SeSystemEnvironmentPrivilege`, lê `OsIndicationsSupported` da variável global EFI `{8BE4DF61-93CA-11D2-AA0D-00E098032B8C}` e, se tiver o bit `EFI_OS_INDICATIONS_BOOT_TO_FW_UI` (0x1), grava `OsIndications` com esse bit **somado aos que já estavam** (atributos preservados; NV+BS+RT se não existia) por `SetFirmwareEnvironmentVariableExW`; depois `SeShutdownPrivilege` + `InitiateShutdownW(SHUTDOWN_RESTART)` (programas abertos ainda podem pedir para salvar). Se o reinício não começar, o `OsIndications` anterior volta (o próximo boot normal não cai na BIOS). Confirmação explícita na UI (modal avisa para salvar o trabalho e como sair sem salvar). Teste: lógica pura do bit (`with_boot_to_fw_ui`); sem admin é recusado antes de qualquer escrita; o reinício nunca roda em teste (o teste pula a checagem quando elevado).

**Não fazemos sem sensor externo (exigem driver de kernel, como o `cpuz.sys` de CPU-Z/Speccy):** VID/multiplicador/bus, tabela SPD/XMP e timings reais de RAM, registradores de chipset; temperatura/ventoinhas/tensões só via HWiNFO/LHM abertos. Bench próprio de CPU e S.M.A.R.T. (elevado) = backlog.

## Check-up e Monitor de hardware (rodada 9)

Duas telas novas leem os mesmos módulos deste doc sem reler o que o conselheiro já tem: a aba **Check-up** do Meu PC (19 medições com limite, veredito e link para a tela que resolve; reaproveita `advisor`, `exposure`, `smart`, `firmware`, `winevents`, `updates`) e a página **Monitor de hardware** (árvore de sensores por dispositivo, mínimo/máximo, CSV; o monitor do Painel continua como está). Detalhe, fontes e limites em [hardware-monitor.md](./hardware-monitor.md).

## Testes
`hardware`: parser SMBIOS com blob sintético (tipos 1/2/3), perfil ao vivo (núcleos, RAM, discos com disco do sistema, GPU). `monitor`: duas amostras em faixa (CPU/GPU/disco 0–100, clock > 0). `restore`: parse de `CreationTime`, status sem admin, `create` recusa sem admin. `recommend`: regras com perfis fictícios (SSD desktop, HD notebook com Game Pass/Outlook, Windows 10 × 11) e cálculo de score. `firmware`: datas SMBIOS, leitura ao vivo sem admin. `sensors`: parser HWiNFO com blob sintético e escolha da temperatura. `tasksched`: Compatibility Appraiser legível ("… Exp" no 24H2+).

**Não verificado pelo agente:** criar ponto de restauração (admin), Boost agora com admin, NVML em máquina AMD/Intel (deve ficar `None`).
