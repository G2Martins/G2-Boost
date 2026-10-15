# Galdino Optimizer — o que extrair (story de 2026-10-14)

**Última revisão:** 2026-10-15 · Fonte: story do Instagram `galdino.tech` ("Só falta patentear — tudo que precisa em um único app — muita loja de informática vai usar"), foto do app rodando em Win11 IoT Enterprise LTSC 2024 (24H2, build 26100), Ryzen 7 9800X3D + RTX 5080. Parte da sidebar "Otimizar" estava tapada ("Tapei pra não copiarem"). Clean-room: aqui só a **ideia** de cada tela e a API nativa que entrega o mesmo dado; nada de assets, textos ou código dele.

## 1. O que aparece na tela

### Sidebar agrupada
`PRINCIPAL` (Painel, Desempenho, Meu PC, Hardware Monitor) · `OTIMIZAR` (lista tapada) · `FERRAMENTAS` · `SISTEMA`. Rodapé com nome do usuário + "Administrador". Versão no topo (v0.13.x).

### Topo do Painel
"Olá, {nome}! Pronto para deixar o seu PC mais rápido?" + chip "OTIMIZAÇÃO PENDENTE — 6 ajustes recomendados desligados".

### 4 KPIs (ícone + número grande + legenda + botão)
| KPI | Legenda | Botão |
|---|---|---|
| Pontuação de otimização **88 %** | 46 de 52 ajustes recomendados ativos | **Boost agora · 2 ajustes** |
| Pontos de restauração **9** | Pontos de restauração do Windows | **Criar ponto** |
| Arquivos para limpar **5,1 GB** | Arquivos que podem ser limpos | **Limpeza** |
| Memória RAM **24 %** | 7,5 GB de 31,6 GB em uso | **Limpar memória** |

### Monitor de Desempenho em Tempo Real
Nome do processador · abas CPU / GPU / RAM / Disco / Rede · gauge circular (4 % uso da CPU, 45 °C) · gráfico de linha (últimos ~60 s) · 4 chips: 4,7 GHz clock atual · 8/16 núcleos/threads · 31,6 GB RAM · 0,0 % carga da GPU.

### Ações recomendadas ("ajustes recomendados para este PC que ainda estão desligados")
Lista com tag de categoria + "Aplicar" por linha + "Ver todas": Mostrar extensões do arquivo (APARÊNCIA) · Desligar os Widgets/notícias (APARÊNCIA) · Desligar a telemetria de compatibilidade (PRIVACIDADE) · Sem destaques na pesquisa na nuvem (PRIVACIDADE) · Não instalar apps de acompanhamento de dispositivos (PRIVACIDADE).

### Informações do Sistema
Computador · Sistema (edição + versão + build) · Processador · Núcleos (físicos/lógicos) · Memória RAM · Ligado há · Placa de vídeo · Placa-mãe · Discos (modelo + capacidade + tipo) · Secure Boot · TPM.

## 2. Mapeamento para o G2 Boost

| Item dele | Nós hoje | Como fazer nativo (sem shell, sem WMI via PowerShell) | Admin | Veredito |
|---|---|---|---|---|
| Pontuação de otimização + "pendente: N" | `boost::detect_all` já dá o estado | **`recommend()` no core**: regras por hardware (SSD/HDD, GPU, notebook/bateria, Game Pass instalado, Outlook) → lista "recomendado para este PC"; score = ativos / recomendados; "Boost agora" aplica os pendentes pelo fluxo normal (backup + admin) | depende | **fazer** — é o mesmo motor que alimenta a IA (fase 8) |
| Pontos de restauração + Criar ponto | não temos | contagem via WMI/COM `root\default:SystemRestore` (`IWbemLocator`, nativo); criar com `SRSetRestorePoint` (`srclient.dll`); Windows limita a 1 ponto/24 h salvo `SystemRestorePointCreationFrequency=0`; "Proteção do sistema" pode estar desligada (mostrar e explicar) | sim | **fazer** — ponto antes de aplicar tweaks casa com o valor central "não destruir nada" |
| Arquivos para limpar X GB | `cleanup::preview` | prévia das categorias padrão ao abrir o painel (cache 5 min) | parcial | **fazer** (reuso) |
| RAM % + Limpar memória | aba RAM | `GlobalMemoryStatusEx` + purge | sim | já temos; só virar KPI |
| Monitor CPU | — (rede temos) | `GetSystemTimes` ou PDH `\Processor Information(_Total)\% Processor Utility`; clock atual via `CallNtPowerInformation(ProcessorInformation)` (`CurrentMhz`) | não | **fazer** |
| Monitor GPU | — | PDH `\GPU Engine(*)\Utilization Percentage` (soma por processo) e `\GPU Adapter Memory(*)\Dedicated Usage` (todos os vendors, Win10+); nome/VRAM via DXGI `EnumAdapters1` | não | **fazer** |
| Monitor RAM / Disco / Rede | RAM sim; rede só latência | PDH `\PhysicalDisk(_Total)\% Disk Time` + bytes/s; rede via `GetIfTable2` (delta de bytes, iphlpapi já linkado) | não | **fazer** |
| Temperatura da CPU (45 °C) | — | **não existe API pública**: `MSAcpi_ThermalZoneTemperature` raramente responde em desktop; HWiNFO/LibreHardwareMonitor usam **driver de kernel** (WinRing0 está na blocklist de drivers vulneráveis da Microsoft). GPU dá: NVML `nvmlDeviceGetTemperature` (nvml.dll, user-mode) / AMD ADLX / Intel IGCL | — | **CPU: não** (mostrar "—" + motivo). **GPU: sim**, só temp da placa |
| Ações recomendadas com categoria | cards do Boost | adicionar `category` (Desempenho / Privacidade / Aparência / Rede) aos tweaks + lista "pendentes" no painel com Aplicar | não | **fazer** |
| Tweaks de aparência/privacidade dele | não temos | HKCU: `Explorer\Advanced\HideFileExt=0` (extensões), `TaskbarDa=0` (widgets), `SearchSettings\IsDynamicSearchBoxEnabled=0` + `Search\BingSearchEnabled=0` (destaques/nuvem), `ContentDeliveryManager\SilentInstalledAppsEnabled=0` (apps automáticos), `AdvertisingInfo\Enabled=0`; "telemetria de compatibilidade" = desabilitar a tarefa `\Microsoft\Windows\Application Experience\Microsoft Compatibility Appraiser` via `IRegisteredTask::SetEnabled` (temos `tasksched.rs`) | só a tarefa | **fazer** — família nova "Privacidade e aparência", toda HKCU |
| Informações do Sistema | só OS + RAM | nome: `GetComputerNameExW`; edição/versão: registro `CurrentVersion` (`ProductName`, `DisplayVersion`, `EditionID`) + `RtlGetVersion`; CPU: `ProcessorNameString`; núcleos: `GetLogicalProcessorInformationEx`; RAM: `GetPhysicallyInstalledSystemMemory`; uptime: `GetTickCount64`; GPU: DXGI; placa-mãe: SMBIOS `GetSystemFirmwareTable('RSMB')` tipo 2; discos: `IOCTL_STORAGE_QUERY_PROPERTY` (modelo, barramento NVMe/SATA/USB) + `StorageDeviceSeekPenaltyProperty` (**SSD vs HDD** — também decide o SysMain); Secure Boot: `SecureBoot\State\UEFISecureBootEnabled`; TPM: `Tbsi_GetDeviceInfo` | não | **fazer** — página "Meu PC"; o detector SSD/HDD é pré-requisito das recomendações |
| Saudação "Olá, {nome}" + badge Administrador | badge temos | `USERNAME` + `is_elevated` | — | fazer no painel |
| Sidebar agrupada | 5 ícones flat | grupos Principal / Otimizar / Ferramentas / Sistema | — | depois, quando houver mais páginas |

## 3. Gráficos que valem a pena (via `<ChartContainer>`, tokens HSL)
- **Gauge circular** de uso (CPU/GPU/RAM/Disco) — reaproveitar o gauge do speedtest.
- **Linha de 60 s** com marcadores — já existe no monitor de rede; virar componente genérico `LiveLine` alimentado por `monitor:tick` a cada 1 s.
- **KPI card** (ícone, valor grande em `num`, legenda, CTA) — componente novo, usado 4× no painel.
- **Barra de pontuação** (0–100 %) com cor por faixa (success / warning / destructive).

## 4. Proposta de fase ("Painel, Meu PC e Monitor")
1. Core `hardware.rs` (CPU/GPU/placa-mãe/discos/SSD-HDD/Secure Boot/TPM) + `monitor.rs` (PDH + `GetIfTable2`, amostra por segundo) + `restore.rs` (`SRSetRestorePoint`, contagem via WMI/COM) + `recommend()` com regras por hardware + família de tweaks "Privacidade e aparência" + `category` nos tweaks.
2. Tauri: comandos `system_profile`, `monitor_start/stop` (evento 1 s), `restore_points`, `restore_create`, `recommendations`.
3. UI: Painel (KPIs + ações recomendadas + monitor), página Meu PC, componente `LiveLine`/`Gauge`/`KpiCard`. Painel vira rota inicial.
4. Testes: parser SMBIOS com buffer fixo, seek-penalty em sandbox (mock de descriptor), regras de recomendação com perfis fictícios (HDD, notebook, Game Pass).

Fora: temperatura de CPU (driver), overlay (backlog), Policies.

## 5. Apps para análise estática (clean-room, só imports/strings/registro — como em `binary-analysis.md`)
- **Galdino Optimizer** — só se houver build pública/legal; o que interessa é a lista de chaves de registro e serviços (strings), não a UI. Respeitar a licença; se proibir engenharia reversa, ficar nas strings e no comportamento observável (Process Monitor).
- **Não precisa baixar** (código aberto, ler o fonte): Chris Titus **WinUtil** (`config/tweaks.json`), **Win11Debloat**, **Sophia Script**, **hellzerg/Optimizer** (C#, catálogo grande), **LibreHardwareMonitor** (mostra por que temperatura exige driver).
- Fechados que só valem pela lista pública de opções: **O&O ShutUp10++** (privacidade), **Winaero Tweaker**. Sensores: **HWiNFO** (driver próprio — referência do que **não** vamos fazer).
