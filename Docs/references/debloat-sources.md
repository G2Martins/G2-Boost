# Fontes das chaves do debloat (rodada 9)

**Última revisão:** 2026-10-25 · Uso: [../boost/debloat.md](../boost/debloat.md), [../boost/tweaks.md](../boost/tweaks.md). Nota interna: a UI nunca cita fontes; aqui sim.

Conferido em 2026-10-25 contra a documentação da Microsoft quando ela existe. O que não tem doc oficial está marcado e foi mantido só quando é o valor que a própria tela de Configurações grava.

| Ajuste | Chave | Fonte | Situação |
|---|---|---|---|
| `recall_off` | `Policies\Microsoft\Windows\WindowsAI\DisableAIDataAnalysis` (usuário e máquina) | Policy CSP WindowsAI (learn.microsoft.com/windows/client-management/mdm/policy-csp-windowsai) | oficial |
| `click_to_do_off` | `…\WindowsAI\DisableClickToDo` (usuário e máquina) | Policy CSP WindowsAI | oficial (24H2+) |
| `copilot_off` | HKCU `Policies\Microsoft\Windows\WindowsCopilot\TurnOffWindowsCopilot` | Policy CSP WindowsAI: escopo só de usuário, marcada como obsoleta, não vale para o app Copilot novo | oficial; HKLM retirado |
| `paint_ai_off` | HKLM `Software\Microsoft\Windows\CurrentVersion\Policies\Paint`: `DisableCocreator`, `DisableGenerativeFill`, `DisableImageCreator` | Policy CSP WindowsAI (mapeamento GPO "Windows Components > Paint") | oficial; `DisableGenerativeErase`/`DisableRemoveBackground` circulam em listas mas não têm doc: fora |
| `notepad_ai_off` | HKLM `SOFTWARE\Policies\WindowsNotepad\DisableAIFeatures` | "Manage AI features in Notepad" (learn.microsoft.com/windows/client-management/manage-notepad) | oficial |
| `edge_ai_off`, `edge_background_off` | HKLM `SOFTWARE\Policies\Microsoft\Edge`: `HubsSidebarEnabled`, `CopilotPageContext`, `ComposeInlineEnabled`, `NewTabPageBingChatEnabled`, `GenAILocalFoundationalModelSettings` (1 = não baixar), `StartupBoostEnabled`, `BackgroundModeEnabled` | Microsoft Edge policies (learn.microsoft.com/deployedge/microsoft-edge-policies) | oficial; `CopilotCDPPageContext` obsoleta desde a 132 e `EdgeHistoryAISearchEnabled` sem doc: fora |
| `office_telemetry_off` | HKCU `Software\Policies\Microsoft\office\common\clienttelemetry\sendtelemetry` = 3 (Neither) | "Use policy settings to manage privacy controls for Microsoft 365 Apps" (learn.microsoft.com/microsoft-365-apps/privacy/manage-privacy-controls) | oficial; `DisableTelemetry` (sem doc) fora |
| `widgets_policy_off` | HKLM `Policies\Microsoft\Dsh\AllowNewsAndInterests`; `Policies\Microsoft\Windows\Windows Feeds\EnableFeeds` | ADMX NewsAndInterests / Feeds | oficial |
| `consumer_features_off` | HKLM `Policies\Microsoft\Windows\CloudContent`: `DisableWindowsConsumerFeatures`, `DisableConsumerAccountStateContent`, `DisableCloudOptimizedContent` | ADMX CloudContent (Enterprise/Education respeitam por inteiro) | oficial |
| `spotlight_off` | HKCU `Policies\…\CloudContent`: `DisableWindowsSpotlightFeatures`, `DisableSpotlightCollectionOnDesktop`; HKCU ContentDeliveryManager `RotatingLockScreenEnabled`, `RotatingLockScreenOverlayEnabled`, `SubscribedContent-338387Enabled` | ADMX CloudContent; os de ContentDeliveryManager são o que Configurações › Personalização › Tela de bloqueio grava | políticas oficiais; CDM observado |
| `telemetry_min` | HKLM `Policies\Microsoft\Windows\DataCollection\AllowTelemetry` = 0 | "Configure Windows diagnostic data" (0 vira Obrigatório no Home/Pro) | oficial |
| `delivery_p2p_off` | HKLM `Policies\Microsoft\Windows\DeliveryOptimization\DODownloadMode` = 0 | "Delivery Optimization reference" (0 = HTTP only) | oficial |
| `start_web_search_off` | HKCU `Policies\Microsoft\Windows\Explorer\DisableSearchBoxSuggestions`; HKLM `Policies\…\Windows Search`: `DisableWebSearch`, `ConnectedSearchUseWeb` | ADMX WindowsExplorer / Search | oficial (os dois de Windows Search valem mais no 10) |
| `account_notifications_off` | HKCU `Explorer\Advanced`: `Start_AccountNotifications`, `Start_IrisRecommendations`; `UserProfileEngagement\ScoobeSystemSettingEnabled` | o que Configurações › Personalização › Iniciar e › Notificações gravam | observado |
| `teams_chat_off`, `taskbar_left` | HKCU `Explorer\Advanced`: `TaskbarMn`, `TaskbarAl` | Configurações › Personalização › Barra de tarefas | observado |
| `error_reporting_off` | HKLM `SOFTWARE\Microsoft\Windows\Windows Error Reporting\Disabled` | "WER settings" (learn.microsoft.com/windows/win32/wer/wer-settings) | oficial |
| `fast_startup_off` | HKLM `…\Session Manager\Power\HiberbootEnabled` | o que Opções de Energia › "Ligar inicialização rápida" grava | observado |
| `ntfs_last_access_off` | HKLM `…\Control\FileSystem\NtfsDisableLastAccessUpdate` = 0x80000001 | `fsutil behavior set disablelastaccess` (doc do fsutil: 0x80000003 = gerenciado pelo sistema, desligado) | oficial |
| `network_throttling_off` | HKLM `…\Multimedia\SystemProfile\NetworkThrottlingIndex` = 0xFFFFFFFF | doc do MMCSS (padrão 10) | oficial; ganho discutível, dito na tela |
| `storage_sense_off` | HKCU `StorageSense\Parameters\StoragePolicy\01` | o que Configurações › Armazenamento grava | observado |
| `notifications_off` | HKCU `PushNotifications\ToastEnabled` | Configurações › Notificações | observado |
| `transparency_off` | HKCU `Themes\Personalize\EnableTransparency` | Configurações › Cores | observado |
| `classic_context_menu` | HKCU `Software\Classes\CLSID\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}\InprocServer32` vazio | mecanismo conhecido do Windows 11 (sombra do registro COM em HKCU) | observado; desfazer apaga a chave inteira se fomos nós que criamos |
| `alt_shift_off` | HKCU `Keyboard Layout\Toggle`: `Language Hotkey` e `Hotkey` = "3" | "Keyboard Layout Toggle" (3 = sem atalho); `SPI_SETLANGTOGGLE` recarrega | oficial |
| `narrator_shortcut_off` | HKCU `Software\Microsoft\Narrator\NoRoam\WinEnterLaunchEnabled` | Configurações › Acessibilidade › Narrador | observado |
| `accessibility_shortcuts_off` | `SPI_GET/SETSTICKYKEYS`, `…FILTERKEYS`, `…TOGGLEKEYS`, bit `*_HOTKEYACTIVE` (0x4) | doc de `SystemParametersInfo` e das structs | oficial |
| `wifi_power_saving_off`, `core_parking_off` | plano ativo: subgrupo `19cbb8fa-…`/`12bbebe6-…` (modo de economia do adaptador sem fio), `54533251-…`/`0cc5b647-…` (CPMINCORES) | "Processor power management options", `powercfg /q` | oficial |
| `nvidia_telemetry_off` | tarefas `\NvTmMon_`, `\NvTmRep_`, `\NvTmRepOnLogon_`, `\NvTmRep_CrashReport1..4_` + `{B2FE1952-0186-46C3-BAEC-A80AA35AC5B8}` | instalações antigas do driver | observado; drivers atuais não criam (indisponível) |
| `recall_feature_off` | recurso opcional `Recall` | `Disable-WindowsOptionalFeature -FeatureName Recall` documentado; feito pela DismApi (`DismDisableFeature`) | oficial |
| `diagtrack_manual` | serviço `DiagTrack` → Manual | [windows-services-research.md](windows-services-research.md) | pesquisa interna |

**Compressão de memória (recusada):** o módulo MMAgent é um CDXML sobre a classe WMI `Root\Microsoft\Windows\PS_MMAgent\PS_MMAgent` (lido em `System32\WindowsPowerShell\v1.0\Modules\MMAgent\ps_mmagent_v1.0.cdxml` nesta máquina). A classe não tem documentação própria como API; sem caminho nativo documentado, ficou de fora.

**Apps pré-instalados:** nomes de família e ids de editor da Store vistos em instalações do Windows 10/11 (`Get-AppxPackage` em máquinas de referência). Editor errado = não casa, então um id desatualizado só deixa de oferecer o app, nunca remove outro.
