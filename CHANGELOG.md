# Changelog

## 0.10.2 (2026-11-21): caça de bugs: atualizador, segurança com o app como administrador e presets

Uma revisão em quatro frentes (o que apaga arquivos, sistema e rotinas, telas, e o que o app aceita de fora) achou 33 problemas no código anterior. Todos corrigidos.

**Importante para quem já usa:** o botão de atualizar do app nunca conseguia abrir o instalador (o Windows dizia que o arquivo estava em uso). Baixe esta versão manualmente uma última vez; a partir dela o botão funciona.

### Corrigido
- **Presets:** aplicar Padrão, Alto desempenho ou "Padrão do Windows" como administrador podia **instalar o Recall** em PC sem Copilot+ e **desligar o agendamento de GPU** que o Windows liga sozinho em placas novas. Agora o app só desfaz o que ele mesmo mudou.
- **Atualizar pelo app:** funciona, inclusive em cópias instaladas fora de Arquivos de Programas.
- **Limpeza como administrador:** apaga arquivo por arquivo pelo próprio identificador do Windows e confere que ainda está dentro da pasta certa, então trocar uma pasta por um atalho de pasta no meio da limpeza não leva a apagar outra coisa. Caminhos escritos de jeitos diferentes (barras, `.`, `\?\`) não driblam mais a proteção das pastas do sistema. A pasta de temporários só é limpa se for mesmo uma pasta Temp.
- **Backups e desfazer:** com o app como administrador ficam numa pasta que só administradores alteram. A pasta de instalação fora de Arquivos de Programas passa a ter a mesma proteção, e "Iniciar com o Windows" só abre como administrador quando o programa não pode ser trocado por outro.
- **Desinstalar:** desinstaladores de apps instalados só para o seu usuário abrem sem administrador. O winget só é usado se for o do próprio Windows.
- **Análise de disco e Claude:** um caminho de rede (`//servidor/pasta`) não é mais aberto; antes isso podia enviar suas credenciais do Windows para outro computador.
- **Modo Jogo:** aplica prioridade e núcleos ao processo certo do jogo (não ao inicializador), volta a prioridade que o jogo tinha e devolve o plano de energia mesmo se o app fechar no meio.
- **Configurações:** um valor inválido no arquivo não apaga mais tudo (jogos, perfis, regras); o arquivo com problema é guardado ao lado.
- **Rotinas:** a limpeza agendada que caiu num dia em que o PC estava desligado roda na próxima oportunidade.
- **Telas:** duplo clique não roda a mesma tarefa duas vezes; a Luli não manda duas perguntas ao mesmo tempo; Apps não abre sempre na aba de sobras; presets não ficam clicáveis enquanto o ponto de restauração é criado; sair do Diagnóstico de rede para o teste; "Otimizar agora" avisa quando algo falhou.
- **Claude / MCP:** pedidos com mais itens do que a janela de confirmação mostra são recusados; ajustes beta não podem ser ligados pelo Claude; restaurar DNS mostra para quais servidores volta.

## 0.10.1 (2026-11-20): aviso do Windows com o app como administrador, Teamfight Tactics e ícone do Discord

Correções achadas testando a 0.10.0 instalada.

- **Aviso do Windows:** com o G2 Boost aberto como administrador (o padrão de quem liga "Sempre abrir como administrador"), o Windows descartava a notificação sem avisar. Agora ela é mostrada por um processo auxiliar sem administrador. Nesse caso, clicar no aviso não reabre a página.
- **Anti-cheat:** o Teamfight Tactics entra na checagem (usa o mesmo Vanguard do League of Legends).
- **Ícones:** Discord e outros apps que iniciam pelo próprio atualizador mostram o ícone real na Inicialização, na Limpeza e na lista de quem usa a RAM, e o atalho "tirar da inicialização" passa a reconhecê-los.

## 0.10.0 (2026-11-20): central de atividade, limpeza inteligente, prontidão para anti-cheat e Conexões da Luli

### Novo
- **Atividade:** sino no topo com as notificações do app (rotina de limpeza, revisão semanal, atualizações em lote, jogo detectado, monitor de segurança, mudanças depois de reiniciar) e página **Atividade** com Notificações e Execuções (tudo o que o G2 Boost rodou neste PC, por módulo). Uma tarefa longa continua visível numa pílula de progresso quando você troca de página, e um ponto pulsa no item do menu. Com a janela escondida, o aviso aparece como notificação do Windows (desliga em Configurações).
- **Limpeza inteligente:** um botão Analisar mede tudo (categorias, arquivos do sistema, Lixeira, arquivos grandes e antigos, pastas sem dono, sobras de apps, duplicados) e separa em **Seguro**, **Revisar** e **Sistema**. Durante a limpeza a barra do disco do sistema ganha espaço livre ao vivo; no fim aparecem o antes e depois e o próximo passo. As abas antigas viraram o **Modo detalhado**. Categorias novas: instaladores de driver deixados na raiz do disco e cache de mídia de editores de vídeo; relatórios de travamento do kernel entram em Despejos.
- **Disco e limpeza juntos:** o mapa marca pastas que a Limpeza já cuida (com "Limpar só isso") e pastas sem dono; os Maiores e os Essenciais mostram o mesmo selo.
- **Pronto para jogar com anti-cheat:** em Jogos e no Meu PC, cada jogo que exige firmware (Valorant e LoL, CS2 com FACEIT, Battlefield 6, Call of Duty) mostra o que falta: TPM 2.0, Secure Boot, modo UEFI e disco GPT, VBS, integridade de memória, IOMMU, versão do Windows e lista de bloqueio de drivers, com o caminho na BIOS da sua placa e o botão Reiniciar na BIOS. Depois de reiniciar, o app confere sozinho e avisa no sino quando algo mudou. A BIOS não pode ser alterada pelo Windows: o app guia e confere.
- **Conexões:** página nova para a Luli. Botão **Testar** para cada provedor de IA (lista os modelos sem gastar créditos e diz se a chave funciona, está sem cota ou é inválida), seletor de modelo, **Skills** no formato SKILL.md (colar, escolher .md ou .zip, arrastar; entram desligadas, só o texto é lido, nada roda) e o mapa só leitura dos servidores MCP que os apps de IA deste PC iniciam, com selo para quem baixa pacote da internet a cada início. Conectar o G2 Boost também ao Cursor e ao VS Code.
- **Memória RAM:** composição ao vivo (em uso, modificada, em espera, livre), antes e depois da limpeza, conselho de quando limpar ajuda, e quem usa a RAM com o ícone real de cada app, com atalho para encerrar ou tirar da inicialização.
- **Ícones reais** na Inicialização, nas sobras de apps (agrupadas por app, com selecionar tudo e desfazer), nas pastas sem dono e nos órfãos. Sugestão de apps pouco usados e grandes em Apps › Instalados.
- **Rede › Diagnóstico:** "minha internet está lenta" em 7 passos, com correções em 3 grupos, backup e antes/depois; economia de energia da placa de rede.
- **Claude / MCP:** ferramentas novas de leitura: atividade recente, plano de limpeza, prontidão para anti-cheat, servidores MCP do PC e memória.

### Mudou
- **Ajustes revisados:** "apps em segundo plano" desligados entra no Debloat › Recomendado; latência de rede e prioridade de jogos agora dizem honestamente quando ajudam e saem da pontuação. Desligar a integridade de memória avisa quando um jogo instalado precisa dela.
- **Limpeza de temporários** deixa de fora arquivos com menos de 24 horas (programas abertos usam esses arquivos).
- O Painel não mede o PC com a janela escondida ou minimizada.
- O modelo padrão da NVIDIA passa a ser `moonshotai/kimi-k3.1-pro` (o anterior saiu da lista pública).

### Corrigido
- Parar a limpeza não cancela mais um reparo da Manutenção rodando ao mesmo tempo.
- A limpeza de componentes do Windows só aparece como concluída quando a tarefa rodou de verdade (antes, na bateria, podia dizer "feito" sem ter rodado).
- Desfazer a economia de energia da placa de rede volta os valores de todas as placas alteradas, não só da primeira.
- Ícones nunca são lidos através de pastas vinculadas que levem a um compartilhamento de rede.
- Reinstalar o G2 Boost em outra pasta repara sozinho a conexão com o Claude.

## 0.9.3 (2026-10-28): Conectar ao Claude volta a funcionar e a Luli explica o limite de uso

- **Conectar ao Claude:** o Claude Desktop da Microsoft Store passou a guardar a configuração em `%LOCALAPPDATA%\Claude-Data`. O G2 Boost continuava gravando na pasta antiga, mostrava "Conectado" e o Claude não carregava o G2 Boost. Agora ele grava na pasta nova e ignora a antiga quando o Claude saiu dela. Quem já tinha conectado: clique em Conectar de novo e reabra o Claude Desktop pela bandeja.
- **Luli com limite de uso:** quando o provedor recusa por cota (a camada grátis do Gemini tem limite por minuto e por dia), a mensagem agora diz que é limite de uso e mostra o detalhe do provedor. Antes aparecia "sobrecarregado, espere alguns minutos" mesmo com a cota do dia esgotada.
- **Menos pedidos gastos:** com limite de uso o app não repete o mesmo modelo na hora; passa direto para o próximo modelo grátis do Gemini, que tem cota própria.

## 0.9.2 (2026-10-26): FPS do overlay, exe com o nome do app e mais testes reais

- **Overlay de jogo: o FPS agora aparece.** Nas versões 0.9.0 e 0.9.1 o contador nunca mostrava quadros: os horários dos eventos do Windows chegavam num relógio diferente do usado na conta. Conferido como administrador: uma animação no Edge marcou 120 FPS num monitor de 120 Hz, com 1% low de 117.
- **O exe instalado se chama "G2 Boost.exe"** (antes `g2-boost.exe`). É o nome que a janela de permissão do Windows mostra para apps sem assinatura.
- **Manutenção:** novo card para apagar os backups que o "Redefinir o Windows Update" deixa (pastas `.bak`), com tamanho e confirmação; aparece só quando há backups.
- **Atualizações pelo próprio app:** Configurações › Atualizações do G2 Boost consulta o GitHub, mostra o que mudou e, na versão instalada, baixa o instalador, confere o SHA-256 publicado e o abre com um clique. Opcional: aviso ao abrir quando há versão nova.
- **Claude / MCP:** três ferramentas novas de leitura: check-up, sensores de hardware e antivírus (60 no total).
- **Testado de verdade, como administrador:** instalador e MSI (instalar, abrir, desinstalar sem sobras), "Preparar e jogar" abrindo o programa sem administrador, reiniciar o áudio, esvaziar a fila de impressão, redefinir o Windows Update e apagar os backups, tela de permissão antes do UAC.
- Atualizando de 0.9.0 ou 0.9.1 com "Iniciar com o Windows" ligado: desligue e ligue de novo em Configurações (o nome do exe mudou).

## 0.9.1 (2026-10-25): correções achadas nos testes como administrador

- **Ajustes protegidos pelo Windows:** quando o Windows (proteção de configurações do usuário) ou o antivírus recusa uma chave mesmo com administrador, como a política dos Widgets no Windows 11 24H2+, o app avisa "bloqueado", desfaz o que tiver gravado pela metade e não deixa nada alterado. Antes aparecia o erro cru `access_denied`.
- **Núcleos sempre acordados e Energia:** em processadores em que o Windows gerencia o estacionamento de núcleos sozinho (a configuração aceita só um valor), o ajuste aparece como indisponível neste PC em vez de falhar ao aplicar.
- **Testes como administrador na máquina de referência:** os 33 ajustes novos fizeram ida e volta sem deixar resto; revisão semanal completa (ponto de restauração, disco, SFC, DISM) rodou de ponta a ponta.
- Visual: classe canônica no assistente pós-formatação.

## 0.9.0 (2026-10-25): primeira versão pública

Primeira release com instalador, MSI e versão portátil. Reúne as rodadas 1 a 9 do desenvolvimento.

### Novo nesta versão (rodada 9)
- **Navegação** agrupada (Principal, Otimizar, Ferramentas, Sistema) com rótulos, recolhível.
- **Painel**: pontuação em velocímetro, pendências, **Boost agora** com confirmação (ajustes recomendados, limpeza e RAM) e card de pós-formatação.
- **Pós-formatação**: apps em lote, debloat, ajustes recomendados e extras numa tela só.
- **Debloat**: telemetria, anúncios, Widgets, Copilot, Recall, Click to Do e outros, em níveis Essencial, Recomendado e Avançado; "Remover a IA do Windows"; apps pré-instalados.
- **PC Boost**: 33 ajustes novos (62 no total), busca, filtros, "Aplicar recomendados" e **Ver o que muda**.
- **Perfis e restauração**: perfis com nome, Sem IA, Padrão do Windows, exportar/importar, desfazer um a um ou tudo, histórico.
- **Mouse e teclado**: velocidade do ponteiro, aceleração, repetição do teclado, atalhos que atrapalham no jogo.
- **Jogos**: capas (cache da Steam e CDN da Steam), **Preparar e jogar** (o jogo abre como usuário normal), adicionar e trocar o exe pelo seletor do Windows, prioridade alta por jogo.
- **Overlay de jogo**: FPS, tempo de quadro e 1% low pelo rastreamento de eventos do Windows, CPU/GPU/temperaturas, atalho F7, sem injeção.
- **Monitor de hardware** (árvore de sensores, mín./máx., CSV) e **Check-up** no Meu PC.
- **Apps › Instalar**: catálogo curado de 166 apps pelo winget, instalação em lote.
- **Manutenção+**: revisão semanal num clique, reparo completo, redefinir o Windows Update, destravar a impressora, reiniciar o áudio.
- **Segurança › Antivírus**: antivírus detectado; liberar o G2 Boost no Defender (versão instalada) ou passo a passo por fabricante.
- **Ferramentas**: teste do mouse (polling e cliques), teste de monitor, perda de pacotes.
- **Limpeza**: anel do disco do sistema e barras por categoria.
- **Permissão**: com "Sempre abrir como administrador", o app explica o pedido antes da janela do Windows; recusar abre em modo limitado.
- **Visual**: marcador que desliza na navegação e nas abas, títulos com entrada, toasts com desfazer, capas que inclinam com o ponteiro, ícones com traço mais fino.
- **Distribuição**: instalador por máquina (Arquivos de Programas) com WebView2 embutido, MSI pt-BR e en-US, zip portátil (dados ao lado do exe), SHA256SUMS.

### Mudanças de comportamento
- Presets do PC Boost só mexem nos próprios ajustes (antes desligavam tudo que não estivesse no preset, inclusive o debloat).
- Desfazer a partir do backup só regrava as entradas que o próprio ajuste captura.
- O instalador passou a ser por máquina: uma instalação antiga por usuário (em `%LOCALAPPDATA%`) pode ser removida em Configurações › Aplicativos.

### Ainda sem assinatura digital
O SmartScreen avisa na primeira execução e o UAC mostra "Fornecedor desconhecido". Veja o README.
