# Manutenção — verificar e reparar o Windows (DISM, SFC, MRT)

**Última revisão:** 2026-10-22 · Código: `crates/core/src/repair.rs` · Shell: `repair_run(tool)`, `repair_cancel`, `repair_status`, `repair_history`, evento `maintenance:progress {tool, percent}` · UI: página **Manutenção** (rota `maintenance`, ícone `Wrench`, logo depois de Limpeza) em `src/pages/maintenance.tsx` · Limpeza de arquivos do sistema (mesma vaga de execução): [../cleanup/system-files.md](../cleanup/system-files.md).

## Ferramentas

| `tool` | O que é | Como | Tempo (mostrado) | Limite |
|---|---|---|---|---|
| `dism_scan` | Verificar imagem (= `/ScanHealth`) | `DismCheckImageHealth(ScanImage = TRUE)` | 5 a 15 min | evento de cancelamento |
| `dism_restore` | Reparar imagem (= `/RestoreHealth`, fonte Windows Update) | `DismRestoreImageHealth(sem fontes)` + `DismCheckImageHealth(FALSE)` rápido para o estado final | 10 a 30 min | evento de cancelamento |
| `sfc` | Verificar arquivos do sistema | `%windir%\System32\sfc.exe /scannow` | 10 a 20 min | 90 min |
| `mrt_quick` / `mrt_full` | Verificação de malware (complementar, **não é antivírus**, dito na tela) | `%windir%\System32\MRT.exe /Q` (`/F` na completa) | 3 a 10 min / 30 min a horas | 90 min / 8 h |

Ordem recomendada na tela: verificar imagem → reparar se precisar → verificar arquivos (o SFC copia as peças boas da imagem; imagem danificada primeiro).

## DISM pela `DismApi.dll` (sem `DISM.exe`)

O crate `windows` não traz a DismApi: `LoadLibraryExW("DismApi.dll", LOAD_LIBRARY_SEARCH_SYSTEM32)` + `GetProcAddress` de `DismInitialize`, `DismOpenSession`, `DismCloseSession`, `DismCheckImageHealth`, `DismRestoreImageHealth` com as assinaturas do `dismapi.h`. `DismInitialize(DismLogErrorsWarningsInfo)` **uma vez por processo** (documentado assim; só o sucesso fica em cache, sem `DismShutdown`), log padrão em `%windir%\Logs\DISM\dism.log`. Sessão online (`DISM_{53BFAE52-…}`) aberta e fechada por execução. Progresso pelo callback (`current/total` → %) na thread do DISM; cancelamento por um evento manual criado uma vez e nunca fechado (o `cancel()` sempre pode sinalizar). Resultado: `Healthy` / `Repairable` / `NonRepairable`; erro = `Failed` com o HRESULT (`0x800F081F`/`0x800F0906`/`0x800F0907`/`0x800F0950` = fonte inacessível; `0x800F082F` = reinício pendente; `0x800704C7` = cancelado).

## SFC e MRT pela exceção da §1.3

Exceção registrada em [global-rules.md §1.3](./global-rules.md) (2026-10-22, decisão do dono), seguida à risca em `repair::spawn_wait`:

- **Caminho absoluto montado pelo app** com `GetSystemDirectoryW` (nunca o PATH, nem variável de ambiente). App x64 = sem redirecionamento WOW64 (o `sfc.exe` só existe no System32 de 64 bits).
- **Sem shell, argumentos fixos**: `sfc /scannow`; `MRT /Q` ou `/Q /F`. Nada vindo do chamador vira argumento: o comando recebe um enum (`Tool`), desserializado pelo serde (valor desconhecido = erro).
- **Só por clique** com o app **elevado** (`needs_admin` antes de qualquer coisa; nenhum auto-relançamento), confirmação na tela com o tempo estimado.
- `CREATE_NO_WINDOW`, stdin/stdout/stderr nulos, **um de cada vez** (`repair::exclusive`, compartilhado com DISM e com a limpeza de arquivos do sistema: `busy`), **prazo + kill** e kill no "Parar".
- **Resultado lido do log do próprio Windows**, por parsers puros testados:
  - **SFC** → `%windir%\Logs\CBS\CBS.log`: só linhas `[SR]` com carimbo ≥ início da execução (`YYYY-MM-DD HH:MM:SS` local, o formato do CBS), lidas a partir do tamanho que o arquivo tinha no início (arquivo menor = rotacionado → lê tudo e filtra pelo horário). Arquivos contados uma vez pelo último `"nome"` da linha; "Cannot repair member file"/"Could not reproject corrupted file" = sem reparo; "Repairing corrupted file"/"Repaired file" = reparado (quem também ficou sem reparo não conta como reparado). Sem nenhuma linha `[SR]` = `Unknown` ("o registro do Windows não trouxe detalhes").
  - **MRT** → `%windir%\debug\mrt.log` (UTF-16 LE com BOM): conta os blocos (`Microsoft Windows Malicious Software Removal Tool v…`) antes e depois; só lê o último bloco se um novo apareceu. Guarda **só o nome da ameaça** (`Found Trojan:Win32/X …`), nunca as linhas de caminho (PII). Veredito pelo código de saída (KB891716: 0 = nada, 6 a 13 = infecção encontrada, removida ou não), com o log como reserva. MRT ausente (chega pelo Windows Update) = `mrt_missing`, cartão mostra o motivo.
- **Parar o SFC** encerra o `sfc.exe`; o trabalho de verificação é do serviço TrustedInstaller e o Windows pode terminar sozinho em segundo plano.

## Antivírus (Kaspersky)

Nada aqui repete a cadeia que o System Watcher pontuou nos scripts antigos ([kaspersky-false-positive.md](../security/kaspersky-false-positive.md)): sem `cmd`/PowerShell, sem auto-elevação, sem `net stop`, sem deleção em massa nossa. São ferramentas da Microsoft, assinadas, chamadas direto, por clique do usuário, com o app já elevado pelo UAC normal. DISM nem vira processo nosso (o `DismHost.exe` é do próprio Windows).

## Histórico

`%LOCALAPPDATA%\G2 Boost\repair-history.json`, últimas 50 execuções: ferramenta, data, duração, veredito, código, contagens do SFC, nomes de ameaça. Sem caminhos.

## Testes (`repair.rs`)

- `parse_cbs`: só a execução atual (linha anterior ao início ignorada), arquivo contado uma vez (mesmo nome em caixa diferente), "sem reparo" vence "reparado", execução limpa, log rotacionado/vazio, texto multibyte antes da coluna 19 sem pânico.
- `parse_mrt`: formato real do `mrt.log` desta máquina (2026-10-14) + bloco infectado fabricado; só o último bloco conta; nomes sem caminho; deduplicado.
- `Tool` desconhecido recusado pelo serde; caminhos do System32/Windows vêm da API; sem admin = `needs_admin`; vaga única (segunda tarefa recusada).
- `dism_api_initialises` (`#[ignore]`, **elevado**): carrega e inicializa a DismApi.

## Verificação elevada

Não rodado pelo agente (exige UAC). Com o binário de teste já compilado (`cargo test -p g2boost-core --no-run`), numa janela **elevada**, saída em arquivo (ver hurdle "Testes elevados" do `CLAUDE.md`):

1. `target\debug\deps\g2boost_core-*.exe --ignored volcache::tests::measure_live --nocapture` → cada handler com bytes, nenhum `untrusted_handler` (só `GetSpaceUsed`, nada é apagado).
2. `target\debug\deps\g2boost_core-*.exe --ignored repair::tests::dism_api_initialises` → `DismInitialize` ok.
3. No app elevado (`tauri:dev`), página **Manutenção**: rodar "Verificar imagem" (progresso em %, Parar no meio → `Parado`; de novo até o fim → `Íntegro`), "Verificar arquivos do sistema" (barra indeterminada + tempo; resultado com contagens ou `Íntegro`), "Verificação de malware: Rápida" (`Nada encontrado`); conferir o `repair-history.json`.
4. Limpeza → **Arquivos do sistema** elevado: Prévia (Limpeza de atualizações leva minutos; Parar funciona), limpar só itens de risco baixo, conferir bytes no resultado e no histórico. Essenciais do disco → WinSxS → "Limpar com segurança" abre essa aba.
5. Com o Kaspersky ativo: nenhuma detecção durante SFC/MRT/DISM/limpeza.
