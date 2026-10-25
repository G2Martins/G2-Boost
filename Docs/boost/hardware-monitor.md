# Monitor de hardware e Check-up do Meu PC

**Última revisão:** 2026-10-25 · Código: `crates/core/src/hwsensors.rs` (árvore + coleta), `hwcsv.rs` (CSV), `checkup.rs` (medições); acréscimos em `sensors.rs` (`read_all`), `winevents.rs` (`unexpected_shutdowns`, `app_crashes`) e `monitor.rs` (helpers de PDH com visibilidade `pub(crate)`); shell `src-tauri/src/hw_cmds.rs` (`hw_start`, `hw_stop`, `hw_export_csv`, `checkup_run`); UI `src/pages/hwmonitor.tsx` (rota `hwmonitor`), `src/components/checkup.tsx` (aba Check-up do `mypc.tsx`), `src/lib/hw.ts`, `src/lib/checkup.ts`; testes `hwsensors::tests`, `hwcsv::tests`, `checkup::tests`, `winevents::tests`, `tests/hw.test.ts`, `tests/checkup.test.ts`, `e2e/hwmonitor.spec.ts` (+ `e2e/tauri-bridge.ts`, a ponte Tauri falsa). Complementa [dashboard.md](./dashboard.md) (Painel, Meu PC, monitor do Painel) e [../references/speccy-cpuz.md](../references/speccy-cpuz.md) (por que não há driver nosso).

## Monitor de hardware (rota `hwmonitor`)

Uma árvore **dispositivo → grupo → sensor** com valor, mínimo e máximo, atualizada **uma vez por segundo** enquanto a página está aberta. Tudo em modo usuário, sem admin e sem driver nosso.

| Dispositivo | Grupos e sensores | Fonte |
|---|---|---|
| Processador | **Carga** (total + cada thread), **Clocks** (total + cada thread, clock efetivo = frequência nominal × % de desempenho), **Potência** (pacote, núcleos, gráficos integrados, DRAM quando existem) | PDH `Processor Information(*)` (`% Processor Utility`, `% Processor Performance`, `Processor Frequency`) e `Energy Meter(RAPL_Package0_*)\Power` (mW → W) |
| Placa de vídeo (NVIDIA) | **Temperaturas**, **Carga** (GPU, controlador de memória), **Clocks** (gráficos, memória, vídeo), **Potência** (consumo, limite, % do limite), **Memória de vídeo** (em uso, total, %), **Ventoinha** (%) | NVML (`nvml.dll`, já carregada por `nvml.rs`; as funções extras vêm do loader próprio do `hwsensors.rs`) |
| Placa de vídeo (AMD / Intel) | Carga e memória de vídeo em uso | PDH `GPU Engine` e `GPU Adapter Memory` (só entram na consulta quando não há NVML; custam caro por processo) |
| Memória | **Física** (uso %, em uso, disponível, cache do sistema), **Confirmada** (confirmada, limite, %) | `GetPerformanceInfo` |
| Cada disco físico | **Atividade** (tempo ativo, leitura, escrita), **Temperatura** | PDH `PhysicalDisk(*)` (nome do modelo de `hardware::disks`); temperatura do NVMe/SATA pelo `smart::health`, a cada 60 s numa thread própria, só quando o `smart.rs` consegue ler (SATA e alguns NVMe pedem admin) |
| Cada adaptador de rede | **Tráfego** (download, upload), **Link** (velocidade) | `GetIfTable2`, só interface de hardware conectada (bit `HardwareInterface`; sem loopback, filtros nem túneis); taxa = delta dos octetos |
| Dispositivos do HWiNFO / LibreHardwareMonitor | Temperaturas, tensões, ventoinhas, potência, clocks, uso, corrente e outros, **agrupados pelo nome de dispositivo que a própria ferramenta usa** | `sensors::read_all` (memória compartilhada `Global\HWiNFO_SENS_SM2` a cada segundo; WMI do LibreHardwareMonitor a cada 3 s) |

- **Temperatura da CPU só vem das ferramentas externas** (decisão do dono: sem PawnIO e sem driver nosso). Sem nenhuma aberta, o dispositivo Processador mostra a dica que o app já usa no Painel (`dash.monitor.cpu_temp.na`). Com a ferramenta aberta, o selo "Sensores externos: HWiNFO" aparece no topo.
- **Ids estáveis.** `cpu/load/thread3`, `gpu0/temp/gpu_core`, `disk1/activity/read`, `net16/traffic/down`; externos `ext/<dispositivo>/<grupo>/<rótulo>` com `#2`, `#3` quando o rótulo se repete (`slug`: só `[a-z0-9-]`, nunca barra nem `..`). A UI guarda mínimo e máximo por id.
- **Rótulos.** Os nós nativos levam uma chave (`hw.d.*`, `hw.g.*`, `hw.s.*`, com `{n}` nos numerados) e a UI traduz; nomes que vêm da máquina ou da ferramenta (modelo da CPU, do disco, alias da rede, rótulo do HWiNFO) ficam como dado. Um teste confere que toda chave que o core pode emitir existe em pt-BR e en.
- **Limites.** 700 leituras externas (uma placa cheia do HWiNFO publica mais de mil); valores não finitos nunca chegam ao JSON; unidade da ferramenta corrigida pelo tipo (o ANSI do HWiNFO pode trazer `°C` como U+FFFD).
- **Ciclo de vida.** `hw_start`/`hw_stop` com contagem de referências (como `monitor_start`), estado em estáticos no `hw_cmds.rs` (o `lib.rs` só ganhou `mod hw_cmds;` e a lista de comandos). Uma thread, cadência fixa de 1 s (dorme até o próximo prazo), evento `hw:tick`. Se um `hw_start` chega enquanto a thread está parando, ela não sai deixando a página sem coleta. A página pausa a coleta quando a janela fica oculta (bandeja) e retoma ao voltar.
- **Custo.** Primeira amostra 1 s depois de abrir (as taxas do PDH precisam de duas leituras); cerca de 60 sensores nesta máquina (12 threads, GPU, 3 discos, rede). O HWiNFO/LHM é relido só quando existe; uma ferramenta lenta (> 800 ms) é deixada em paz por 30 segundos.

### Página

Tabela recolhível (`<table>` semântica, `aria-expanded` nos nós) com **Sensor · Valor · Mínimo · Máximo**, cabeçalho fixo, números em `.num` (fonte mono, largura estável: casas fixas por unidade). Barra de ferramentas: contador "N sensores · atualiza a cada 1 s" com ponto vivo, **Recolher tudo** (dobra os dispositivos) / **Expandir tudo** (abre também os grupos), **Zerar mín./máx.** (recomeça do valor da tela) e **Gravar em CSV**. O estado recolhido é lembrado no `localStorage` (só conveniência). Realce de mudança de valor: um tom do accent a 16% que some em 700 ms (`Element.animate`, nunca com movimento reduzido); percentuais ganham um preenchimento fino e os outros um marcador na faixa já vista (transição de posição só com `motion-safe`). Fora do app (navegador) a página pede o app em vez de inventar sensores.

### CSV (`hwcsv.rs`)

`Documentos\G2 Boost - sensores - AAAA-MM-DD HH-MM-SS.csv` (pasta conhecida `FOLDERID_Documents`, vale o OneDrive redirecionado), UTF-8 **com BOM**, `\r\n`. Português: `;` e vírgula decimal (o que o Excel em português abre com duplo clique); outros idiomas: `,` e ponto. Colunas: dispositivo, grupo, sensor, valor, mínimo, máximo, unidade (títulos no idioma da UI). Depois de gravar, o Explorer abre com o arquivo selecionado (`reveal_item_in_dir`).

O pedido da UI é hostil: no máximo 5000 linhas e 7 colunas de título; células sem caracteres de controle, cortadas em 200; **fórmulas neutralizadas** (célula de texto começando por `=`, `+`, `-` ou `@` ganha um apóstrofo, e um trilho chamado `-12V` paga esse preço); aspas dobradas e célula entre aspas quando tem o separador. O nome e a pasta são do core; o arquivo é criado com `create_new` (se existir, `(2)`, `(3)`…: nunca sobrescreve).

## Check-up (Meu PC › Check-up)

Dezenove medições, cada uma com valor, limite e veredito **ok / atenção / sem leitura**, e a tela do app que resolve. **Só mede, nunca corrige.** Uma chamada (`checkup_run`) mede tudo em paralelo (thread por medição lenta) e leva cerca de 1 s a 3 s; na aba, esqueletos até chegar. Resumo no topo: "N de M ok" (M = medidas com veredito; as sem leitura aparecem à parte, para um GPU antigo sem Resizable BAR não impedir o "tudo ok"), faixa de 19 marcas e botão **Medir de novo**.

| Linha | Como mede | Limite (atenção quando…) | Resolve em |
|---|---|---|---|
| `disk_free` | `GetDiskFreeSpaceEx` do `%SystemDrive%` | achado `system_disk_full` do conselheiro (< 15% livre ou < 20 GB) | Limpeza |
| `ram_use` | `GlobalMemoryStatusEx` | > 85% em uso | PC Boost |
| `disk_health` | `smart::health` (NVMe sem admin; SATA precisa) | algum disco `Caution`/`Bad`; todos ilegíveis = "sem leitura" + botão Elevar | Meu PC › Visão geral |
| `antivirus`, `firewall` | `exposure::quick` (Central de Segurança do Windows, política do firewall) | sem antivírus ligado e atualizado; firewall desligado na rede em uso (severidade ≥ média, como no conselheiro) | Segurança |
| `uac` | `EnableLUA` e `ConsentPromptBehaviorAdmin` | desligado, ou administrador sem pedido de confirmação | (Windows) |
| `secure_boot`, `tpm` | perfil de hardware | desligado; TPM diferente de 2.0 | Meu PC › Visão geral (guia da BIOS) |
| `activation` | WMI `SoftwareLicensingProduct` (só leitura, id do Windows) | nenhuma licença do Windows em `Licensed` | (Windows) |
| `windows_update` | histórico da Windows Update (`updates::wu_status`) sem as definições do Defender (KB2267602, KB4052623) | última instalação com sucesso há > 45 dias | Apps › Atualizações |
| `reboot` | `RebootRequired` (Windows Update), `RebootPending` (CBS) e o `RebootRequired` da WUA; `PendingFileRenameOperations` fica de fora (instaladores a deixam sem pedir reinício) | reinício pendente | Apps › Atualizações |
| `shutdowns` | eventos Kernel-Power 41 / EventLog 6008 em 30 dias (`wevtapi`, só contagem); vale o maior dos dois (descrevem o mesmo desligamento) | qualquer um | Manutenção |
| `crashes` | Application Error 1000 em 7 dias | mais de 5 | Manutenção |
| `startup` | `startup::list`, itens ligados e não protegidos | mais de 10 | PC Boost |
| `internet` | dois pings (1.1.1.1, 8.8.8.8) e um TCP em 1.1.1.1:443, o mais rápido vale (redes que derrubam ICMP) | nenhum respondeu | Analisador de rede |
| `gpu_driver`, `bios_age` | data do driver / da BIOS | achados `gpu_driver_old` (> 180 dias), `bios_old` (> 3 anos) | GPU / Meu PC |
| `xmp`, `rebar` | `firmware::read` | achados `xmp_off`, `rebar_off` (só em GPU de geração que se beneficia; fora disso "não se aplica") | Meu PC › Visão geral |

Os veredictos de hardware vêm dos **achados do conselheiro** (`advisor::analyze` com o que o check-up já leu), então Painel, Luli e Check-up compartilham os mesmos limites. Os demais limites estão em constantes do `checkup.rs`. Texto de cada linha: `checkup.<id>.title|ok|bad` (e `checkup.unknown.<motivo>`), com números dimensionados pela UI (`*_bytes`, `*_days`). Linha sem tela no app (UAC, ativação) diz onde mexer no Windows.

## Não fazemos

Temperatura, tensão e rotação da placa sem HWiNFO/LibreHardwareMonitor (exigem driver); gravação contínua em CSV (é um instantâneo, com mínimo e máximo do que a página viu); histórico entre sessões; alerta por limite. Backlog: tool MCP `get_hardware_sensors` / `get_checkup`, e dobrar o loader NVML próprio dentro do `nvml.rs` depois que a rodada 9 estiver fundida.

## Verificado e não verificado

**Verificado nesta máquina (2026-10-25, sem admin):** snapshot ao vivo com 61 sensores (12 threads com clock efetivo, RAPL pacote e núcleos, RTX 3050 completa pelo NVML, memória, 3 discos com a temperatura do NVMe, Ethernet); check-up ao vivo em 0,8 s (BIOS de 3,4 anos e Resizable BAR desligado = atenção, como já se sabia; um desligamento inesperado em 30 dias); CSV gravado e relido em `Documentos`; 10 specs Playwright com ponte Tauri falsa (árvore, mínimo/máximo, recolher, zerar, CSV, parar a coleta ao sair, movimento reduzido, linhas do check-up e seus links).

**Não verificado:** caminho com HWiNFO ou LibreHardwareMonitor abertos (o leitor é o do `sensors.rs`, testado com bloco sintético; nenhuma das duas está instalada aqui); GPU AMD/Intel pelo PDH; temperatura de disco SATA/USB elevado; o app Tauri inteiro rodando os quatro comandos (compilam e estão registrados; a ponte falsa cobre o lado da UI); contagem de eventos num PC com milhares de falhas (limite de 500).
