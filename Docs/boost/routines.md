# Rotinas — iniciar com o Windows, limpeza agendada, RAM ao abrir jogo, bandeja

**Última revisão:** 2026-10-23 (sempre abrir como administrador) · 2026-10-15 · Código: `crates/core/src/settings.rs`, `routines.rs`, `tasksched.rs`; shell `src-tauri/src/lib.rs` (`Routines`, `spawn_routines`, bandeja) · UI: `src/pages/settings.tsx`, aba RAM em `src/pages/boost.tsx`, `src/components/game-exes.tsx`.

## Configurações (`settings.json`)

`%LOCALAPPDATA%\G2 Boost\settings.json`, validado no load **e** antes de salvar (`settings::sanitize`): categorias desconhecidas caem fora, exes viram `nome.exe` minúsculo sem caminho (máx. 50), horas 1–168, hora 0–23, dia 0–6.

| Campo | Padrão | Uso |
|---|---|---|
| `close_to_tray` | `true` | X da janela só esconde enquanto uma rotina estiver ativa |
| `schedule` | `off` | `at_logon` · `every_hours{hours}` · `daily{hour}` · `weekly{weekday,hour}` |
| `routine_categories` | `default_on` do catálogo **sem admin** | shaders e Lixeira são opt-in; categorias admin só rodam com o app elevado |
| `game_ram_clean` | `false` | purge da standby list ao detectar jogo |
| `game_exes` | `[]` | jogos fora da Steam |

## Sempre abrir como administrador (`always_admin`)

Desligado por padrão. Ligado: toda abertura normal sem elevação chama `boost::relaunch_elevated` (UAC via `ShellExecuteExW("runas")`, nunca PowerShell) passando só `--route` validado, e a cópia sem elevação encerra; se o usuário recusar o UAC, a cópia sem elevação segue normalmente (sem laço). Não vale para `--mcp` (o Claude Desktop abre o exe; um UAC quebraria o MCP), `--gpu-helper` nem `--tray` (logon; a tarefa já é `Highest` quando registrada com o app elevado). **Por que não sem UAC:** a alternativa "silenciosa" (tarefa agendada com privilégios mais altos disparada pelo próprio app) vira um desvio de UAC enquanto o exe mora numa pasta gravável pelo usuário (instalação por usuário em `%LOCALAPPDATA%\Programs`): qualquer programa do usuário trocaria o exe e ganharia admin. Mesmo comportamento do ExitLag (pedido a cada abertura).

## Iniciar com o Windows

`autostart_set(true)` registra a tarefa **`G2 Boost`** na raiz do Agendador (`tasksched::register_logon_task`): gatilho de logon **do usuário atual** (sem `UserId` o Windows exige admin), atraso 10 s, ação `g2-boost.exe --tray`, `StartWhenAvailable`, sem limite de tempo, sem parar na bateria, instâncias ignoradas.

- App **elevado** ao ligar a opção → `RunLevel = Highest`: inicia como admin sem UAC (único jeito; padrão CCleaner). Rotinas de sistema e purge funcionam.
- App **comum** → `RunLevel = LUA`: inicia sem admin; rotinas que precisam de admin devolvem `needs_admin` e ficam no histórico como erro. A UI explica e oferece "Elevar".
- Desligar = `delete_task`. Estado = `task_exists` (a tarefa é a fonte da verdade, não o JSON).
- Nada em `HKCU\Run` (não serve para iniciar elevado) e nada em `Policies`.

`--tray` esconde a janela principal logo após criar; a bandeja (ícone do app) abre com clique esquerdo ou "Abrir" e tem "Sair".

## Limpeza agendada

Thread única no shell (`spawn_routines`): a cada 60 s avalia `routines::due(schedule, Clock::now(), last_run, ran_since_start)`:

| Modo | Dispara quando |
|---|---|
| `at_logon` | uma vez por processo (o app inicia com o Windows → uma vez por logon) |
| `every_hours{n}` | `agora − última ≥ n h` |
| `daily{h}` | hora atual ≥ h e a última execução foi antes do slot `h:00` de hoje |
| `weekly{d,h}` | dia da semana = d, hora ≥ h, última antes do slot de hoje |

Executa `cleanup::execute(routine_categories, "routine")` **sem token**: a confirmação humana foi o ato de ligar a rotina e marcar as categorias (decisão documentada; o MCP não tem comando de settings). Resultado vai para o histórico com gatilho `routine` e evento `routine:done`. `last_run` fica em `routine.json`.

## RAM ao abrir jogo

A cada 5 s (`game_ram_clean` ligado): `routines::detect_game(exes)`:

1. **Steam:** `HKCU\Software\Valve\Steam\RunningAppID ≠ 0` → nome em `Apps\<id>\Name`.
2. **Lista do usuário:** processo cujo exe está em `game_exes`.
3. **Layout Steam:** processo com caminho contendo `\steamapps\common\`.

`GameWatcher` é um detector de borda: só dispara na transição "sem jogo → jogo"; o primeiro tick é baseline (jogo já aberto quando o app sobe não dispara). Ao disparar: `boost::purge_standby_list()` (admin; senão registra `needs_admin`), grava `RunRecord{trigger:"game", items:[ram_purge]}` no histórico e emite `game:started{game, freed_bytes, error}` (aba RAM mostra "Último: X — Y liberados").

Fora: trim de working set (piora), "lista de permissões" (só fazia sentido com trim por processo).

## Bandeja e fechar

`on_window_event(CloseRequested)`: se `close_to_tray` e (`schedule ≠ off` ou `game_ram_clean`) → `prevent_close` + `hide`. Caso contrário fecha normal. Menu da bandeja: Abrir / Sair (rótulos em pt-BR, idioma padrão; i18n do menu nativo = backlog).

## Testes

- `settings`: sanitize (categorias, exes hostis `C:\evil\game.exe`, clamps), round-trip JSON com `mode` tagueado e campos ausentes.
- `routines`: regras de `due` para os 5 modos com relógio fixo; matcher de processos (steamapps, lista, negativo); detector de borda (baseline, start, sem repetição).
- `tasksched`: round-trip de tarefa LUA como usuário comum (ignored, passou em 2026-10-15).

**Não verificado pelo agente:** tarefa `Highest` (precisa de app elevado), purge real ao abrir jogo (admin), comportamento da bandeja/`--tray` em logon de verdade. Checklist para o dono: ligar "Iniciar com o Windows" elevado, reiniciar sessão, conferir ícone na bandeja e histórico.
