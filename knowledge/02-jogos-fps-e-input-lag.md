# Jogos: FPS, 1% lows e input lag

**Última revisão:** 2026-10-18

## Diagnosticar antes de receitar

- **GPU em ~95% ou mais durante o jogo:** limitado pela GPU. Alavancas: DLSS, FSR ou XeSS (ou escala de renderização menor), baixar sombras, volumétricos, oclusão e ray tracing. NVIDIA Reflex ou AMD Anti-Lag reduzem latência justamente nesse cenário.
- **FPS baixo com GPU bem abaixo de 90%:** limitado por CPU, RAM ou segundo plano. Conferir processos pesados, XMP/EXPO desligado, RAM em canal único, plano de energia e se o jogo está num HD.
- **Travadas curtas (stutter) nas primeiras partidas ou mapas novos:** compilação de shaders. Some depois que o cache se forma. Por isso limpar cache de shader com frequência piora; a Limpeza deixa shaders fora das rotinas por padrão.
- **1% lows ruins com FPS médio bom:** RAM (canal único, sem XMP), apps em segundo plano (overlays, navegador com vídeo, gravação), jogo em HD, temperatura.

## Correções grátis primeiro

- Monitor rodando abaixo da taxa máxima (comum: 144 Hz ligado em 60 Hz). Ajuste em Configurações › Sistema › Tela › Tela avançada.
- XMP/EXPO desligado na BIOS: a RAM roda na velocidade base (ex.: 4800 em vez de 6000). Ligar é o upgrade grátis mais comum.
- Resizable BAR desligado numa GPU que suporta: ligar na BIOS (exige UEFI, sem CSM).
- Jogo instalado em HD mecânico: mover para SSD pelo gerenciador de armazenamento do launcher (Steam: Configurações › Armazenamento).

## Input lag

- Tela cheia exclusiva, ou janela sem borda com o modelo flip (ajuste Otimizações para jogos em janela no PC Boost).
- VRR (G-Sync/FreeSync) ligado mais limite de FPS 3 abaixo da taxa do monitor: menos latência, calor e ruído.
- Reflex/Anti-Lag no jogo quando existir. Mouse a 1000 Hz, aceleração do mouse desligada (ajuste Mouse no PC Boost).

## Ajustes do Windows e o que esperar

- **Modo de jogo:** ligado por padrão no Windows 11; pode ficar ligado.
- **HAGS (agendamento de GPU acelerado):** ganho pequeno e variável; é exigido pelo DLSS Frame Generation da NVIDIA. Vale após reiniciar.
- **Integridade de memória (HVCI) e plataforma de máquina virtual:** a própria Microsoft lista desligar como opção para jogos, com perda de proteção ("Options to optimize gaming performance in Windows 11", Suporte Microsoft). Tom's Hardware mediu até ~5% em alguns jogos. Só explique quando o usuário perguntar; nunca empurre.
- **Plano Desempenho máximo:** pouco ganho em desktop moderno; aumenta consumo e calor. Em notebook prefira Equilibrado na tomada mais limite de FPS.
- **DNS** não aumenta FPS nem muda o ping dentro do jogo; só encurta a resolução de nomes.

## Números de FPS

Nunca invente. Use benchmark com a mesma GPU e CPU (ou muito próximas), diga resolução e preset, dê faixa e cite a fonte. Sem fonte confiável, diga que não há e raciocine pela classe (tier) da máquina.
