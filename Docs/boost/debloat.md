# Debloat: o que o Windows trouxe sem você pedir

**Última revisão:** 2026-10-25 (rodada 9: página nova) · Código: `crates/core/src/boost.rs` (ajustes), `winfeature.rs` (recurso Recall pela DismApi), `debloat.rs` (apps pré-instalados), `recommend.rs`; shell `src-tauri/src/tweaks_cmds.rs`; UI `src/pages/debloat.tsx`, `src/components/boost/tweak-kit.tsx`, níveis em `src/data/catalogs.ts` · Fontes de cada chave: [../references/debloat-sources.md](../references/debloat-sources.md) · Mecânica de backup/revert: [tweaks.md](tweaks.md).

Rota `debloat`. Uma linha diz o que é (telemetria, anúncios, IA e apps de brinde) e o resto é um livro-razão: tudo com prévia do que muda, backup antes e desfazer item a item. Nada aqui mexe em Windows Update, Defender, firewall, UAC ou Prefetch (teste `nothing_forbidden_in_the_catalog`).

## Tela

| Peça | O que faz |
|---|---|
| Barra de comando | "N de M aplicados" (M = itens que existem neste PC; desconhecido por falta de admin conta), medidor, **Marcar todos / Desmarcar todos** (só o que dá para aplicar agora: sem admin, os itens admin ficam travados), **Aplicar selecionados** (o CTA tátil da página). |
| Confirmação | Lista o que vai mudar com o selo de reinício de cada item e oferece **ponto de restauração antes** (marcado conforme `restore_before_preset`, só elevado). Depois aplica um a um, mostra o progresso e solta um toast "N ajustes aplicados" com **Desfazer** (desfaz o lote em ordem inversa). |
| Remover a IA do Windows | Card com os 7 itens do conjunto de IA (luzes acesas = aplicado), um botão que manda o conjunto para a mesma confirmação, e o aviso de que o app Copilot sai pela lista de apps. |
| Tabela por nível | Essencial / Recomendado / Avançado como grupos de uma tabela densa: caixa de seleção, ícone, nome e o que faz, selo de admin (só sem elevação) e de reinício, estado (**Aplicado / Não aplicado / Parcial / Desconhecido (precisa de admin) / Indisponível neste PC**), **Desfazer** na linha e **Ver o que muda** (abre a lista exata de escritas e o "não recomendado se…"). |
| Apps pré-instalados | Lista o que está instalado da allowlist, caixa por app, **Remover selecionados** (vermelho) → modal com os nomes → token → remoção → toast e erros por app. |

Estado "Desconhecido (precisa de admin)" = `status: unavailable` + `detail: needs_admin`: só o recurso Recall precisa de admin para ser lido (a DismApi não abre sessão sem elevação).

## Níveis

| Nível | Itens |
|---|---|
| Essencial | `tips`, `widgets_off`, `search_highlights_off`, `silent_apps_off`, `advertising_id_off`, `edge_background_off`, `teams_chat_off`, `consumer_features_off`, `account_notifications_off` |
| Recomendado | `compat_telemetry_off`, `feedback_tasks_off`, `widgets_policy_off`, `telemetry_min`, `diagtrack_manual`, `copilot_off`, `recall_off`, `click_to_do_off`, `paint_ai_off`, `notepad_ai_off`, `edge_ai_off`, `delivery_p2p_off`, `start_web_search_off`, `spotlight_off`, `office_telemetry_off`, `nvidia_telemetry_off` |
| Avançado | `svc_telemetry`, `svc_geolocation`, `recall_feature_off`, `error_reporting_off` |

O nível mora no catálogo da UI (`level`); a mecânica, no core. Os mesmos ids aparecem no PC Boost › Ajustes de sistema (com busca e filtros) e servem à página de Perfis (conjunto de IA, teclado).

**Conjunto de IA** (`AI_SET` em `catalogs.ts`): `copilot_off`, `recall_off`, `click_to_do_off`, `recall_feature_off`, `paint_ai_off`, `notepad_ai_off`, `edge_ai_off`.

## Ajustes novos (mecânica)

Todos com backup exato e desfazer; políticas são escritas nativas no registro (`RegSetValueExW`), nunca `reg.exe`.

| id | Grava | Admin | Vale |
|---|---|---|---|
| `edge_background_off` | HKLM `Policies\Microsoft\Edge`: `StartupBoostEnabled=0`, `BackgroundModeEnabled=0` | sim | ao reabrir o Edge |
| `widgets_policy_off` | HKLM `Policies\Microsoft\Dsh\AllowNewsAndInterests=0`, `…\Windows\Windows Feeds\EnableFeeds=0` | sim | sair e entrar |
| `teams_chat_off` | HKCU `Explorer\Advanced\TaskbarMn=0` (só nos builds 22000–22630; nos outros fica indisponível) | não | na hora |
| `consumer_features_off` | HKLM `Policies\…\CloudContent`: `DisableWindowsConsumerFeatures=1`, `DisableConsumerAccountStateContent=1`, `DisableCloudOptimizedContent=1` (Home/Pro respeitam parte; dito na tela) | sim | |
| `telemetry_min` | HKLM `Policies\…\DataCollection\AllowTelemetry=0` (o mínimo de cada edição) | sim | reiniciar |
| `diagtrack_manual` | serviço `DiagTrack` → Manual | sim | reiniciar |
| `copilot_off` | HKCU `Policies\…\WindowsCopilot\TurnOffWindowsCopilot=1` (política só de usuário; obsoleta e ignorada pelo app Copilot da Store, por isso o app sai pela lista de apps), HKCU `ShowCopilotButton=0` | sim | sair e entrar |
| `recall_off` | `DisableAIDataAnalysis=1` em HKCU e HKLM `Policies\…\WindowsAI` | sim | sair e entrar |
| `click_to_do_off` | `DisableClickToDo=1` em HKCU e HKLM `Policies\…\WindowsAI` | sim | sair e entrar |
| `recall_feature_off` | recurso opcional `Recall` desativado pela DismApi (payload mantido) | sim | reiniciar |
| `paint_ai_off` | HKLM `CurrentVersion\Policies\Paint`: `DisableCocreator`, `DisableGenerativeFill`, `DisableImageCreator` = 1 (só as três documentadas; apagar generativo e remover fundo não têm política publicada) | sim | ao reabrir |
| `notepad_ai_off` | HKLM `Policies\WindowsNotepad\DisableAIFeatures=1` | sim | ao reabrir |
| `edge_ai_off` | HKLM `Policies\Microsoft\Edge`: `HubsSidebarEnabled`, `CopilotPageContext`, `ComposeInlineEnabled`, `NewTabPageBingChatEnabled` = 0; `GenAILocalFoundationalModelSettings=1` (`CopilotCDPPageContext` ficou de fora: obsoleta desde o Edge 132) | sim | ao reabrir o Edge |
| `delivery_p2p_off` | HKLM `Policies\…\DeliveryOptimization\DODownloadMode=0` (HTTP só; o Windows Update continua) | sim | |
| `start_web_search_off` | HKCU `Policies\…\Explorer\DisableSearchBoxSuggestions=1`; HKLM `Policies\…\Windows Search`: `DisableWebSearch=1`, `ConnectedSearchUseWeb=0` | sim | reiniciar o Explorer |
| `account_notifications_off` | HKCU `Explorer\Advanced`: `Start_AccountNotifications=0`, `Start_IrisRecommendations=0`; `UserProfileEngagement\ScoobeSystemSettingEnabled=0` | não | reiniciar o Explorer |
| `spotlight_off` | HKCU CDM `RotatingLockScreenEnabled=0`, `RotatingLockScreenOverlayEnabled=0`, `SubscribedContent-338387Enabled=0`; HKCU `Policies\…\CloudContent`: `DisableWindowsSpotlightFeatures=1`, `DisableSpotlightCollectionOnDesktop=1` | sim (HKCU\Policies só aceita escrita elevada) | sair e entrar |
| `office_telemetry_off` | HKCU `Policies\Microsoft\Office\Common\ClientTelemetry\SendTelemetry=3` ("Nenhum": nem opcionais nem obrigatórios; dados de serviço indispensáveis continuam); só aparece com Office instalado | sim | ao reabrir o Office |
| `nvidia_telemetry_off` | tarefas `\NvTmMon_…`, `\NvTmRep_…`, `\NvTmRepOnLogon_…`, `\NvTmRep_CrashReport1..4_…` (GUID `{B2FE1952-…}`) desligadas; drivers recentes não têm, então fica indisponível | sim | |
| `error_reporting_off` | HKLM `Windows Error Reporting\Disabled=1` | sim | |

`HKCU\Software\Policies` só aceita escrita de processo elevado (ACL do Windows), por isso `copilot_off`, `recall_off`, `click_to_do_off`, `spotlight_off`, `start_web_search_off` e `office_telemetry_off` pedem admin mesmo gravando (só ou também) em HKCU. Elevação com outra conta de administrador grava no HKCU dessa outra conta: caso raro, não tratado.

## Remover o componente Recall (`winfeature.rs`)

- `DismGetFeatureInfo` / `DismDisableFeature` / `DismEnableFeature` da `DismApi.dll` (System32, `LOAD_LIBRARY_SEARCH_SYSTEM32`). `DismInitialize` é uma vez por processo e pertence ao `repair` (`repair::dism_ready`); o slot `repair::exclusive()` serializa com DISM/SFC/limpeza de sistema (ocupado = erro `busy`, a UI explica).
- Allowlist `winfeature::ALLOWED = ["Recall"]`: qualquer outro nome volta `unknown_feature` antes de checar admin ou abrir sessão (teste com nomes hostis).
- Estado: Instalado = ajuste desligado; Preparado/Removido = ligado; pendentes viram `detail = reboot_pending`; ausente (Windows 10, 11 antigo) = indisponível. Leitura só elevada; a resposta fica em cache no processo (uma sessão DISM custa segundos) e `set` limpa a entrada.
- Desativa **sem** remover o payload, então o desfazer (`DismEnableFeature`) funciona sem internet. `ERROR_SUCCESS_REBOOT_REQUIRED` conta como sucesso.

## Apps pré-instalados (`debloat.rs`)

- **Allowlist** por nome exato + id do editor da Store (ou prefixo + editor, só `king.com.`): Notícias, Clima, Dicas, Solitaire, Hub de Comentários, Mapas, Visualizador 3D, Print 3D, Portal de Realidade Misturada, Email e Calendário antigo, Pessoas e Mensagens antigos, Copilot, Cortana, Family, Power Automate, Microsoft 365 (hub), Planos Móveis, Skype, Clipchamp, Candy Crush e família, e promoções de streaming (Spotify, Netflix, Disney+, Prime Video, TikTok). Nada vem marcado.
- **Nunca**, mesmo se alguém mexer na allowlist (`NEVER`, testado contra sobreposição): Xbox e Game Bar, Store e App Installer, Fotos, Calculadora, Terminal, Bloco de Notas, Paint (inclusive o Paint 3D), Câmera, Ferramenta de Captura, segurança do Windows, runtimes, codecs e apps de fabricantes (NVIDIA, AMD, Intel, Realtek, Dolby, Waves, Synaptics, Elan).
- Fluxo: `debloat_apps` lista (a partir de `apps::list`, Store, sem os protegidos pelo usuário) e guarda a última lista → `debloat_apps_confirm(ids)` aceita só ids dessa lista, sem repetição, até 64, e emite token de uso único (2 min) amarrado aos ids → `debloat_apps_remove(token, ids)` consome o token, relê os pacotes e confere de novo cada um (ainda instalado, ainda na allowlist) antes de `apps::uninstall` (PackageManager, só para a conta atual). Família hostil (`..`, caminho, editor errado, nome mais longo, NUL) não casa (teste).
- Log em `%LOCALAPPDATA%\G2 Boost\debloat-apps-log.json` (últimas 200: quando, nome, ok, erro; sem caminhos).
- Para voltar: Microsoft Store. Pacotes provisionados podem voltar numa atualização grande do Windows ou numa conta nova (o G2 Boost remove só para a conta atual, de propósito).

## Fora, de propósito

- **Compressão de memória:** o único jeito é o módulo MMAgent do PowerShell (`Disable-MMAgent -mc`), que por baixo é a classe WMI `PS_MMAgent` em `root\Microsoft\Windows\PS_MMAgent` (CDXML), sem API Win32 documentada. Chamar essa classe direto seria depender de um detalhe interno do cmdlet. E o ganho não existe em PC com 16 GB ou mais; com pouca RAM, desligar piora (mais paginação). Não entrou.
- Windows Update em "só avisar" ou desligado, Defender, firewall, UAC, Prefetch: promessa do produto.
- Remover Edge, OneDrive, Xbox, Store: quebra coisas ou não volta pela Store.

## Testes

- Core: `catalog_matches_the_ui_and_the_dictionaries` (mesmos ids, categoria, admin e beta no TS; nome/descrição/aviso nos dois idiomas), `nothing_forbidden_in_the_catalog`, `every_tweak_says_what_it_changes`, `service_targets_never_loosen`, `gated_tweaks_are_unavailable_where_they_mean_nothing`, `empty_key_is_removed_only_if_we_created_it` (chave descartável em HKCU), restauração com backup adulterado ignorando o que não é do ajuste, `winfeature::only_allowlisted_names_reach_dism`, `debloat::*` (allowlist com nomes hostis, sobreposição com `NEVER`, token amarrado aos ids).
- Elevados (`#[ignore]`): `round9_roundtrip_elevated` (liga e desliga cada ajuste novo, menos o recurso DISM, e confere o estado final) e `recall_state_elevated` (só leitura): `cargo test -p g2boost-core -- --ignored round9_roundtrip_elevated recall_state_elevated` num terminal elevado com `G2BOOST_DATA_DIR` apontando para uma pasta de teste.
- E2E: `e2e/debloat.spec.ts` (níveis, card de IA, apps; marcar e desmarcar; ver o que muda) e `e2e/boost.spec.ts` (busca, filtro por categoria, ver o que muda).

**Não verificado pelo agente (precisa do dono, elevado):** gravação real das políticas HKLM, o recurso Recall pela DismApi (num PC Copilot+ e num comum), a remoção real de apps da Store, o efeito visível de cada política no build do dono, e a reação do Kaspersky a gravações em `Policies` (registrar em [../security/kaspersky-false-positive.md](../security/kaspersky-false-positive.md) se acusar).
