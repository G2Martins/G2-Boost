# Manutenção — verificar e reparar o Windows (DISM, SFC, MRT, disco, unidades, opções de inicialização)

**Última revisão:** 2026-10-23 (Manutenção+: verificar disco, otimizar unidades, reiniciar nas opções avançadas) · 2026-10-22 · Código: `crates/core/src/repair.rs`, `volumes.rs`, `firmware.rs` (`reboot_to_boot_options`) · Shell: `repair_run(tool)`, `repair_cancel`, `repair_status`, `repair_history`, evento `maintenance:progress {tool, percent}`; `volumes_list`, `volumes_run(op, volume)`, `volumes_status`, `volumes_history`, evento `maintenance:volume-progress {op, volume, percent}`; `boot_options_reboot` · UI: página **Manutenção** (rota `maintenance`, ícone `Wrench`, logo depois de Limpeza) em `src/pages/maintenance.tsx` + `src/components/maintenance/drive-tools.tsx` · MCP: `get_maintenance_status`, `get_volumes` (só leitura) · Limpeza de arquivos do sistema (mesma vaga de execução): [../cleanup/system-files.md](../cleanup/system-files.md).

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

## Verificar disco e otimizar unidades (`volumes.rs`, 2026-10-23)

Pela Storage Management API (WMI `ROOT\Microsoft\Windows\Storage`), o provedor por trás de `Repair-Volume` e `Optimize-Volume`. Nunca `chkdsk.exe`, `defrag.exe` nem PowerShell.

| `op` | Chamada | Quando o core permite | Limite |
|---|---|---|---|
| `scan` | `MSFT_Volume.Repair(Scan = TRUE)` (= `chkdsk /scan`, online, sem reiniciar) | NTFS | 2 h |
| `spot_fix` | `Repair(SpotFix = TRUE)` (corrige só o que o scan registrou; unidade de dados sai do ar por segundos, a do Windows fica para o próximo reinício) | NTFS **e** (último `scan` desta unidade deu `needs_spot_fix` **ou** o Windows marcou `OperationalStatus` 53262/53263) | 1 h |
| `analyze` | `Optimize(Analyze = TRUE)`, depois lê `LastRunPercentFragmentation` | NTFS em HD ou tipo desconhecido | 1 h |
| `retrim` | `Optimize(ReTrim = TRUE)` | SSD (NTFS ou ReFS) | 2 h |
| `defrag` | `Optimize(Defrag = TRUE)` | **só HD com prova** (NTFS) | 12 h |

- **Lista** (`volumes::list`, sem admin): `MSFT_Volume` fixos (`DriveType = 3`) com letra: letra, rótulo, sistema de arquivos, tamanho/livre (`uint64` chega como texto no VARIANT), `HealthStatus`, `OperationalStatus` (53261 = scan pedido, 53262 = spot fix, 53263 = reparo completo). O `ObjectId` (chave) precisa estar no `SELECT`, senão o WMI não devolve `__RELPATH`.
- **SSD × HD**: `IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS` em `\\.\X:` (`hardware::volume_disks`) dá os discos da unidade; o tipo vem do seek penalty de cada `PhysicalDriveN` (`hardware::disks`). SSD só se todos os discos dizem SSD, HD só se todos dizem HD; misto ou disco que não responde = `unknown` → **nunca desfragmenta** (só analisa). Na máquina do dono o HD USB (ponte que não responde ao seek penalty) cai em `unknown`.
- **Fragmentação e última otimização** vêm das estatísticas do próprio Windows em `HKLM\SOFTWARE\Microsoft\Dfrg\Statistics\Volume{GUID}` (`LastRunPercentFragmentation`, `LastRunTime` = SYSTEMTIME; lidas sem admin). Mostradas só para HD/desconhecido. Acima de 10% a UI sugere desfragmentar (o mesmo limite do Windows).
- **Validação**: o `volume` que chega da UI/MCP é o `\\?\Volume{GUID}\` e tem que estar na lista ao vivo; a operação tem que estar em `ops` daquela unidade (`not_allowed`); o caminho WMI usado na chamada é o `__RELPATH` que o próprio WMI devolveu, nunca texto do chamador. `Op` desconhecido é recusado pelo serde (sem `format`, `offline_scan_and_fix` etc.).
- **Vaga única**: usa `repair::exclusive()` (mesma de DISM/SFC/MRT e da limpeza de arquivos do sistema: `busy`) e o mesmo `repair::cancel()` (o botão Parar chama `repair_cancel`). A UI bloqueia os cartões de DISM/SFC/MRT enquanto um disco roda e vice-versa.
- **Assíncrono e cancelável**: `ExecMethodAsync` com `WBEM_FLAG_SEND_STATUS` numa thread MTA própria (o sink é chamado em threads RPC, sem bomba de mensagens); o sink (`IWbemObjectSinkEx`, `#[implement]`) guarda `ReturnValue`/`Output`, o status final e o progresso (por `SetStatus(WBEM_STATUS_PROGRESS)` ou `WriteProgress`, o que o provedor mandar; sem progresso = barra indeterminada). Parar ou passar do limite = `CancelAsyncCall` e mais 30 s de espera pelo fim. **Pergunta do provedor (`PromptUser`) é respondida "não"** e registrada (`prompted`): nada de desmontagem forçada sem o usuário saber.
- **Resultado**: `Output` de `Repair` pelo enum `Volume.RepairStatus` do Storage (0/4 = sem erros, 1/2/5/16 = corrigido, 6/7/15 = precisa spot fix, 13 = verificação offline no reinício, 8/10/12 = rodar de novo, resto = falhou); `ReturnValue` ≠ 0 = falhou com o código (40001 acesso negado, 43001 sistema de arquivos sem suporte); HRESULT negativo = falhou com o HRESULT; 1460 = tempo esgotado.
- **Histórico**: `%LOCALAPPDATA%\G2 Boost\volume-history.json`, últimas 50 (op, id da unidade, letra, data, duração, veredito, código, `Output`, fragmentação, `prompted`). Sem caminhos de arquivo.
- **Testes**: regras puras (`allowed`: defrag nunca em SSD/desconhecido/ReFS/FAT, ReTrim só SSD, spot fix só com achado, removível = nada), `media_of` (todos concordam), `needs_of`, `repair_verdict`, chave Dfrg sem traversal, data SYSTEMTIME real desta máquina, serde recusa op desconhecido, sem admin = `needs_admin`, elevado com id fora da lista = `unknown_volume` (nunca chega ao WMI). `#[ignore]`, só leitura: `live_volumes` (lista com tipo e saúde) e `live_async_method` (o caminho assíncrono inteiro com `GetSupportedFileSystems`, que não muda nada).
- **Ao vivo (2026-10-23, sem admin)**: C: NTFS SSD íntegra (ops `scan`, `retrim`), D: NTFS HD USB = `unknown` (ops `scan`, `analyze`; 0% na última análise do Windows em 2025-07-31), G: NTFS SSD NVMe (ops `scan`, `retrim`). `GetSupportedFileSystems` assíncrono: status final + `ReturnValue 0` chegaram pelo sink.

## Reiniciar nas opções avançadas (`firmware::reboot_to_boot_options`)

O que `shutdown /r /o` faz, sem o `shutdown.exe`: `SeShutdownPrivilege` + `InitiateShutdownW(SHUTDOWN_RESTART | SHUTDOWN_RESTART_BOOTOPTIONS)` (0x404; nunca forçar logoff, desligar ou híbrido), mesmo `restart()` do Reiniciar na BIOS (ERROR_ACCESS_DENIED/1314 = `needs_admin`). Só elevado (`shutdown /o` também exige), confirmação em modal (salvar o trabalho; "Continuar" volta ao Windows). O cartão explica o caminho até o Modo de Segurança (Solução de Problemas › Opções avançadas › Configurações de Inicialização › Reiniciar › tecla 4, ou 5 com rede), as outras opções (Reparo de Inicialização, Restauração do Sistema, Desinstalar Atualizações, Firmware UEFI) e avisa da chave do BitLocker. Teste: só a composição dos flags e a recusa sem admin; **nunca reinicia em teste**.

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
6. **Disco (2026-10-23)**, app elevado, página Manutenção: "Verificar" em C: (barra; resultado `Sem erros` ou contagem; conferir se veio progresso em % ou só indeterminado), "Analisar" em D: (fragmentação no resultado), "Otimizar (TRIM)" em C: ou G: (`Concluído`), "Parar" no meio de uma análise (`Parado`, slot liberado: DISM volta a começar), conferir `volume-history.json` e `prompted: false`. Se um dia aparecer "Corrigir erros", rodar em unidade de dados e anotar se o provedor perguntou algo (`prompted`).
7. **Reiniciar nas opções avançadas**: fica para o dono testar à mão (reinicia o PC; voltar com "Continuar").
