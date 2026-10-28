# Changelog

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
