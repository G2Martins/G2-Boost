# ExitLag 5.23 — spec por tela (prints de 2026-10-14) e escopo

**Última revisão:** 2026-10-14 · Fontes: 13 prints do cliente (pt-BR) + `exitlag.com/blog/exitlag-guide/` (marketing: multipath routing por rede própria de servidores — **não replicável** sem infra).

## Analisador de rede
| Aba | O que mostra | Como replicar |
|---|---|---|
| **Monitoramento em tempo real** | "Monitore seu serviço de internet" (ligar/desligar, IP público, "Visualizar logs"); alvos `Internet` / `Gateway` / `Amazon AWS`; janela 1 min…; card **Saúde da conexão** (pill `Estável`; Latência 21 ms, Perda 0 %, Jitter 0 ms, Picos 0, com check verde); **Gráfico de latência** com séries Google/Cloudflare, marcadores de disponibilidade e perda na base, Méd/Mín/Máx. | ICMP a cada 1 s via `IcmpSendEcho2Ex` para 8.8.8.8, 1.1.1.1, gateway (rota padrão por `GetIpForwardTable2`) e endpoint AWS da região; jitter = média do \|Δ latência\|; pico = amostra > 2× mediana; IP público via header `cf-meta-ip` do Cloudflare. Logs em SQLite. |
| **Scanner** ("Diagnóstico de conexão") | gauges Download/Upload (Mbps), "Dados usados", "Ping médio", "Variação"; ISP, IP, interface; tabela **Atividade** (navegação, áudio, 4K, videoconferência, jogos) em condições ideais × sob carga; **Latência** sem carga / com carga download / com carga upload; **Bufferbloat** nota A+. | `speed.cloudflare.com/__down?bytes=N` / `__up` (CORS `*`), medir latência durante a carga; nota de bufferbloat = Δ latência sob carga (A+ < 5 ms … F > 400 ms, escala do Waveform); ISP/ASN/cidade dos headers `cf-meta-*`; interface via `GetAdaptersAddresses`. |
| **Ping para jogos** | biblioteca de jogos (86), servidores por região (`América do Sul › Sao Paulo (Valve) 28 ms`), ordenar/buscar, data do último teste. | Lista curada por jogo: Valve (CS2/Dota 2/Deadlock) via Steam `GetSDRConfig` (53 POPs, relay IPs); outros jogos: endpoints públicos conhecidos, atualizáveis por JSON no repo. ICMP ou TCP connect quando ICMP é bloqueado. |
| **Ferramentas** | MTR / Ping / TCPing com campo "IP do servidor ou domínio", engrenagem (opções), reiniciar, **Iniciar**; vazio: "Nenhum teste MTR executado ainda". | ICMP com TTL incremental (MTR por hop: perda, méd, mín, máx), ping contínuo, TCP connect timing. |

## PC Boost
| Aba | O que mostra | Como replicar |
|---|---|---|
| **Ajustes de sistema** | presets `Padrão` / `Alto desempenho` / `Personalizado`; cards com toggle: Potenciador de Unreal Engine (Beta), Alto desempenho da GPU (Beta), Reduzir input lag, Reduzir tempo de entrada do teclado, Desligar contadores de desempenho do Windows, Desempenho máximo para jogos; cada card com descrição + "Não recomendado se…". | Tweaks com `detect/apply/revert` + backup `.reg`. Mapeamento inicial: input lag = fullscreen optimizations off + GameDVR off + Game Mode; teclado = `HKCU\Control Panel\Keyboard` KeyboardDelay/Speed; contadores = `Perflib` Disable; desempenho jogos = MMCSS + HAGS + plano de energia; GPU = preferência de desempenho por app (Graphics Settings `UserGpuPreferences`); Unreal = variáveis de ambiente/ini — avaliar, provavelmente fora. |
| **Limpeza de RAM** | barra Usado 9,8 GB / Livre 22,1 GB / 32 GB; botão **Limpar**; "Última limpeza em…"; rotina: automatizar quando uma conexão de jogo começa / quando status muda para "Precisa de limpeza" (a cada 12 h); **Lista de permissões** (apps intocados); aviso "seu sistema pode ficar lento por um breve momento". | `GlobalMemoryStatusEx` para a barra; limpeza = purge da **standby list** (`NtSetSystemInformation(SystemMemoryListInformation, MemoryPurgeStandbyList)`, requer `SeProfileSingleProcessPrivilege`/admin); **não** `EmptyWorkingSet` em processos (comprovadamente piora — ver scripts); allowlist só faz sentido se um dia houver trim por processo. Rotina por timer no app. |
| **DNS Optimizer** | ranking Cloudflare `1.1.1.1/1.0.0.1` (Atual), Quad9 (Mais rápido, 3 ms), NextDNS, Google, Windows (padrão); "Resp. média"; botões **Escolher o mais rápido** / Escolher / **Restaurar padrão do Windows**; "Atualizado em…". | Query UDP `A` cronometrada (5 amostras, mediana) para cada provedor; aplicar por adaptador com `SetInterfaceDnsSettings` (admin); guardar config anterior para restaurar; mostrar DNS atual via `GetAdaptersAddresses`. |

## Outras telas
- **Limitador de tráfego** ("limita a velocidade para conexões fora do ExitLag") e **Multi Internet** (várias conexões simultâneas): dependem do túnel/rede deles → **fora de escopo v1**. Limitador local via QoS do Windows = backlog.
- **Rota do jogo** (diagrama Brasília01/São Paulo31 → BR São Paulo01, "Redirecionar login", "Reotimizar", "Aplicar rotas"): rede proprietária → fora de escopo.
- Shell: sidebar de ícones, topbar "ExitLag ON" + sino + busca, versão no rodapé, banner escuro por seção, abas sublinhadas, `?` de ajuda ao lado de títulos — replicar no design system.

## Fontes de dados gratuitas validadas (2026-10-14)
- Cloudflare: `GET https://speed.cloudflare.com/__down?bytes=1000000` → 200, `Access-Control-Allow-Origin: *`, headers expostos `cf-meta-ip|asn|country|city|colo|…`. Upload: `POST /__up`. Pacote `@cloudflare/speedtest` (browser) já calcula download/upload/latência/jitter/**loaded latency**.
- Steam SDR: `GET https://api.steampowered.com/ISteamApps/GetSDRConfig/v1/?appid=730` → `pops` (53) com `desc`, `geo`, `relays[].ipv4/port_range`. `gru` = São Paulo (155.133.227.35, 27015–27060).
- Evitar: Ookla Speedtest CLI (licença proprietária), fast.com (sem API oficial).
