# PC Boost — tweaks, efeitos visuais, serviços, RAM

**Última revisão:** 2026-10-25 (rodada 9: modelo novo, 34 ajustes de debloat e do Windows, busca/filtros/"Ver o que muda", presets só gerenciam os próprios membros, revert endurecido) · 2026-10-22 (tarefas WinSAT/diagnóstico/feedback, Spooler) · 2026-10-18 · Código: `crates/core/src/boost.rs`, `winfeature.rs`, `visualfx.rs`, `registry.rs`, `startup.rs`, `procs.rs` · UI: `src/pages/boost.tsx`, `src/components/boost/tweak-kit.tsx`, catálogo de apresentação em `src/data/catalogs.ts`.

> Planos de energia e Gerenciador de tarefas (abas Energia e Processos) → [power-processes.md](power-processes.md). Página Debloat (níveis, IA do Windows, apps pré-instalados) → [debloat.md](debloat.md).

## Rodada 9 (2026-10-25): modelo e aba Ajustes de sistema

- **Categorias:** Desempenho, Jogos, Privacidade, IA, Aparência, Rede, Sistema (`Category` no core; `category` no catálogo da UI). Remapeadas: modo de jogo, input lag, otimizações em janela, teclado, mouse, HAGS, MMCSS e `priority_sep` → Jogos. O card de ações do Painel lê a categoria do catálogo.
- **Igualdade core × UI testada:** `catalog_matches_the_ui_and_the_dictionaries` lê `catalogs.ts` e os dois dicionários: mesmos ids, categoria, admin e beta, e nome/descrição/aviso em pt-BR e en.
- **Disponibilidade (`gate`):** Windows 11 (`classic_context_menu`, `taskbar_left`), builds com botão Chat (`teams_chat_off`), Office instalado (`office_telemetry_off`), adaptador Wi-Fi (`wifi_power_saving_off`). Fora disso: `unavailable` + `detail = not_present`, não conta na pontuação.
- **Serviço com alvo:** `Svc.target` = Desativado ou Manual. Nunca afrouxa: alvo Manual com o serviço já Desativado não mexe e conta como ativo; só alvo Desativado para o serviço.
- **Tipos novos:** `PowerSetting` (AC/DC no plano ativo, reaplica o plano; backup guarda o plano alterado; sem backup grava o padrão da personalidade Equilibrado via `PowerReadAC/DCDefaultIndex`), `AccessKeys` (bit `HOTKEYACTIVE` de Aderência/Filtro/Alternância por SPI), `EmptyKey` (chave HKCU com padrão vazio; desfazer apaga a árvore só se fomos nós que criamos), `Feature` (recurso opcional pela DismApi, ver [debloat.md](debloat.md)) e DWORD por bit (`NtfsDisableLastAccessUpdate`: o 0x80000003 do próprio Windows conta como ligado).
- **Reinício (`restart` no catálogo):** `reboot`, `signout` ou `explorer`, mostrado como selo. Depois de aplicar, `refresh` manda `WM_SETTINGCHANGE` com `TraySettings` (barra) ou `ImmersiveColorSet` (transparência) e `SPI_SETLANGTOGGLE` (Alt+Shift).
- **"Ver o que muda":** `boost::changes(id)` deriva da própria definição (registro, serviço + tipo de início, tarefas, energia, recurso); o comando `tweak_changes` entrega à UI.
- **Revert endurecido:** o backup JSON é gravável pelo usuário; `restore_entries` só repete entradas que uma captura nova do mesmo ajuste também produziria (mesma raiz, caminho e nome). Entrada plantada é ignorada (teste).
- **Presets:** Padrão/Alto desempenho só ligam ou desligam membros de algum preset (`PRESET_MANAGED`); debloat, ajustes do Windows e os "Personalizado" ficam como o usuário deixou. Antes, aplicar um preset desligava tudo que não era dele.
- **Aba Ajustes de sistema:** busca (sem acento/caixa), chips de categoria com contagem, "N de M ativos", **Aplicar recomendados** (ponto de restauração antes conforme a configuração, toast com Desfazer do lote), ícone de catálogo em todo card, "Ver o que muda" em todo card.

| id | O que faz | Mecanismo | Admin | Categoria |
|---|---|---|---|---|
| `ntfs_last_access_off` | sem data de último acesso | HKLM `FileSystem\NtfsDisableLastAccessUpdate=0x80000001` | sim | Desempenho |
| `storage_sense_off` | Sensor de Armazenamento off | HKCU `StoragePolicy\01=0` (ausente = já desligado) | não | Sistema |
| `fast_startup_off` | inicialização rápida off | HKLM `Session Manager\Power\HiberbootEnabled=0` | sim | Sistema |
| `network_throttling_off` | sem limitação de rede multimídia | HKLM `SystemProfile\NetworkThrottlingIndex=0xFFFFFFFF` | sim | Rede |
| `wifi_power_saving_off` | Wi-Fi em desempenho máximo (AC e DC) | plano ativo `19cbb8fa…\12bbebe6…` = 0 | sim | Rede |
| `core_parking_off` | núcleos sem estacionar (AC) | plano ativo `SUB_PROCESSOR\CPMINCORES` = 100 | sim | Desempenho |
| `transparency_off` | sem transparência | HKCU `Personalize\EnableTransparency=0` | não | Aparência |
| `classic_context_menu` | menu completo no clique direito | HKCU `Classes\CLSID\{86ca1aa0-…}\InprocServer32` vazio | não | Aparência |
| `taskbar_left` | barra à esquerda | HKCU `TaskbarAl=0` | não | Aparência |
| `notifications_off` | notificações off | HKCU `PushNotifications\ToastEnabled=0` | não | Sistema |
| `alt_shift_off` | Alt+Shift não troca idioma | HKCU `Keyboard Layout\Toggle`: `Language Hotkey`/`Hotkey`="3" | não | Jogos |
| `narrator_shortcut_off` | Win+Ctrl+Enter não liga o Narrador | HKCU `Narrator\NoRoam\WinEnterLaunchEnabled=0` | não | Jogos |
| `accessibility_shortcuts_off` | atalhos de Aderência/Filtro/Alternância off | SPI, bit `HOTKEYACTIVE` | não | Jogos |

Debloat e IA (`edge_background_off` … `error_reporting_off`): tabela em [debloat.md](debloat.md). **Compressão de memória ficou de fora:** só existe pelo módulo MMAgent do PowerShell (CDXML sobre uma classe WMI sem documentação própria) e não ajuda com 16 GB ou mais ([../references/debloat-sources.md](../references/debloat-sources.md)).

## Modelo

- Cada tweak é **dado** (lista de valores de registro, nomes de serviço, parâmetros `SystemParametersInfo`, esquema de energia). O core expõe `detect` / `apply` / `revert`; a UI só mostra estado e chama.
- **Backup exato antes de aplicar.** `apply(id, true)` lê os valores atuais de tudo que o tweak toca e grava `%LOCALAPPDATA%\G2 Boost\backups\<id>.json` (uma vez; um segundo `apply` não sobrescreve). `apply(id, false)` restaura esse backup e apaga o arquivo. Sem backup (o usuário já tinha mexido à mão), desligar grava o **padrão documentado do Windows** e a UI avisa ("Sem backup: desligar grava o padrão do Windows").
- **Nunca shell.** Registro via `RegOpenKeyExW`/`RegSetValueExW`; serviços via SCM (`OpenServiceW`, `ChangeServiceConfigW`, `ControlService`, `StartServiceW`); energia via `PowerGetActiveScheme`/`PowerSetActiveScheme`/`PowerDuplicateScheme`; RAM via `NtSetSystemInformation(SystemMemoryListInformation, MemoryPurgeStandbyList)`; elevação via `ShellExecuteExW("runas")`.
- **Detecção = o que o Windows mostra (2026-10-18).** `detect` conta só os valores que distinguem ligado de desligado:
  - valor igual nos dois estados (MMCSS `GPU Priority` = 8) é ignorado;
  - `op_default_on`: ausência já significa "ligado" no Windows (Modo de jogo: `AutoGameModeEnabled`/`AllowAutoGameMode` ausentes = ligado, só `0` explícito desliga), então não puxa o estado para Parcial;
  - **estado em execução onde o registro é só um pedido:** HAGS lê o driver (`hardware::hags_state`, D3DKMT `KMTQAITYPE_WDDM_2_7_CAPS`; GPU sem suporte = Indisponível) e `hvci_off` lê a WMI `Win32_DeviceGuard` (`advisor::hvci_running`). Valor ausente → mostra o padrão em execução; pedido ≠ execução → `detail = "reboot_pending"` e a UI avisa "Reinicie o PC para valer";
  - plano de energia: qualquer cópia do Desempenho máximo conta como ligado (nome do modelo localizado ou "Ultimate Performance (…)" de outras ferramentas).
  - A página relê tudo quando a janela volta ao foco (`focus` + `visibilitychange`): mexeu em Configurações do Windows, o G2 Boost acompanha.
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

**Fora, de propósito:** `Psched\NonBestEffortLimit` (fica em `Policies` — gatilho do Kaspersky, efeito desprezível), `MSMQ\TCPNoDelay` (cargo cult, só MSMQ), Prefetch (ver `legacy-bat-scripts.md`), Windows Update/Defender. Spooler entrou em 2026-10-22 só para PC sem impressora (abaixo).

Serviços: `WSearch` é "automático (início atrasado)"; o revert restaura o tipo de início gravado, mas não a flag de atraso (`ChangeServiceConfig2W` fica para quando alguém sentir falta). Serviço ausente na máquina (ex.: Xbox em LTSC) é ignorado; se nenhum existir, o card mostra "Indisponível".

### Adicionado em 2026-10-16

| id | O que faz | Preset |
|---|---|---|
| `windowed_optimizations` | Flag `SwapEffectUpgradeEnable=1` dentro de `HKCU\Software\Microsoft\DirectX\UserGpuPreferences\DirectXUserGlobalSettings` (a opção "Otimizações para jogos em janela" de Configurações › Tela › Gráficos): DX10/11 em janela/sem borda usam flip model → menos latência, VRR/Auto HDR. Edita **só essa chave** da string (`dx_set`/`dx_get`, testados) e preserva Auto HDR/VRR; backup = string inteira. Recomendado só no Windows 11 (`windows10` como motivo no 10). | Padrão, Alto |

**Avaliados e deixados de fora** (vídeos/stories de otimização): HPET/`useplatformclock` via bcdedit (shell + efeito controverso, piora em CPUs modernas), resolução de timer global (`timeBeginPeriod` afeta o sistema todo; Windows 11 já isola por processo), prioridade por IFEO para exes de jogo (é o padrão que antivírus marcam como persistência/injeção), "limpeza de RAM" contínua de working set (piora; só purge da standby sob demanda), desligar mitigações de CPU (segurança).

### Adicionados em 2026-10-22 (rodada 7)

| id | O que faz | Mecanismo | Admin | Preset | Categoria |
|---|---|---|---|---|---|
| `winsat_task_off` | avaliação do Windows em ociosidade off | tarefa `\Microsoft\Windows\Maintenance\WinSAT` (`Kind::Tasks`) | sim | Alto | Desempenho |
| `feedback_tasks_off` | diagnóstico de disco, pedidos de feedback e envio da fila de relatórios de erro off | tarefas `\Microsoft\Windows\DiskDiagnostic\Microsoft-Windows-DiskDiagnosticDataCollector`, `\…\Feedback\Siuf\{DmClient, DmClientOnScenarioDownload}`, `\…\Windows Error Reporting\QueueReporting` | sim | Padrão + Alto | Privacidade |
| `svc_spooler` | Spooler de impressão Disabled + parado | SCM (`Spooler`, padrão Automático) | sim | Personalizado | Desempenho |

- Tarefas ausentes no build são ignoradas (comportamento do `tasksched`); se nenhuma existir, o card fica "Indisponível". **Nunca** `ScheduledDefrag` (TRIM de SSD), `UpdateOrchestrator` nem tarefas do Defender.
- **WinSAT continua no Meu PC:** "Rodar avaliação" usa a API COM (`CInitiateWinSAT::InitiateFormalAssessment`, `winsat.rs`), não a tarefa agendada; desligar a tarefa só para a execução automática em ociosidade (as notas param de se atualizar sozinhas, o achado `winsat_stale` aponta quando rodar).
- **Spooler recomendado só sem impressora real** (`recommend.rs`): `EnumPrintersW` (local + conexões, nível 4), ignorando virtuais (nome com PDF, XPS, OneNote, Fax). Impressora real → motivo `printer`; lista ilegível (Spooler já desligado, RPC) → `unknown`. Fora dos presets porque o preset não consulta a recomendação; entra no "Boost agora" do Painel só quando recomendado. Imprimir em PDF pela impressora virtual também precisa do Spooler (dito no aviso do card).

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
