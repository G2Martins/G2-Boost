# Onde cada coisa fica no G2 Boost

**Última revisão:** 2026-11-20

Quando o app resolve, aponte a tela exata (nome como aparece no menu, em pt-BR ou en conforme o usuário). Quando não resolve, diga isso e explique o caminho no Windows.

- **Painel › Visão geral:** pontuação de otimização, monitor ao vivo (CPU, GPU, RAM, rede), pontos de restauração (criar um antes de várias mudanças), ações recomendadas e os achados da Luli para este PC.
- **Painel › Meu PC:** hardware completo (CPU, pentes de RAM e velocidade nominal, GPU, discos SSD/HD, monitores), BIOS/firmware visto do Windows (UEFI, Secure Boot, TPM, virtualização, IOMMU, disco GPT, Resizable BAR, XMP/EXPO, idade da BIOS), requisitos de anti-cheat, WinSAT e relatório para exportar.
- **Jogos:** biblioteca de todos os launchers, Preparar e jogar, Modo Jogo e Pronto para jogar com anti-cheat (o que cada anti-cheat exige da BIOS e do Windows, passo a passo pela placa, Reiniciar na BIOS).
- **Analisador de disco:** varredura com mapa (treemap), aba Arquivos e filtros (`*.mp4 >1gb >6m nome`), Essenciais grandes (pastas do sistema explicadas, como WinSxS e Installer), mostrar na pasta e enviar para a Lixeira.
- **Limpeza:** catálogo com prévia em bytes antes de apagar. Diária (temporários, caches leves), Mensal (logs, dumps, caches de shader, Windows Update) e Avançado. Histórico de execuções.
- **Desinstalar apps:** usa o desinstalador do próprio app e depois mostra as sobras (pastas, registro, tarefas) com motivo e Lixeira. Inclui apps da Store, desfazer a última remoção e a aba Sobras de apps removidos.
- **Analisador de rede:** monitoramento em tempo real (Google, Cloudflare e o roteador), Scanner (velocidade e bufferbloat), Ping para jogos (relays da Steam por região), Dispositivos (quem está na rede: tipo, fabricante, selo Novo, verificação de portas de risco) e Ferramentas (MTR, Ping, TCPing).
- **Console:** comandos nativos do G2 Boost, só leitura (`pc`, `erros`, `disco`, `pasta`, `ping`, `rota`, `dns`, `rede`, `dispositivos`, `processos`, `apps`, `energia`, `sensores`, `inicializacao`). Quando o usuário pergunta pelo Console, sugira linhas exatas desses comandos.
- **PC Boost:** Ajustes de sistema (cada ajuste salva o valor original e reverte exato; presets Padrão, Alto desempenho, Personalizado), Limpeza de RAM (standby list, precisa de administrador), DNS Optimizer (mede e aplica DNS por adaptador com backup), Inicialização (o mesmo que o Gerenciador de Tarefas), Energia (planos e ajustes do plano) e Processos.
- **Luli:** esta conversa. Os botões que a Luli sugere passam pelo fluxo normal do app (backup, confirmação, administrador quando precisa).
- **Atividade (sino no topo):** notificações do app (limpeza agendada, revisão semanal, atualizações em lote, jogo detectado, monitor de segurança) e Execuções, o histórico de tudo o que o G2 Boost rodou neste PC, por módulo. Use para ver o que mudou antes de diagnosticar uma piora.
- **Conexões:** chave de cada provedor de IA (Cofre do Windows), botão Testar (lista os modelos sem gastar créditos e mostra se a chave funciona, está sem cota ou é inválida) e o modelo; Skills (SKILL.md colado, escolhido ou arrastado; entra desligada, só o texto é lido, nada roda); MCP: conectar o G2 Boost ao Claude Desktop, Claude Code, Cursor e VS Code, e a lista só leitura dos servidores MCP que esses apps iniciam neste PC (selo para quem baixa pacote da internet a cada início ou aponta para programa que não existe). Para remover um servidor de terceiro, o caminho é o app dono dele.
- **Configurações:** rotinas (limpeza agendada, limpar RAM ao abrir jogo, iniciar com o Windows na bandeja), ponto de restauração antes de preset, permissão de pesquisa na web da Luli.

Itens que pedem administrador mostram "Elevar (administrador)": o app reabre elevado na mesma tela.

Pelo Claude Desktop conectado ao G2 Boost, as mesmas ações existem como ferramentas: cada mudança abre uma janela de confirmação do Windows no PC e só acontece com o "Sim" da pessoa. Ações de administrador precisam do app elevado.
