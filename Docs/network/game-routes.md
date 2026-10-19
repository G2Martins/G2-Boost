# Jogos e conexões (estilo ExitLag) — viabilidade e arquitetura

**Última revisão:** 2026-10-18 · Status: **seção 2 implementada** (decisão do dono: "siga"), menos QoS; implementação em [../boost/games.md](../boost/games.md) · Referência: [../references/exitlag.md](../references/exitlag.md), prints do dono de 2026-10-18 (Conexões, diagrama de rota do CS2, Rotas Multipath, Reotimizar, "Analisando rotas 2/3 (Routes 55/103)").

## 1. O que o ExitLag faz e por que depende de servidores

- **Conexões:** lista de jogos com chave liga/desliga, modo Manual/Automático, botão +.
- **Por jogo:** servidor de destino (BR São Paulo01), "Redirecionar login", diagrama `você → Brasília05 → São Paulo25 → destino` e uma rota alternativa (`São Paulo32`), ping estimado, Rotas Multipath (TCP/UDP), Reotimizar (testa ~100 rotas por região).
- **Os nós do meio são servidores deles.** O pacote do jogo sai do PC por um túnel (driver/WFP + rede própria) até um relay e de lá para o servidor do jogo. Multipath = o mesmo pacote enviado por 2 ou mais relays, e chega o primeiro. Sem uma rede de relays não existe "outra rota": a internet decide o caminho entre o seu provedor e o servidor do jogo.

Conclusão: **rota e multipath reais exigem servidores** (custo mensal por região + banda). O resto da experiência dá para fazer local, de verdade, sem prometer o que não entrega.

## 2. O que o G2 Boost pode entregar sem servidores (fase proposta "Jogos")

| Parte | Como (nativo) | Admin | Esforço |
|---|---|---|---|
| **Lista de jogos** (igual "Conexões") | Steam (`steam.rs`), Epic (`ProgramData\Epic\EpicGamesLauncher\Data\Manifests\*.item`), EA/Ubisoft/Battle.net/Riot (chaves `Uninstall` já lidas em `apps.rs`), GOG (registro), Xbox/Store (`PackageManager`), + adicionar exe à mão. Ícone pelo `apps::icon`. | não | M |
| **Chave por jogo = Modo Jogo** | Ao abrir o jogo (detector de `routines.rs`): plano de energia, purge da standby list, prioridade do processo (`SetPriorityClass`), pausar rotinas e notificações (`SHQueryUserNotificationState`); tudo volta ao fechar. | parcial | M |
| **Servidor real do jogo** | TCP: `GetExtendedTcpTable` (pid → IP remoto). UDP (a maioria dos jogos): a tabela UDP não tem IP remoto; precisa de sessão ETW `Microsoft-Windows-Kernel-Network` (admin) para ver para onde o processo envia. Valve: o "servidor" é o relay SDR (`GetSDRConfig`, já usado). | UDP: sim | M |
| **Diagrama de rota real** | MTR (`IcmpSendEcho2Ex` com TTL, já existe) até o servidor detectado ou o endpoint da região; desenhar como o ExitLag (nós com ms entre eles), marcando o salto que piora e se ele está no seu provedor ou depois. | não | M |
| **"Reotimizar" honesto** | Bateria: DNS mais rápido (`dns.rs`), bufferbloat (speedtest), Wi-Fi × cabo e sinal (`WlanQueryInterface`), ping por região do jogo (SDR POPs, endpoints públicos por publisher num JSON atualizável) → recomenda a região e aplica os ajustes de rede (`network_latency`), com prévia e desfazer. | DNS: sim | M |
| **Prioridade do jogo na rede** | Política QoS por exe (`MSFT_NetQosPolicy` via WMI, DSCP 46 + limite para os outros apps) | sim | M |
| **Ping estimado / ao vivo** | Probe ICMP/TCP ao servidor detectado a cada 1 s enquanto o jogo roda; overlay depois (janela Tauri transparente, sem injeção). | não | S–L |

Nada disso muda o caminho dos pacotes; muda o que está no nosso controle (PC, Wi-Fi, DNS, fila do roteador, escolha de região) e mostra onde está o problema quando ele é do provedor.

## 3. Se um dia houver servidores ("G2 Relay", decisão de produto)

- **Túnel:** WireGuard (o que Kaspersky VPN e outros usam; driver `wireguard-nt` assinado pela WireGuard LLC, redistribuível [?] licença a confirmar), relays em VPS por região (São Paulo, Miami, Santiago...).
- **Só o jogo pelo túnel sem driver nosso:** rotas por destino (IPs do servidor do jogo detectado → interface do túnel), não por app. Por app (split tunneling de verdade) exigiria driver/WFP callout.
- **Multipath:** enviar o mesmo datagrama UDP por dois relays e descartar duplicata no relay de saída: lógica no relay e no cliente (user-mode com wintun), sem driver próprio.
- **Custo:** VPS + banda por usuário ativo, autenticação, abuso (vira VPN aberta), suporte. Só faz sentido como produto pago.

## 4. Decisões pendentes do dono

1. Fazer a fase "Jogos" local (seção 2) agora? Ordem sugerida: lista de jogos → Modo Jogo → diagrama MTR → Reotimizar honesto → QoS.
2. Detectar o servidor UDP exige admin (ETW): aceitar "Elevar" nessa tela?
3. G2 Relay (seção 3): fora até haver plano de negócio.
