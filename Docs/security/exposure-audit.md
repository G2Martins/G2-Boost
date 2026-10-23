# Segurança › Exposição (auditoria de acesso remoto)

**Última revisão:** 2026-10-22 · Rodada 7, frente "Auditoria de acesso remoto e exposição" ([../core/next-steps.md](../core/next-steps.md)) · Código: `crates/core/src/exposure.rs`; shell `src-tauri/src/lib.rs` (`exposure_*`, monitor no laço de rotinas); UI `src/pages/security.tsx` (rota `security`); MCP `get_exposure_report`.

**O que é:** uma auditoria de **por onde alguém de fora chega neste PC** e como fechar cada porta. **Não é antivírus** (a tela diz isso): não procura vírus, não mata processo, não apaga nada.

## Verificações

Cada verificação tem a decisão numa função pura testada (`exposure::tests`) e um leitor que só junta os valores crus. Resultado: `{id, state (ok | exposed | unknown), severity (info | low | medium | high), details, items, fix}`.

| id | Fonte (nativa, sem shell) | Admin para ler | Exposto quando | Correção |
|---|---|---|---|---|
| `rdp` | `HKLM\…\Terminal Server\fDenyTSConnections` (+ política `SOFTWARE\Policies\…\Terminal Services`), porta em `WinStations\RDP-Tcp\PortNumber` | não | valor efetivo = 0 (ausente = negado). Alta sem NLA, média com | `rdp_disable` |
| `nla` | `RDP-Tcp\UserAuthentication` (+ política) | não | RDP ligado e valor = 0 (ausente = NLA exigida) | `nla_enable` |
| `remote_assistance` | `Control\Remote Assistance\fAllowToGetHelp` (+ política) | não | = 1 (baixa: precisa de convite) | `ra_disable` |
| `winrm` | SCM (`boost::svc_query`) + escuta em 5985/5986 | não | serviço rodando ou porta aberta fora do loopback | `winrm_stop` (para e volta a manual) |
| `smb1_server` | driver `srv` instalado e não desativado + `LanmanServer\Parameters\SMB1` ≠ 0 | não | ligado (alta) | `smb1_server_disable` (vale após reiniciar) |
| `smb1_client` | driver `mrxsmb10` não desativado | não | instalado (média) | sem botão: a tela mostra o caminho em Recursos do Windows |
| `firewall` | `INetFwPolicy2` (perfil atual + ligado por perfil); firewall de terceiros em `root\SecurityCenter2 FirewallProduct` | não | perfil desligado: alta no perfil em uso, média em outro; um nível abaixo se outro firewall está ativo | `firewall_on {profile}` |
| `upnp` | `UPnPNAT` → `StaticPortMappingCollection` numa thread própria com prazo de 5 s (o roteador pode não responder; uma consulta por vez) | não | mapeamento cujo cliente interno é um IP deste PC (`dns::adapters`); protocolo só TCP/UDP, cliente precisa ser IP | `upnp_remove {protocol, external_port}` (sem admin) |
| `remote_apps` | tabela de dados no core (nomes de exe, prefixos de serviço, trechos de `DisplayName`) contra processos, serviços (`SYSTEM\CurrentControlSet\Services`) e `apps::desktop_list` | não | encontrado: rodando = média, serviço automático = baixa, só instalado = info. Assistência Rápida só conta rodando | sem auto-kill: abrir local + ir para Desinstalar apps |
| `listeners` | `netconns::connections` (TCP em escuta) | não | escuta fora do loopback por programa fora de `%windir%`; risco pela porta (21, 23, 135, 139, 445, 3389, 5555, 5900–5902, 5985/5986 = alta; 22 e bancos = média). Processo ilegível só conta em porta de risco que não é do Windows | abrir local |
| `inbound` | mesmas tabelas | não | TCP estabelecida cuja porta local é uma porta em escuta e o remoto é IP público | abrir local |
| `sessions` | `WTSEnumerateSessionsW` + `WTSClientProtocolType`/`WTSClientName` | não | sessão RDP (protocolo 2) ativa/conectada (alta) ou desconectada (média) | sem botão |
| `persistence` | tarefas (Task Scheduler COM, recursivo, sem a pasta `\Microsoft`, só ativas) e serviços de processo próprio/compartilhado | não (elevado vê mais tarefas) | exe sem assinatura em pasta do usuário (alta), sem assinatura (média), assinado mas em Temp/Downloads/Public (média), `rundll32`/`regsvr32`/`mshta`/`wscript`/`cscript`/`powershell`/`cmd`/`msiexec` com argumento em pasta do usuário (alta). Assinado em AppData = normal (atualizadores por usuário) | tarefa: `task_disable {path}` (só tarefas do último relatório) |
| `guest` | WMI `Win32_UserAccount` local, SID terminando em `-501` | não | ativada (média) | sem botão |
| `antivirus` | WMI `root\SecurityCenter2 AntiVirusProduct`, `productState` (bits 12–15 = 1 ligado; bits 4–7 = 0 em dia) | não | nenhum ligado (alta) ou ligado só com definições velhas (média) | sem botão |

**Assinatura:** `WinVerifyTrust` (`WINTRUST_ACTION_GENERIC_VERIFY_V2`, sem revogação, só cache de URL) e, quando o arquivo não tem assinatura embutida, o hash Authenticode procurado nos catálogos do sistema (`CryptCATAdmin*`, SHA256 e SHA1): muitos programas do Windows fora de `%windir%` são assinados só por catálogo (ex.: `wmpnetwk.exe`, que aparecia como "sem assinatura" antes do fallback). Cache por caminho, verificação em paralelo (rayon), só para programas fora de `%windir%`.

Nomes de apps, serviços, tarefas e mapeamentos vêm do PC e do roteador (dados, sem caracteres de controle, até 160 chars); a UI nunca os tem em i18n.

## Correções (admin, confirmação, backup, desfazer)

- Toda correção chega da UI como `Fix` (enum serde fechado; porta fora de `u16`, perfil desconhecido ou `kind` desconhecido = recusado) e é **revalidada contra o estado ao vivo** antes de escrever: política de grupo presente = `managed_by_policy`; mapeamento UPnP que não existe mais ou não aponta para este PC, ou tarefa fora do último relatório = `not_applicable`.
- Sem elevação: `needs_admin` (exceto `upnp_remove` e `task_disable`, que o Windows deixa o usuário fazer; tarefa protegida devolve `needs_admin` do próprio Task Scheduler). A tela mostra Elevar.
- **Backup antes** com `registry::save_backup` (ids `exposure_*`; o primeiro backup é o original e não é sobrescrito): valor do registro (ou ausente), estado do firewall por perfil, mapeamento UPnP inteiro (JSON), tipo de início e estado do WinRM, tarefa ativa. O próprio `Fix` vai junto (entrada `fix/json`) para a tela listar "Mudanças feitas pelo G2 Boost" com Desfazer.
- **Desfazer** lê o backup como entrada hostil: só aplica as entradas que aquela correção poderia ter escrito (caminho e nome esperados, tipo DWORD/ausente, IP do mapeamento ainda deste PC) e apaga o backup.
- Nunca `sc.exe`, `netsh`, `reg.exe`: SCM (`ChangeServiceConfigW`/`ControlService`), `INetFwPolicy2::put_FirewallEnabled`, `IStaticPortMappingCollection::Remove/Add`, `RegSetValueExW`, Task Scheduler COM.
- Abrir local (`exposure_reveal`) só aceita caminhos que o último relatório mostrou; vai pelo `disk::reveal` (shell API).

## Monitor (laço de rotinas)

A cada ~5 min (primeira passada ~10 s após abrir, que só grava a linha de base) e fora do Modo Jogo com pausa, numa thread própria (nunca duas ao mesmo tempo): programa novo aceitando conexões fora do loopback, conexão nova vinda da internet (por programa), programa de acesso remoto aberto, mapeamento UPnP novo para este PC. Linha de base em `%LOCALAPPDATA%\G2 Boost\exposure_baseline.json` (lista de chaves `tipo:…`; ao ler: JSON inválido = nova linha de base, chaves com tipo desconhecido, controle ou > 200 chars descartadas, até 2000). Só cresce: cada coisa avisa uma vez. Chaves por programa, não por porta (programas que escutam em porta aleatória não geram alerta a cada abertura).

Avisos: evento `exposure:alerts` → contador no ícone Segurança da barra lateral e card "Novidades desde a última verificação" (Entendi limpa). O app não tem notificação de desktop; não foi adicionado plugin para isso. Configurações › "Vigiar exposição" (`settings.exposure_monitor`, padrão ligado).

## Conselheiro, Luli e MCP

`advisor::Env.exposure` = `exposure::quick()` (registro, firewall, antivírus, conexões de entrada e os mapeamentos UPnP da última consulta se tiverem < 15 min; ~0,2 a 1,5 s). Achados (`kind: security`): `rdp_open`, `nla_off`, `firewall_off`, `smb1_on`, `upnp_open`, `remote_inbound`, `av_off` (alta → crítico, média → aviso). A Luli recebe o retrato da tela (`security.report`). MCP `get_exposure_report`: relatório completo, só leitura, sem caminhos de perfil, IPs remotos e nomes de computador cliente.

## Na máquina do dono (2026-10-22, sem admin)

Relatório completo em 2,2 a 3,2 s; rápido em 0,2 a 1,5 s. RDP desligado, WinRM parado (manual), SMBv1 ausente (servidor e cliente), firewall OK com o firewall de terceiros ativo, antivírus de terceiros ligado e em dia (o Defender aparece desligado, como esperado com outro antivírus), Convidado desativado, nenhuma sessão remota, nenhum app de acesso remoto, nenhuma conexão de entrada, roteador respondeu UPnP com 0 mapeamentos. Exposto: **Assistência Remota com convites permitidos** (baixa) e um agente de periférico escutando numa porta alta em todas as interfaces (info). Persistência: nada (o falso positivo de catálogo foi corrigido).

**Não verificado aqui (precisa de elevação, roteiro no relatório da rodada):** aplicar/desfazer de cada correção, `task_disable`, remover e recriar mapeamento UPnP num roteador com mapeamentos, relatório elevado (mais tarefas e processos legíveis).

## Limites conhecidos

- Assinatura por catálogo só pela existência do hash num catálogo instalado (adicionar catálogo exige admin); não validamos o assinante.
- Tarefas sem permissão de leitura para o usuário comum não aparecem sem elevação.
- UPnP só IPv4 (o roteador responde IPv4); IGD que mente sobre o cliente interno é filtrado (cliente precisa ser IP deste PC).
- Conexões de entrada por UDP não são contadas (tabelas UDP não têm estado).
