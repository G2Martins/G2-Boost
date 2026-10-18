# PC Boost — tweaks, efeitos visuais, serviços, RAM

**Última revisão:** 2026-10-18 · Código: `crates/core/src/boost.rs`, `visualfx.rs`, `registry.rs`, `startup.rs`, `procs.rs` · UI: `src/pages/boost.tsx`, catálogo de apresentação em `src/data/catalogs.ts`.

## Modelo

- Cada tweak é **dado** (lista de valores de registro, nomes de serviço, parâmetros `SystemParametersInfo`, esquema de energia). O core expõe `detect` / `apply` / `revert`; a UI só mostra estado e chama.
- **Backup exato antes de aplicar.** `apply(id, true)` lê os valores atuais de tudo que o tweak toca e grava `%LOCALAPPDATA%\G2 Boost\backups\<id>.json` (uma vez; um segundo `apply` não sobrescreve). `apply(id, false)` restaura esse backup e apaga o arquivo. Sem backup (o usuário já tinha mexido à mão), desligar grava o **padrão documentado do Windows** e a UI avisa ("Sem backup: desligar grava o padrão do Windows").
- **Nunca shell.** Registro via `RegOpenKeyExW`/`RegSetValueExW`; serviços via SCM (`OpenServiceW`, `ChangeServiceConfigW`, `ControlService`, `StartServiceW`); energia via `PowerGetActiveScheme`/`PowerSetActiveScheme`/`PowerDuplicateScheme`; RAM via `NtSetSystemInformation(SystemMemoryListInformation, MemoryPurgeStandbyList)`; elevação via `ShellExecuteExW("runas")`.
- **Admin:** o app roda `asInvoker`. Tweaks `HKLM`, serviços, energia e purge de RAM retornam `needs_admin` sem tentar; a UI mostra "Elevar (administrador)", que relança o exe elevado com `--route boost` e fecha a instância atual. IDs vindos da UI passam por `registry::safe_id` (`[a-z0-9_]`) antes de virar nome de arquivo.

## Presets (decisão do dono, 2026-10-14)

| Preset | Conteúdo |
|---|---|
| **Padrão** | efeitos visuais **Média**, modo de jogo + Game DVR off, input lag, teclado, dicas off, **SysMain off**, telemetria off |
| **Alto desempenho** | Padrão + efeitos visuais **Máxima**, mouse sem aceleração, plano Desempenho máximo, HAGS, MMCSS, latência de rede, WSearch off, Xbox off |
| **Personalizado** | o usuário liga/desliga cada card; betas (`priority_sep`, `perf_counters`) só aqui |

A UI calcula o preset ativo comparando o estado real (tweaks "indisponíveis" são ignorados). Aplicar um preset percorre os cards que diferem; sem admin, os cards admin ficam de fora e a UI diz quantos.

## Catálogo

| id | O que faz | Mecanismo | Admin | Reboot | Não recomendado se… |
|---|---|---|---|---|---|
| `visual_fx` | 17 itens de "Opções de Desempenho" em níveis | ver abaixo | não | não | gosta das animações (muda sensação, não FPS) |
| `game_mode` | Game Mode on; Game DVR/captura off | HKCU `GameBar\AutoGameModeEnabled=1`, `AllowAutoGameMode=1`, `GameConfigStore\GameDVR_Enabled=0`, `GameDVR\AppCaptureEnabled=0` | não | não | grava clipes pela Game Bar |
| `input_lag` | FSE off para todos os jogos | HKCU `GameConfigStore\GameDVR_FSEBehaviorMode=2`, `GameDVR_HonorUserFSEBehaviorMode=1`, `GameDVR_FSEBehavior=2`, `GameDVR_DXGIHonorFSEWindowsCompatible=1`, `GameDVR_EFSEFeatureFlags=0` | não | não | overlays que exigem janela otimizada |
| `keyboard` | repeat delay 0, speed 31 | `SPI_SETKEYBOARDDELAY/SPEED` (grava em `Control Panel\Keyboard`) | não | não | teclas de filtro |
| `tips` | dicas/sugestões off | HKCU `ContentDeliveryManager\SubscribedContent-338389Enabled=0`, `SoftLandingEnabled=0`, `SystemPaneSuggestionsEnabled=0`, `…-310093`, `…-353694`, `…-353696` | não | não | quer sugestões da Microsoft |
| `mouse` | aceleração off | `SPI_SETMOUSE [0,0,0]` (padrão `[6,10,1]`) | não | não | acostumado com aceleração |
| `svc_sysmain` | SysMain Disabled + parado | SCM | sim | não | HD mecânico |
| `svc_telemetry` | DiagTrack + dmwappushservice Disabled | SCM | sim | não | Insider |
| `power_plan` | esquema "Desempenho máximo" ativo | procura esquema com o nome do template `e9a42b02-…`; duplica se não existe; backup = GUID anterior | sim | não | notebook na bateria |
| `hags` | `HwSchMode=2` | HKLM `Control\GraphicsDrivers` | sim | **sim** | GPU < 2018 |
| `mmcss` | categoria Games alta | HKLM `Multimedia\SystemProfile\SystemResponsiveness=10`, `Tasks\Games`: `GPU Priority=8`, `Priority=6`, `Scheduling Category=High`, `SFIO Priority=High` | sim | não | streaming perde quadros |
| `network_latency` | ACK imediato, sem Nagle | HKLM `Tcpip\Parameters\Interfaces\*`: `TcpAckFrequency=1`, `TCPNoDelay=1`, `TcpDelAckTicks=0` (backup por interface) | sim | sim | satélite/perda alta |
| `svc_search` | WSearch Disabled | SCM | sim | não | usa busca do Windows/Outlook |
| `svc_xbox` | XblAuthManager, XblGameSave, XboxNetApiSvc, XboxGipSvc Disabled | SCM | sim | não | Game Pass / app Xbox |
| `priority_sep` (beta) | `Win32PrioritySeparation=0x26` | HKLM `Control\PriorityControl` (padrão 2) | sim | não | encoder/build em segundo plano |
| `perf_counters` (beta) | `Disable Performance Counters=1` | HKLM `Perflib` | sim | não | usa perfmon/telemetria |

### Adicionados em 2026-10-15 (pesquisa de serviços + extração do Galdino Optimizer)

| id | O que faz | Mecanismo | Admin | Preset | Categoria |
|---|---|---|---|---|---|
| `show_extensions` | Explorer mostra extensões | HKCU `Explorer\Advanced\HideFileExt=0` | não | Padrão + Alto | Aparência |
| `widgets_off` | Widgets/notícias off | HKCU `Explorer\Advanced\TaskbarDa=0` | não | Padrão + Alto | Aparência |
| `search_highlights_off` | destaques/Bing na pesquisa off | HKCU `SearchSettings\IsDynamicSearchBoxEnabled=0`, `Search\BingSearchEnabled=0` | não | Padrão + Alto | Privacidade |
| `silent_apps_off` | apps promovidos/pré-instalados off | HKCU `ContentDeliveryManager\SilentInstalledAppsEnabled=0`, `PreInstalledAppsEnabled=0`, `OemPreInstalledAppsEnabled=0` | não | Padrão + Alto | Privacidade |
| `advertising_id_off` | ID de publicidade off | HKCU `AdvertisingInfo\Enabled=0` | não | Padrão + Alto | Privacidade |
| `compat_telemetry_off` | CompatTelRunner/CEIP off | tarefas `\Microsoft\Windows\Application Experience\{Microsoft Compatibility Appraiser, … Exp, ProgramDataUpdater}` e `\…\Customer Experience Improvement Program\{Consolidator, UsbCeip}` via `IRegisteredTask::SetEnabled` (ausentes são ignoradas) | sim | Padrão + Alto | Privacidade |
| `svc_dps` | DPS Disabled | SCM | sim | Personalizado | Desempenho |
| `svc_geolocation` | lfsvc Disabled | SCM | sim | Personalizado | Privacidade |
| `hvci_off` (beta) | Isolamento de Núcleo off | HKLM `DeviceGuard\Scenarios\HypervisorEnforcedCodeIntegrity\Enabled=0` (reboot) | sim | Personalizado | Desempenho |

**Mudanças de preset pela evidência** ([../references/windows-services-research.md](../references/windows-services-research.md)): `svc_xbox` saiu do Alto desempenho (serviços já Manual/parados → ganho nulo, efeitos reais no Game Pass); `svc_sysmain` continua no Padrão por decisão do dono, mas `recommend()` o tira da lista em HD mecânico. Cada tweak tem `category` (Desempenho / Privacidade / Aparência / Rede) e, no card, o motivo de "não recomendado neste PC" ([dashboard.md](dashboard.md)).

**Fora, de propósito:** `Psched\NonBestEffortLimit` (fica em `Policies` — gatilho do Kaspersky, efeito desprezível), `MSMQ\TCPNoDelay` (cargo cult, só MSMQ), Prefetch (ver `legacy-bat-scripts.md`), Windows Update/Defender/Spooler.

Serviços: `WSearch` é "automático (início atrasado)"; o revert restaura o tipo de início gravado, mas não a flag de atraso (`ChangeServiceConfig2W` fica para quando alguém sentir falta). Serviço ausente na máquina (ex.: Xbox em LTSC) é ignorado; se nenhum existir, o card mostra "Indisponível".

### Adicionado em 2026-10-16

| id | O que faz | Preset |
|---|---|---|
| `windowed_optimizations` | Flag `SwapEffectUpgradeEnable=1` dentro de `HKCU\Software\Microsoft\DirectX\UserGpuPreferences\DirectXUserGlobalSettings` (a opção "Otimizações para jogos em janela" de Configurações › Tela › Gráficos): DX10/11 em janela/sem borda usam flip model → menos latência, VRR/Auto HDR. Edita **só essa chave** da string (`dx_set`/`dx_get`, testados) e preserva Auto HDR/VRR; backup = string inteira. Recomendado só no Windows 11 (`windows10` como motivo no 10). | Padrão, Alto |

**Avaliados e deixados de fora** (vídeos/stories de otimização): HPET/`useplatformclock` via bcdedit (shell + efeito controverso, piora em CPUs modernas), resolução de timer global (`timeBeginPeriod` afeta o sistema todo; Windows 11 já isola por processo), prioridade por IFEO para exes de jogo (é o padrão que antivírus marcam como persistência/injeção), "limpeza de RAM" contínua de working set (piora; só purge da standby sob demanda), desligar mitigações de CPU (segurança).

## Inicialização e processos (`startup.rs`, `procs.rs` — aba "Inicialização")

- **Lista:** `HKCU\…\Run`, `HKLM\…\Run`, `HKLM\WOW6432Node\…\Run`, pastas Inicializar do usuário e comum. Estado lido de `…\Explorer\StartupApproved\{Run,Run32,StartupFolder}` exatamente como o Gerenciador de Tarefas (1º byte par = ligado; ímpar = desligado + FILETIME).
- **Desligar/ligar** grava só o flag `StartupApproved` (12 bytes): a entrada Run e o atalho ficam onde estavam; religar restaura. Item precisa existir na lista ao vivo (caller hostil); HKLM/pasta comum exigem admin (`needs_admin`); **software de segurança é protegido** — Segurança do Windows (`SecurityHealth`) e antivírus de terceiros comuns (Kaspersky, Avast, AVG, Bitdefender, ESET, Norton, McAfee, Malwarebytes, Sophos, Trend Micro, Avira…; `startup::SECURITY_MARKERS`, casado no nome ou no comando) — recusado no core (`protected_item`), cadeado na UI. Vale para a IA também.
- **Processos:** `EnumProcesses` + `QueryFullProcessImageNameW` + `GetProcessTimes` em duas amostras (1 s) → agrupado por exe (contagem, RAM = soma do working set, CPU % da capacidade total), `system` = vive sob `C:\Windows`. Só leitura: o G2 Boost **não mata processo**; a IA e a UI apontam para fechar o app ou tirá-lo da inicialização.
- Testes: flags de `StartupApproved` iguais ao Gerenciador de Tarefas, round-trip real em HKCU com entrada descartável, item desconhecido recusado, proteção; processos agrupam a família do próprio teste.

## Efeitos visuais (`visualfx.rs`)

O diálogo do Windows é data-driven: `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\VisualEffects\<item>` diz se o item é uma ação `SystemParametersInfo` (`SPIActionGet/Set`) ou um valor HKCU (`RegPath`). Lido na máquina de dev em 2026-10-14 e congelado como tabela de 17 itens:

| Família | Itens | Get/Set |
|---|---|---|
| `SPI` BOOL em pvParam (0x1000+) | combobox_animation 0x1004/05, menu_animation 0x1002/03, listbox_smooth_scrolling 0x1006/07, selection_fade 0x1014/15, tooltip_animation 0x1016/17, cursor_shadow 0x101A/1B, drop_shadow 0x1024/25, control_animations 0x1042/43 | pvParam |
| `SPI` BOOL em uiParam | drag_full_windows 38/37, font_smoothing 74/75 | uiParam |
| `SPI_GET/SETANIMATION` | animate_minmax 72/73 (`ANIMATIONINFO.iMinAnimate`) | struct |
| HKCU `Explorer\Advanced` | taskbar_animations, listview_alpha_select, listview_shadow, thumbnails (`IconsOnly`, invertido) | DWORD |
| HKCU `DWM` | aero_peek (`EnableAeroPeek`), save_thumbnails (`AlwaysHibernateThumbnails`) | DWORD |

Aplicação com `SPIF_UPDATEINIFILE | SPIF_SENDCHANGE` + broadcast `WM_SETTINGCHANGE`; `VisualFXSetting` recebe 3 (personalizado) ou 1 (aparência). Alguns itens do Explorer (barra de tarefas, miniaturas) podem só refletir visualmente após reiniciar o Explorer — verificar na máquina do dono.

| Nível | Desligados |
|---|---|
| Aparência | nenhum (tudo ligado) |
| Mínima | animações da barra, conteúdo ao arrastar, salvar miniaturas, retângulo translúcido |
| Média (setup do dono) | Mínima + sombras sob janelas |
| Máxima | tudo, exceto fontes suavizadas |

`detect` devolve o nível que bate exatamente; mistura própria = `level: null` ("Mistura própria" na UI). "Restaurar como estava" aplica o backup dos 17 itens + `VisualFXSetting`.

Teste ao vivo (`cursor_shadow_roundtrip`, `#[ignore]`): flip + restore passou em 2026-10-14.

## Limpeza de RAM

`purge_standby_list()`: habilita `SeProfileSingleProcessPrivilege` no token (só existe elevado) e chama `NtSetSystemInformation(80, MemoryPurgeStandbyList=4)`. Devolve `avail` antes/depois e bytes liberados. **Não** faz `EmptyWorkingSets` (piora, script v2.0). Rotina automática (ao abrir jogo / agendada) = fase Rotinas.

## Testes

- `registry`: round-trip DWORD/SZ/delete em `HKCU\Software\G2 Boost\Test`, HKLM legível sem admin, backup JSON + ids hostis (`../evil` rejeitado).
- `boost`: `detect_all` sem admin (serviços via SCM, nome do plano de energia); apply → backup → revert byte a byte com um tweak de teste em `HKCU\Software\G2 Boost\Test\boost`; `Partial`; `apply_default`; admin recusado sem elevação; enumeração de esquemas de energia + parse de GUID.
- `visualfx`: 17 itens legíveis, consistência dos níveis, round-trip real (ignored).

**Não verificado pelo agente (precisa de UAC):** apply/revert de HKLM, serviços, plano de energia e purge. Testar na máquina do dono elevando o app; se o Kaspersky reclamar, registrar em `Docs/security/kaspersky-false-positive.md`.
