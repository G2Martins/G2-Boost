# Kaspersky 21.26 e Kaspersky VPN 5.26 — inventário de recursos (clean-room)

**Última revisão:** 2026-10-18 · Método: inventário estático com REA (`inspect_pe_resources`), tabelas de import e busca de strings; **nada executado, nada descompilado** (EULA), nenhum acesso negado. Notas e dumps fora do repo em `D:\dev\g2boost-experiments\kaspersky-rea\`. Legenda: **[O]** observado, **[I]** inferido, **[U]** desconhecido. Interno: não citar em README nem na UI.

## O que vale trazer (ordenado por valor ÷ esforço)

| # | Recurso no Kaspersky | Mecanismo | Versão G2 Boost (nativa, sem driver) | Admin | Esforço |
|---|---|---|---|---|---|
| 1 | Histórico de câmera/microfone (`WebCamProtection`, lê `CapabilityAccessManager\ConsentStore`) [O] | Bloqueio é driver HIPS; o histórico vem do ConsentStore [I] | Ler `ConsentStore\webcam` e `\microphone` (HKCU/HKLM, `LastUsedTimeStart/Stop` por app): "quem usou sua câmera e quando" | não | S |
| 2 | Modo Jogo / Não perturbe (`resource_saving/gaming_mode.cpp`, "Gaming detection", "Fullscreen") [O] | Detecção de tela cheia + lista de exes, adia tarefas [I] | Detector atual + `SHQueryUserNotificationState`: pausar rotinas e avisos durante o jogo | não | S |
| 3 | Saúde do disco S.M.A.R.T. com histórico (`drive_health_monitor/*`, "HardDriveHealthDecreased") [O] | IOCTL SMART/NVMe [I] | `IOCTL_STORAGE_QUERY_PROPERTY` (NVMe health log / ATA SMART) em `\\.\PhysicalDriveN`, histórico JSON, alerta quando piora | alguns discos | M |
| 4 | Atividade de rede por app ("Atividade dos Aplicativos"; imports `GetExtendedTcpTable/UdpTable`) [O] | Tabelas IP Helper → pid; bytes por processo vêm do driver [I] | Aba "Quem usa a rede": conexões por processo (sem admin); taxa por processo via ETW `Kernel-Network` (admin) | bytes: sim | M |
| 5 | Wi-Fi: rede insegura + analisador de canais (`wifi_protection.dll`, `wifianalyzerpage.xaml`) [O] | WLAN API [I] | `WlanGetAvailableNetworkList`/`WlanGetNetworkBssList`: tipo de segurança (Aberta/WEP/TKIP), sinal, canal congestionado | não | S–M |
| 6 | Apps pouco usados (`UnusedAppsScanTask`) [O] | Fonte exata [U] | `UserAssist` (HKCU, ROT13: contagem + última execução), datas da pasta, Steam `LastPlayed`; só sugere desinstalar. Prefetch continua proibido | não | S–M |
| 7 | Duplicados e arquivos grandes (`DuplicateFilesScanner::GetFastHash`, extensões importantes atualizáveis) [O] | Tamanho → hash rápido → hash completo [O/I] | Em cima da árvore do scan de disco; Lixeira com a guarda atual | não | M |
| 8 | Inicialização rápida (impacto no boot, `QuickStartupNotificationLogic`) [O] | Run/Startup + medição de boot [I] | Event Log `Diagnostics-Performance` 100/101 via `EvtQuery` (já temos `winevents.rs`) | não | S–M |
| 9 | Atualizador de apps (`am_patch_management.dll`, catálogo próprio) [O] | Catálogo deles [O] | `winget upgrade` com confirmação (já no Console) numa tela própria | não | S |
| 10 | Configurações fracas do Windows (`weak_settings.dll`, histórico de correções) [O] | Regras em arquivo de conteúdo [U] | Tweaks com backup: SMBv1, autorun, nível do UAC, RDP exposto | HKLM: sim | M |
| 11 | Limpeza de privacidade (wizard com restauração) [O] | Regras em `kleaner.pack` (cifrado) [U] | Catálogo: MRU de documentos recentes, jump lists, histórico da área de transferência, navegadores por perfil | não | M |

## O que não trazer

VPN (WireGuard go/NT, Hydra/AmneziaWG, OpenVPN; precisa de servidores), split tunneling (driver `kltun.sys`), firewall por callout (`klwtp.sys`), verificação de contas vazadas (manda dado para fora), backup, cofre, controle parental, triturador.

**Curiosidade útil:** o kill switch da VPN é **WFP em modo usuário** (`FwpmEngineOpen0`/`SubLayerAdd0`/`FilterAdd0` por AppId) [O], sem driver de callout. Serve de modelo para um "bloquear a rede deste app" com admin, se um dia for pedido.
