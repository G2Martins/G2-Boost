# Saúde do Windows: erros e problemas sem botão no app

**Última revisão:** 2026-10-18

Muitos problemas não têm ação dentro do G2 Boost. Mesmo assim, ajude: explique a causa provável e o caminho pela interface do Windows. Nunca entregue comandos.

## Onde olhar

- **Monitor de Confiabilidade:** Painel de Controle › Sistema e Segurança › Segurança e Manutenção › Manutenção › Exibir histórico de confiabilidade. Linha do tempo de travamentos de apps, falhas do Windows e atualizações. Ótimo para "começou depois de quê?".
- **Visualizador de Eventos › Logs do Windows › Sistema:** erros com origem e ID.
- **Saúde do SSD:** Configurações › Sistema › Armazenamento › Configurações de armazenamento avançadas › Discos e volumes › Propriedades. Mostra vida útil estimada, reserva disponível e temperatura em NVMe.
- **Reparar o Windows sem formatar:** Configurações › Sistema › Recuperação › "Corrigir problemas usando o Windows Update" reinstala a versão atual mantendo apps e arquivos (Windows 11).
- **Teste de memória:** app Diagnóstico de Memória do Windows (menu Iniciar), reinicia e testa a RAM.

## Erros comuns e causa provável

- **WHEA-Logger (eventos 17, 18, 19) ou tela azul WHEA_UNCORRECTABLE_ERROR:** erro de hardware. Na prática quase sempre overclock, undervolt ou XMP instável; depois disso, temperatura e fonte. Primeiro passo: voltar a BIOS para o padrão e testar.
- **Kernel-Power 41:** desligou sem desligar direito. Se acontece em jogo: fonte, temperatura ou overclock. Se foi queda de energia ou botão, ignore.
- **Display 4101 ou VIDEO_TDR_FAILURE:** driver de vídeo parou de responder. Driver limpo e atualizado pelo site do fabricante, clock de fábrica na GPU, temperatura.
- **MEMORY_MANAGEMENT, PAGE_FAULT_IN_NONPAGED_AREA:** RAM ou XMP instável; rodar o Diagnóstico de Memória, testar sem XMP.
- **IRQL_NOT_LESS_OR_EQUAL, DRIVER_IRQL_NOT_LESS_OR_EQUAL:** driver com defeito. O nome do driver no dump aponta o culpado (rede, áudio, antivírus de terceiros).
- **Eventos de disco (disk 7, 153, 157) ou Ntfs 55:** disco ou cabo SATA com problema. Backup primeiro, depois verificar a saúde do disco.
- **Wi-Fi caindo:** driver do adaptador pelo site do fabricante, economia de energia do adaptador, rede de 5 GHz ou cabo.

## Atualizações e drivers

- Windows Update em dia, incluindo "Atualizações opcionais" de drivers só quando há problema.
- Driver de vídeo: pelo app do fabricante (NVIDIA App, AMD Software Adrenalin, Intel Graphics Software). Driver muito antigo aparece nos achados (`gpu_driver_old`).
- BIOS: só pelo site do fabricante da placa, com energia estável. Atualiza microcódigo e compatibilidade de RAM.
