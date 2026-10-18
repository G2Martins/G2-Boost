# Analisador de rede e DNS Optimizer

**Última revisão:** 2026-10-18 · Código: `crates/core/src/network.rs`, `dns.rs`, `devices.rs` (+ `data/oui.tsv`, gerado por `tools/oui.py`), UI `src/components/network/devices-tab.tsx`; shell `src-tauri/src/lib.rs` (`ping_host`, `trace_host`, `tcping_host`, `dns_*`); UI `src/pages/network.tsx`, `src/pages/boost.tsx` (aba DNS), `src/lib/speedtest.ts`.

## Monitor de conexão

ICMP por `IcmpSendEcho2` (iphlpapi): **sem admin e sem raw socket**. Alvos: Google (8.8.8.8), Cloudflare (1.1.1.1) e o **gateway** do adaptador principal (de `dns::adapters`) — separa "minha rede local/Wi-Fi" de "internet". Latência, jitter, perda e picos em janela deslizante.

## Teste de velocidade

Cloudflare (`speed.cloudflare.com/__down` e `__up`, CORS liberado): download, upload, latência ociosa × sob carga (**bufferbloat**), ISP/cidade dos cabeçalhos `cf-meta-*`. Roda no próprio webview. Alternativa avaliada para o futuro: M-Lab NDT7 (aberto, servidores próprios); o speedtest da Ookla é proprietário e fica fora.

## Ping para jogos

Steam Datagram Relay (`ISteamApps/GetSDRConfig`, sem chave) via `tauri-plugin-http` (host na allowlist): POPs com IPs de relay de CS2, Dota 2 e Deadlock (`gru` = São Paulo), pingados por ICMP.

## Dispositivos (`devices.rs`, aba Dispositivos; Fase 21, 2026-10-18)

Quem está na rede, **sem driver, pcap, socket bruto nem admin** (o niv-on usado como referência de ideias depende de Npcap + admin; aqui não):

1. **Varredura ICMP** do /24 deste PC (rede maior que /24 = só o /24 em volta; /31 e /32 = nada), 64 threads, 400 ms. Serve para o Windows resolver ARP e preencher a tabela de vizinhos (aparelho que bloqueia ping ainda responde ARP).
2. **`GetIpNetTable2`** na interface principal: IP ↔ MAC (estados Probe a Permanent; ignora MAC zerado, broadcast e multicast).
3. **mDNS / DNS-SD** (224.0.0.251:5353, bit de resposta unicast, socket efêmero ligado ao IP do PC): tipos de serviço, nome da instância só para serviços conhecidos (`_googlecast`, `_airplay`, `_ipp`…), hostname `.local`. Serviços com id aleatório em hex são descartados. Parser DNS próprio com limites (ponteiros só para trás, ≤ 32 rótulos, ≤ 256 registros); teste corta o pacote em todo tamanho sem pânico.
4. **SSDP / UPnP** (M-SEARCH `ssdp:all`): `SERVER`, `ST`; a descrição XML (`friendlyName`, `manufacturer`, `modelName`, `deviceType`) só é buscada se o `LOCATION` aponta para **o mesmo IP que respondeu**, `http://` puro, porta válida, caminho ASCII ≤ 200, 64 KB, timeouts curtos (anti-SSRF: um aparelho não consegue fazer o app buscar outro endereço).
5. **DNS reverso** (`GetNameInfoW` com `NI_NAMEREQD`) em paralelo.
6. **Fabricante** pelo registro público MA-L do IEEE (40 mil prefixos, 800 KB embutidos, nomes encurtados por `tools/oui.py`; baixar `oui.csv` em `D:\dev\oui` para regenerar). MAC administrado localmente = "aleatório" (celulares, Windows com endereço aleatório).
7. **Tipo provável** em ordem: este PC, roteador (gateway), serviços mDNS/UPnP, nomes (hostname, UPnP, modelo), fabricante, MAC aleatório (provável celular), desconhecido.
8. **Histórico** em `devices.json` (MAC → primeira/última vez, conhecido; chaves inválidas descartadas ao carregar; até 500): **Novo** só a partir da segunda varredura; "Conheço este" tira o selo.
9. **Portas de risco (opcional, por aparelho):** conexão TCP de 600 ms a 16 portas (Telnet 23/2323 e ADB 5555 = alto; FTP, SMB, RTSP, MQTT, RDP, VNC, TR-069 = médio; SSH, HTTP, NetBIOS, impressão = informativo), cada uma com explicação. **Só IPs da última varredura** (`LAST`): o app não vira scanner de endereço arbitrário.

Na máquina do dono (2026-10-18): 4 aparelhos em 7,2 s (roteador ZTE via UPnP IGD, este PC, dois celulares: um pelo hostname Android, outro pelo MAC aleatório). Também no Console (`dispositivos`) e no MCP (`get_network_devices`, sem MACs). A Luli lê o retrato da aba sem MACs. Backlog: mapeamentos de porta UPnP do roteador (o que está exposto à internet), alerta de aparelho novo pela rotina em segundo plano.

## Ferramentas

| Ferramenta | Como |
| --- | --- |
| Ping | `IcmpSendEcho2`, até 100 sondas, timeout e intervalo limitados no shell |
| MTR / traceroute | TTL incremental com `IcmpSendEcho2Ex` (sem driver), até 64 saltos |
| **TCPing** | `host:porta` (padrão 443): handshake TCP cronometrado (`network::tcping`, connect com timeout) — mede servidores que bloqueiam ICMP; mesmas estatísticas do ping |

Toda entrada é limitada no Rust (`clamp`), resolução só IPv4 por enquanto.

## DNS Optimizer (`dns.rs`, PC Boost › DNS)

- **Catálogo no core** (10 provedores: Cloudflare, Quad9, Google, NextDNS, OpenDNS, Control D, AdGuard, Verisign, Comodo, Neustar). A UI só manda o **id** — nunca um IP arbitrário.
- **Medição:** consulta DNS UDP montada à mão (`build_query`, id novo por consulta, validação de resposta `is_reply`) para nomes populares em rodízio (já em cache em todo resolver grande → mede ida e volta até o resolver, não recursão), N amostras por provedor em paralelo; mediana + perda; ordenado do mais rápido.
- **Adaptador:** `GetAdaptersAddresses` (ativo, com gateway, sem loopback) + `GetInterfaceDnsSettings` (servidores estáticos × automáticos); mostra provedor atual quando bate com o catálogo.
- **Compatibilidade:** `Get/Set/FreeInterfaceDnsSettings` só existem no Windows 10 2004+; são carregadas por `GetProcAddress` (`dns_api()`), então o app abre em builds antigos e aplicar responde `unsupported_windows` (a medição funciona em qualquer versão).
- **Adaptador:** `GetBestInterface` (rota para 8.8.8.8, nada é enviado) marca `primary` = o que carrega o tráfego; ele vem primeiro e a UI deixa escolher outro quando há vários (VPN, Wi-Fi + cabo).
- **Allowlist no core:** `dns::apply(guid, provider_id)` aceita só id de `PROVIDERS` — UI, MCP e rotinas passam pela mesma validação.
- **Aplicar (admin):** `SetInterfaceDnsSettings` no GUID do adaptador (precisa existir na lista ao vivo), backup do estado anterior em `%LOCALAPPDATA%\G2 Boost\dns_backup.json` (só na primeira troca) e flush do cache DNS. **Restaurar** devolve exatamente o anterior (estático ou automático/DHCP).
- Medição real na máquina do dono (2026-10-15): NextDNS 4,1 ms, Cloudflare 4,3, Quad9 4,5, Google 33,8.
- DNS não aumenta FPS: só encurta resolução de nomes (a IA é instruída a dizer isso).

## Testes

Pacote DNS e validação de resposta, adaptadores ao vivo, `NeedsAdmin` sem elevação, adaptador desconhecido recusado, ranking ao vivo (ignorado), TCPing contra listener local, estatísticas de latência.

## Fora de escopo (v1)

Rotas otimizadas/multipath/túnel e limitador de tráfego — exigem rede de servidores própria. Limitador por QoS local e IPv6 nas ferramentas = backlog.
