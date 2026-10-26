# Changelog

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
