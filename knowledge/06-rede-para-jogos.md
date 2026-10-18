# Rede para jogos

**Última revisão:** 2026-10-18

- **Ping** é o tempo de ida e volta. **Jitter** é a variação; acima de ~10 ms já se sente em jogo competitivo. **Perda** acima de 1% causa teleporte e tiros que não registram.
- **Picos no roteador (gateway) também:** o problema é a rede local (Wi-Fi, cabo, roteador), não a internet. Picos só nos destinos externos: provedor ou rota.
- **Bufferbloat:** a latência sobe quando alguém baixa ou envia algo. O Scanner mede com e sem carga. Correção: QoS ou SQM no roteador (o nome varia: "Smart Queue", "QoS adaptativo"), ou limitar a banda dos downloads.
- **Wi-Fi:** 5 GHz ou 6 GHz perto do roteador, 2,4 GHz só longe. Cabo é sempre mais estável. Repetidor em malha (mesh) adiciona latência.
- **MTR:** perda num salto do meio que não continua nos seguintes é o roteador limitando ICMP, não perda real. Só conta perda que vai até o destino.
- **TCPing:** mede servidores que bloqueiam ping comum (muitos servidores de jogo).
- **Ping para jogos:** mostra a latência até os relays da Steam por região. Escolha no jogo a região com menor ping estável, não só o menor número.
- **DNS:** não muda ping nem FPS dentro do jogo. Serve para resolver nomes mais rápido e às vezes para fugir de DNS lento do provedor. O DNS Optimizer mede antes de sugerir.
- **Velocidade:** quase nenhum jogo precisa de mais de alguns Mbps. Download alto não corrige ping; o que importa é latência estável.
- VPNs e "boosters" de rota só ajudam quando a rota do provedor até o servidor é ruim; o G2 Boost não faz isso.
