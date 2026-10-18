# Analisador de rede e DNS Optimizer

**Última revisão:** 2026-10-18 · Código: `crates/core/src/network.rs`, `dns.rs`; shell `src-tauri/src/lib.rs` (`ping_host`, `trace_host`, `tcping_host`, `dns_*`); UI `src/pages/network.tsx`, `src/pages/boost.tsx` (aba DNS), `src/lib/speedtest.ts`.

## Monitor de conexão

ICMP por `IcmpSendEcho2` (iphlpapi): **sem admin e sem raw socket**. Alvos: Google (8.8.8.8), Cloudflare (1.1.1.1) e o **gateway** do adaptador principal (de `dns::adapters`) — separa "minha rede local/Wi-Fi" de "internet". Latência, jitter, perda e picos em janela deslizante.

## Teste de velocidade

Cloudflare (`speed.cloudflare.com/__down` e `__up`, CORS liberado): download, upload, latência ociosa × sob carga (**bufferbloat**), ISP/cidade dos cabeçalhos `cf-meta-*`. Roda no próprio webview. Alternativa avaliada para o futuro: M-Lab NDT7 (aberto, servidores próprios); o speedtest da Ookla é proprietário e fica fora.

## Ping para jogos

Steam Datagram Relay (`ISteamApps/GetSDRConfig`, sem chave) via `tauri-plugin-http` (host na allowlist): POPs com IPs de relay de CS2, Dota 2 e Deadlock (`gru` = São Paulo), pingados por ICMP.

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
