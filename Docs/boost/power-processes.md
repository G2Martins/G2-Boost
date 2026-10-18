# Energia e Gerenciador de tarefas (PC Boost)

**Última revisão:** 2026-10-18 · Código: `crates/core/src/power.rs`, `procs.rs` (+ helpers de esquema em `boost.rs`, `startup::is_security_software`); shell `src-tauri/src/lib.rs` (`power_*`, `procs_list/end/reveal`, estado `ProcSampler`); UI `src/components/boost/power-tab.tsx`, `procs-tab.tsx` (abas **Energia** e **Processos** do PC Boost).

## Energia (`power.rs`)
O mesmo que Configurações > Sistema > Energia / Opções de Energia, por `powrprof` (nunca `powercfg`).

- **Planos:** listar (`PowerEnumerate`), ativo (`PowerGetActiveScheme`), **ativar**, **criar "Desempenho máximo"** (cópia visível do modelo oculto Ultimate, criada uma vez — `boost::ensure_ultimate_scheme`), **duplicar** (com nome), **renomear**, **excluir**. Excluir recusa os planos do Windows (Equilibrado, Alto desempenho, Economia) e o plano ativo (`protected_plan`). Nome: 1–64 caracteres visíveis.
- **Ajustes (allowlist `SETTINGS`)**, AC (na tomada) e DC (na bateria — a coluna só aparece com bateria, `GetSystemPowerStatus`):

| id | Subgrupo / setting | Tipo |
|---|---|---|
| `display_off` | vídeo / `VIDEOIDLE` | segundos (0 = nunca) |
| `sleep_after` | suspensão / `STANDBYIDLE` | segundos |
| `hibernate_after` | suspensão / `HIBERNATEIDLE` | segundos |
| `wake_timers` | suspensão / `RTCWAKE` | 0 desligado · 1 ligado · 2 só importantes |
| `disk_off` | disco / `DISKIDLE` | segundos |
| `usb_suspend` | USB / seletiva | 0 · 1 |
| `pcie_aspm` | PCIe / ASPM | 0 desligado · 1 moderada · 2 máxima |
| `cpu_min` / `cpu_max` | processador / `PROCTHROTTLEMIN/MAX` | % |
| `cpu_boost` | processador / `PERFBOOSTMODE` | 0 · 1 · 2 (agressivo) |
| `core_parking_min` | processador / `CPMINCORES` | % (100 = sem estacionamento) |

  Setting que o sistema não expõe (sem hibernação, sem bateria…) volta sem valor e some da tela. Escrever valida id + faixa no core, grava AC/DC e, se o plano é o ativo, reaplica (`PowerSetActiveScheme`) — senão fica salvo para quando for ativado. **Wake timers** entrou pela análise do ExitLag ([binary-analysis.md §2.12](../references/binary-analysis.md)).
- **Privilégio:** verificado em 2026-10-18 — usuário padrão duplica, escreve, renomeia e exclui planos sem UAC (teste `power_round_trip`). Acesso negado vira `needs_admin`.

## Gerenciador de tarefas (`procs.rs`)
- **Lista ao vivo** (`Sampler`): um snapshot por chamada (sem `sleep`); CPU % = delta desde a chamada anterior (a UI chama a cada 2 s; aba desmontada = sem polling). Por processo: nome, caminho, RAM (working set), CPU %, criação, sessão.
- **Identidade = pid + horário de criação** (FILETIME). Encerrar e "abrir local" recebem os dois e reabrem o processo conferindo a criação: pid reciclado → `process_gone`, nunca o processo errado. O `created` viaja como **string** (1,3e17 não cabe exato num número JS).
- **Encerrar** (`end`, `TerminateProcess` código 1) só depois do `<Modal>` de confirmação. Recusado (`protected_process`): pid 0/4, o próprio G2 Boost, tudo sob `%SystemRoot%`, processo crítico (`IsProcessCritical`), sessão 0 (serviço), software de segurança (mesmos marcadores da Inicialização). Elevado/de outro usuário sem admin → `needs_admin`.
- **Abrir local:** caminho resolvido no core a partir de pid + criação (nunca um caminho vindo da UI) → `disk::reveal` (`SHOpenFolderAndSelectItems`).

## Testes
`power`: validação (faixas, nome, ids únicos), leitura ao vivo + recusas (plano desconhecido, excluir ativo/Windows, setting fora da allowlist, valor fora da faixa); `power_round_trip` (ignorado, à mão) duplica → escreve → renomeia → exclui. `procs`: amostrador, recusa de si mesmo/pid 4/criação errada, e **encerramento real**: o binário de teste se relança rodando só `sleep_helper` e o teste encerra esse filho pela identidade (código de saída 1, segunda tentativa = `process_gone`).
