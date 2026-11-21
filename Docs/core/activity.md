# Atividade — sino, notificações, execuções e jobs em andamento

**Última revisão:** 2026-11-21 (`boot.startTab` lido uma vez: `takeStartTab`) · 2026-11-20 (rodada 11, bloco A1)

Pedido do dono: uma aba de notificações e execuções para o usuário, feedback visual do que o app está fazendo
(limpando, verificando, atualizando) e motion melhor. Tudo local: nada sai do PC, sem telemetria.

## 1. Peças

| Peça | Onde | O que faz |
|---|---|---|
| Linhas do sino | `crates/core/src/activity.rs` (base) + `crate::notify` no shell | Uma linha = chave i18n + args pequenos (sem caminho, usuário ou PII), tom, rota. `activity.json`, máx. 200, arquivo relido como entrada hostil. Evento `activity:new`. MCP `get_activity`. |
| Store de jobs + feed | `src/lib/jobs.ts` | Store de módulo (`useSyncExternalStore`, como o toast) ligada no boot do `App` (`startJobs`), então sair da página não perde o progresso. |
| Sino | `src/components/shell/activity.tsx` (`ActivityBell`) | Topbar: contador de não lidas (chip accent que "pula" ao mudar, sino balança uma vez), `[popover]` nativo com as últimas 10 linhas, "Marcar como lidas", "Ver tudo". Lidas ao fechar o popover. |
| Pílula | `JobPill` (mesmo arquivo) | Job rodando numa página que não está na tela: anel de progresso (ou anel girando sem total), rótulo, %, bytes. Ao terminar vira check que se desenha (ou X) por 2,4 s e sai. Clique volta para a página do job. |
| Ponto na sidebar | `AppShell` (`busy`) | Item do módulo com job rodando ganha um ponto accent que pulsa. |
| Toast de linha nova | `ActivityEffects` | Linha nova vira toast **só** se a página dela não está na tela; se há uma pílula daquela página, a linha vira a nota da pílula (sem toast duplicado). |
| Notificação do Windows | `activity_cmds::activity_os_notify` + `g2boost_core::winnotify` | Janela na bandeja ou minimizada: toast do Windows (clique reabre na rota); sem atalho do instalador, pisca o botão da barra de tarefas. Configurações › "Avisos do Windows" (`notify_native`, padrão ligado). |
| Página Atividade | `src/pages/activity.tsx` | Abas **Notificações** (feed por dia, marcar lidas, limpar) e **Execuções** (linha do tempo unificada, filtro por módulo, "Em andamento"). |
| Normalização | `src/lib/runs.ts` (puro, `tests/runs.test.ts`) | Junta os históricos que o core já guarda; não cria storage novo. Também o texto das linhas (`lineText`). |

## 2. Jobs: começo, progresso, fim

- **Começo:** `backend.ts` → `callOrThrow` avisa um único observador (`watchCalls`) quando um comando longo começa e
  quando ele termina. Comandos em `CALLS` (`jobs.ts`): `cleanup_preview/execute`, `volcache_preview/execute`,
  `repair_run`, `volumes_run`, `maint_plan_run`, `updates_apps_run`, `updates_wu_install`, `install_run`.
  Varreduras que voltam na hora e reportam por evento (`disk_scan_start`, `dupes_start`) nascem do 1º evento.
- **Progresso:** `cleanup:measure`, `cleanup:progress`, `volcache:progress`, `disk:progress`, `dupes:progress`,
  `maintenance:progress`, `maintenance:volume-progress`, `maintenance:plan-progress`, `updates:apps`, `updates:wu`,
  `install:progress`. Um job por tipo; evento sem job ativo "adota" um.
- **Fim:** a promessa do comando assenta (ok/erro), `disk:done`/`disk:error`, `dupes:done`, ou o evento final da
  limpeza (`current = null` com `done = total`). Evento atrasado de um job que acabou de terminar (< 3 s) é
  ignorado; job "adotado" (nascido de evento, sem chamada para assentar) que fica 2 min sem evento é descartado.
- **Dono:** o job pertence à página que o começou (`setHere` no `useLayoutEffect` do App), não ao módulo. A medida do
  Painel e as instalações da Pós-formatação não acendem pílula enquanto o usuário está nelas.

## 3. Linhas que o core levanta (rodada 11)

| Onde | Chave | Tom | Rota |
|---|---|---|---|
| Rotina de limpeza (base) | `activity.routine.cleanup` | ok | cleanup |
| Jogo detectado + RAM (`on_game_started`) | `activity.game.ram` / `.ram_failed` | ok / warn | games |
| Monitor de exposição viu algo novo (`exposure_tick`) | `activity.security.alerts` | warn | security |
| Fim do lote de apps / Windows Update (`activity_cmds::updates_done`) | `activity.updates.{apps,wu}.{ok,partial}`, `.reboot`, `.cancelled` | ok / warn / err / info | apps |
| Fim da revisão semanal / reparo completo (`activity_cmds::plan_done`) | `activity.maint.{weekly,repair}.{ok,issues}`, `.cancelled` | ok / warn / err / info | maintenance |

Args seguem a convenção da base: `bytes` vira tamanho, `count`/`ok`/`total`/`failed` número, o resto texto literal
(nome do jogo). Chave que este build não conhece aparece como "Algo aconteceu em {kind}". Linha de jogo não gera
notificação do Windows (a pessoa está jogando; o sino basta).

## 4. Notificação do Windows (decisão)

- **Avaliado `tauri-plugin-notification`:** puxa `notify-rust` + `tauri-winrt-notification`, um invólucro da mesma API
  WinRT, e em dev usa o app id do PowerShell. Dependência e capability novas para ~40 linhas.
- **Escolhido:** WinRT direto pelo crate `windows` que já está no core (features `UI_Notifications`,
  `Data_Xml_Dom`, sem código C). `ToastNotificationManager::CreateToastNotifierWithId("com.grovicdata.g2boost")`.
  O instalador NSIS grava esse AppUserModelID no atalho do menu Iniciar (conferido no atalho instalado:
  `System.AppUserModel.ID = com.grovicdata.g2boost`). Sem o atalho o Windows descarta o toast sem erro, então
  `winnotify::available()` procura o atalho antes (cópia portátil e build de dev em PC limpo: pisca a barra de tarefas
  via `request_user_attention` = `FlashWindowEx`; na bandeja sem toast, o sino guarda a linha).
- **App elevado (2026-11-20, 0.10.1):** o Windows descarta, sem erro, toast de app sem pacote rodando como
  administrador (confirmado no PC do dono: o handler `com.grovicdata.g2boost` aparece no banco de notificações, mas
  nenhuma notificação fica; o mesmo teste sem admin entrega). Elevado, `winnotify::show_unelevated` abre
  `G2 Boost.exe --toast "<título>" "<texto>"` com os direitos normais do usuário pelo Explorer (mesmo caminho do
  "Preparar e jogar", `games::run_unelevated`); esse processo mostra o toast e sai antes de subir o Tauri. Argumentos
  limpos, cortados e sem `"`/`\` (não fecham as aspas nem viram outra flag). Limite: o clique nesse toast não reabre a
  rota (nenhum handler sobrevive ao processo auxiliar).
- **Texto:** o webview renderiza (tem o i18n) e manda; o core trata como hostil: tira controle e bidi, corta (80/200),
  escapa XML, documento só com dois `<text>` (sem imagem, ação ou URI). Rota do clique só se estiver em `ROUTES`.
- **Quando:** linha nova (menos `game`) e fim de job; o core responde `visible` se a janela está na tela. Uma por rota
  a cada 5 s (a linha do job chega antes do fim e leva o texto mais rico).

## 5. Execuções (fontes)

`cleanupHistory` (manual, rotina, jogo, sobras de apps, Lixeira do disco, IA, arquivos do sistema), `repairHistory`,
`volumesHistory`, `maintPlanHistory`, `maintFixHistory`, `updatesHistory`, `installHistory`, `profilesHistory`
(passos do mesmo perfil a menos de 60 s viram uma execução). Ordenadas pelo **fim** (início + duração): a revisão
semanal fica acima dos passos que ela rodou (os passos continuam com linha própria, como no histórico da Manutenção).
Máx. 300. Clique abre a página dona (com `boot.startTab` quando há aba: Limpeza › Arquivos do sistema, Apps ›
Atualizações/Instalar/Sobras, PC Boost › RAM, Perfis › Restauração). A página lê a aba **uma vez**
(`takeStartTab()` de `src/lib/boot.ts`, num inicializador de `useState`): a visita seguinte abre na aba padrão, e a
varredura de Sobras pedida por `unowned` não roda de novo ao voltar para a aba. Fora do app: estado vazio com
"Disponível só dentro do app".

## 6. Testes

- `tests/runs.test.ts`: ordem, rotas/abas, agrupamento de perfil, gatilho desconhecido, toda chave existe, linha com
  chave desconhecida e arg com markup (fica texto), durações, dias.
- `winnotify::tests`: XML com markup hostil, controle e bidi; `shows_a_real_toast` (`--ignored`, mostra um toast de
  verdade; passou nesta máquina em 2026-11-20).
- `e2e/activity.spec.ts`: página nas duas abas fora do app, sino vazio + "Ver tudo", linha nova (contador, toast fora
  da rota, popover, clique abre a página, lida ao fechar), pílula + ponto do disco até `disk:done`, sem pílula na
  própria página, reduced-motion (nada anima, tudo visível).

## 7. Não testado / limites

- Toast pelo app instalado e elevado com a janela na bandeja (o caminho `--toast` foi conferido sem admin; o
  lançamento sem admin pelo Explorer é o mesmo do "Preparar e jogar").
- Fim de job com a janela escondida: a notificação depende do JS receber o evento com o webview escondido (esperado,
  não verificado no app real); timers de página escondida podem atrasar o aviso de fim de job (espera 1,5 s).
- Rota igual com aba diferente (ex.: estar em Apps › Instalar e clicar numa execução de Atualizações) não troca a aba:
  a página não remonta.
