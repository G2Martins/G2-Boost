# Atualizações (apps pelo winget, Windows Update pela WUA)

**Última revisão:** 2026-10-23 · Código: `crates/core/src/updates.rs` (sobre `winget.rs`), shell `src-tauri/src/lib.rs` (`updates_*`), UI `src/components/apps/updates-tab.tsx` (aba **Atualizações** em **Desinstalar apps**, `src/pages/apps.tsx`), MCP `get_updates` (`crates/mcp/src/lib.rs`), configuração `settings.json → update_excluded`.

Pedido do dono (2026-10-23): o equivalente a `winget upgrade --all` dentro do app, mais o Windows Update, sem shell.

## Apps (winget)

- **Lista:** `winget::upgrades()` (o mesmo `winget.exe` da exceção registrada em [global-rules §1.3](./global-rules.md)), classificada em `updates::classify`:
  - `skip = self`: o próprio G2 Boost (nome "G2 Boost" ou id com "G2Boost"); atualizar a si mesmo no meio do lote mataria o processo.
  - `skip = unknown_version`: versão instalada ou disponível sem dígito ("Unknown"/"Desconhecido").
  - `skip = excluded`: ids que o usuário tirou com **Não atualizar** (`settings.json → update_excluded`, validado no uso por `updates::excluded_ids`: id válido, sem repetição, até 300).
  - `machine`: o nome bate com um app instalado em HKLM (`apps::list`, `needs_admin`); nomes cortados pelo winget ("…") batem pelo prefixo. Aviso só: o instalador desses deve abrir o UAC do Windows quando o G2 Boost não está elevado (a UI avisa antes e no modal).
- **Atualizar todos/selecionados:** nunca `--all`. O shell guarda a última lista no core (`APPS_LAST`); `updates_apps_confirm(ids)` só emite o token se cada id veio dessa lista, não é pulado e não se repete (até 200). O token (uso único, 2 min) é amarrado ao tipo e à **sequência exata** de ids (`updates:apps:<id>`). `updates_apps_run` consome o token e chama `winget::install(id, true)` um por vez (o mesmo comando validado do console: `upgrade --id X --exact --silent --accept-package-agreements` + `COMMON`), cada um registrado também em `winget-log.json`.
- **Progresso:** evento `updates:apps` (`index`, `total`, `id`, `name`, `status` = running/ok/failed/cancelled, `reason`, `code`) antes e depois de cada pacote.
- **Cancelar:** `updates_apps_cancel` vale entre pacotes; o pacote em andamento termina (matar um instalador no meio pode deixar o app quebrado). Os que faltavam entram como `cancelled`.
- **Código de saída → motivo** (`updates::reason`): `0x8A150109/0x8A15010B/3010/1641` reinicie para concluir (conta como ok), `0x8A15010A` reinicie antes, `0x8A150101/0103/0111` app aberto, `0x8A150102/1618` outra instalação, `0x8A15010C/1602/1223` cancelado (UAC recusado), `0x8A150019` precisa de admin, `0x8A15002B/0010/0014/010D` nada aplicável, `0x8A150008/0107` download, `0x8A150011` hash, `0x8A150105` disco cheio, `0x8A15010F/001B/001C` política, `0x8A150006` instalador não abriu; o resto é "falhou (código)".
- Um lote por vez por processo (`busy`). A trava vale também para o lote da aba **Instalar** ([install-apps.md](./install-apps.md)), que usa o mesmo padrão de token amarrado à lista ordenada; a aba Instalar mostra "há atualização" nos apps do catálogo (coluna Disponível do `winget list`) e leva para cá.

## Windows Update (API do Windows, sem `usoclient`/`wuauclt`/PowerShell)

- **COM:** `CoCreateInstance(UpdateSession)` → `CreateUpdateSearcher`/`CreateUpdateDownloader`/`CreateUpdateInstaller`, feature `Win32_System_UpdateAgent` do crate `windows` (nenhum crate novo). `ClientApplicationID = "G2 Boost"` (aparece no histórico do Windows).
- **Thread própria em MTA** (`updates::mta`): a WUA chama de volta nas threads dela; num STA sem laço de mensagens o job nunca completa. Os `Begin*` recebem um callback vazio (`Noop`, `#[implement]` das 5 interfaces) e o job é consultado a cada 300 ms (`IsCompleted`, `GetProgress`). Cancelar ou estourar o prazo chama `RequestAbort` e espera até 2 min para ler o resultado por atualização.
- **Procurar:** `Search("IsInstalled=0 and IsHidden=0")`, sem admin. `online = false` lê a última verificação do Windows (cache local, segundos; a aba abre assim e o MCP usa só isso); **Verificar agora** pergunta à Microsoft (até 5 min). Por atualização: `UpdateID` (GUID), título, KBs, `MaxDownloadSize`, `MsrcSeverity`, categorias, driver (`Type`), reinício (`InstallationBehavior.RebootBehavior`), já baixada, obrigatória. Drivers vêm desmarcados, como em Configurações.
- **Instalar (admin, `needs_admin` sem):** `updates_wu_confirm(ids)` exige o app elevado e ids GUID da última busca (até 100); token `updates:wu:<id>`. A instalação busca de novo só esses ids (`UpdateID='…' or …`, seguro porque o GUID é validado: só hex e hífens nas posições certas), primeiro no cache e, se faltar algum, online; aceita a licença das atualizações escolhidas (o usuário confirmou, como em Configurações), baixa (`BeginDownload`) e instala só o que baixou (`BeginInstall`). Evento `updates:wu` (`phase` download/install, `percent`, `current`, `total`). Resultado por atualização (`ResultCode`, `HResult`, `RebootRequired`).
- **Nunca reinicia sozinho.** Com reinício pendente (`ISystemInformation.RebootRequired` ou o resultado da instalação), a UI mostra **Reiniciar agora** com modal; `updates_reboot` recusa (`no_reboot_needed`) se nada estiver pendente e usa `InitiateShutdownW(SHUTDOWN_RESTART)` sem forçar apps (eles podem pedir para salvar).
- **Histórico do Windows** ("Get-HotFix"): `QueryHistory` (mais recentes primeiro), título, KB extraído do título, data (OLE → Unix), instalação/remoção, resultado, HRESULT. Pacotes da Store gravam uma entrada por arquitetura: entradas iguais seguidas são agrupadas; a UI filtra "Só com KB" por padrão.

## Histórico do G2 Boost

`updates-history.json` no data dir (50 execuções; tipo `apps`/`windows`, itens com status/motivo/código, reinício, cancelado). Arquivo hostil na leitura: até 2 MB, tipos desconhecidos fora, strings sem controle e limitadas, status/motivo fora do formato viram `failed`.

## MCP

`get_updates` (só leitura): apps com versão nova (classificados com as exclusões do usuário), pendências do Windows **do cache** (rápido, sem rede), reinício pendente e as 10 últimas do histórico do Windows. Instalar fica no app.

## Testes

`updates::tests`: classificação (próprio app, versão desconhecida, excluído sem diferenciar caixa, HKLM, nome cortado), lista de exclusão validada, códigos → motivos, GUID (inclusive tentativa de injetar critério), KB e data OLE, token amarrado à lista/ordem/tipo e de uso único, `needs_admin` sem elevação, histórico hostil (JSON inválido, NUL, status falso). **Ao vivo (`--ignored live_updates --nocapture`, só leitura, nunca instala):** 2026-10-23 nesta máquina: winget 9 atualizações (9 elegíveis, 7 para todos os usuários); Windows Update 1 pendente (driver de monitor, sem KB, 24 KB) no cache e online (cache em segundos, online ~40 s); histórico com 10 entradas depois de agrupar as repetições da Store, 3 com KB (KB5127648, KB5127690, KB890830). E2E: `e2e/apps.spec.ts` abre a aba no navegador (botões desabilitados, aviso de admin).

## Pendente (passos elevados)

- Instalar uma atualização real do Windows com o app elevado (driver opcional ou KB pequeno), conferir progresso, resultado e "Reiniciar agora".
- Lote real de apps sem e com elevação (UAC por app de máquina; cancelar entre pacotes).
- Limitação conhecida: o winget corta nomes e ids longos na tabela; um id cortado ("…") não é válido e o pacote some da lista (mesmo comportamento do console).
