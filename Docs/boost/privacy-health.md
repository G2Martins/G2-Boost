# Privacidade, saúde dos discos, rede por app, Wi-Fi, boot, prioridades, apps pouco usados, duplicados

**Última revisão:** 2026-10-19 (testes ponta a ponta com e sem admin, taxas por processo via ETW, ícones dos programas, boot com DPS desligado) · Módulos de core feitos por subagentes em worktrees isolados e integrados no mesmo dia; todos só leitura, sem spawn de processo, com testes das partes puras e um teste ao vivo ignorado. Ideias vindas do inventário do Kaspersky ([../references/kaspersky.md](../references/kaspersky.md)).

| Tela | Módulo | Fonte | Admin | Na máquina do dono (2026-10-18) |
|---|---|---|---|---|
| PC Boost › Privacidade | `consent.rs` | `HKCU`/`HKLM` `...\CapabilityAccessManager\ConsentStore\{webcam,microphone,location}` (`NonPackaged` com `#` no lugar de `\`; `LastUsedTimeStart/Stop` FILETIME) | não | 390 registros; "em uso" inflado por paradas zeradas após travamento → UI mostra "pode estar em uso" só se o início foi na última hora |
| Meu PC › Saúde dos discos | `smart.rs` | NVMe: log 0x02 por `IOCTL_STORAGE_QUERY_PROPERTY` (protocol-specific); SATA: `SMART_RCV_DRIVE_DATA` | SATA: sim | XPG S70 (NVMe) lido sem admin: 51 °C, 0% de desgaste, 8 TB gravados; Kingston SATA elevado: 26 atributos, saúde Boa; USB sem dados |
| Rede › Uso por app | `netconns.rs` + `netetw.rs` | `GetExtendedTcpTable`/`GetExtendedUdpTable` com pid; taxas por processo pela sessão ETW `G2Boost-NetUsage` (Microsoft-Windows-Kernel-Network, eventos de envio/recebimento TCP/UDP v4/v6) | taxas: sim | 275 conexões, 39 processos; elevado: colunas Baixando/Enviando (ex.: claude.exe 2 KB/s ↓ 4,2 KB/s ↑). Sessão só abre com a aba aberta e fecha ao sair dela (`net_usage_stop`) |
| Rede › Wi-Fi | `wlan.rs` | WLAN API (`WlanQueryInterface`, `WlanGetNetworkBssList`) | não | PC cabeado, sem serviço WLAN → "este PC não usa Wi-Fi". 24H2: sem acesso à localização o Windows esconde SSID (tela explica) |
| PC Boost › Inicialização › Tempo de boot | `boottime.rs` | log `Diagnostics-Performance/Operational` (100 = boot; 101–109 = o que atrasou) | sim | `needs_admin` sem elevação (botão Elevar). Elevado: nenhum boot desde 10/05/2025 porque o Serviço de Política de Diagnóstico (ajuste `svc_dps`) está desligado; o card mostra a última medição (`last_known`) e explica (`dps_off`) |
| PC Boost › Processos › Prioridade fixa | `prio.rs` | `SetPriorityClass` por regra `nome.exe → classe` (Baixa…Alta, **nunca tempo real**), uma vez por processo (`Enforcer`), sem programas do Windows/antivírus/sessão 0 | não (processos do usuário) | teste ao vivo mudou só um processo filho do próprio teste |
| Desinstalar apps › ordenar por Uso | `userassist.rs` | `UserAssist\{GUID}\Count` (ROT13; registro de 72 bytes: execuções, foco, última execução) | não | 273 entradas; é pista (apps abertos por outros programas não contam) |
| Disco › Duplicados | `dupes.rs` | árvore do último scan → tamanho → hash do começo e do fim (64 KiB) → comparação byte a byte; hard links excluídos | não | — |

**Segurança dos duplicados:** `dupes_recycle` só aceita caminhos que estão no último resultado, recusa apagar todas as cópias de um grupo (`keeps_none`), passa cada caminho pela guarda do mapa do disco (`validate_recycle`) e manda para a Lixeira.

**Alerta de saúde:** `disk_health` guarda uma leitura por dia por disco (chave = hash de modelo+serial, sem serial cru) e mostra o que piorou (`worsened`: realocados/pendentes subiram, desgaste +5, reserva −10, alerta crítico novo).

**Ícones dos programas:** Privacidade, Processos e Uso por app mostram o ícone do exe (`exe_icon` no shell: só `.exe` local que existe, nunca UNC, cache de 1000; `ExeIcon` na UI com inicial de reserva). Na Privacidade, versão antiga apagada procura o exe atual em pastas irmãs de versão; sem exe = selo "removido".

**Limpeza:** 16 categorias novas (navegadores, launchers, apps e caches de dev) em [../cleanup/catalog.md](../cleanup/catalog.md).
