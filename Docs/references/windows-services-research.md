# Serviços do Windows — pesquisa com fontes (2026-10-14)

**Última revisão:** 2026-10-15 · Produzido por sub-agente de pesquisa (Sonnet) a pedido do dono para validar o catálogo de serviços do Boost contra Galdino Tech, winutil (Chris Titus) e documentação Microsoft. Leituras na máquina de dev foram somente leitura. Decisões derivadas estão em [../boost/tweaks.md](../boost/tweaks.md).

## Resumo
- **Galdino:** só um vídeo cita serviços (WSearch, SysMain, DiagTrack, DPS). Sem benchmark. O canal vende otimização remota paga ("Windows Gamer"); os 3 posts do Instagram indicados são anúncios sem conteúdo técnico. Outros vídeos usam "packs" (RAR com `.reg`/`.exe` de MediaFire, rodados como admin) — lista de serviços não é mostrada.
- **winutil** (HEAD, `tweaks.json` de 2026-09-19): mexe em 5 serviços + `lfsvc`. Nunca desliga SysMain, WSearch, Xbox, DPS nem WerSvc. Até 2026-04-21 a lista tinha 191 serviços, mas só 16 mudanças reais (140 já eram Manual). O commit `87a5779f` ("tweak service reduction") e o PR #4466 justificam que o tweak "tem que fazer o que diz".
- **Ganho de FPS:** nenhum benchmark controlado (TechPowerUp, Tom's, Gamers Nexus, Igor's Lab) isola serviços. Dado indireto mais forte: TechTeamGB, AtlasOS e Windows stock empatam. Medição local: 18 serviços padrão em execução somam ~0,43 GB de working set e ~0,04 % de CPU. Única alavanca mensurável: **VBS/HVCI (Isolamento de Núcleo) ≈ −5 %** (Tom's Hardware 2021 e 2023) — não é serviço e custa segurança.
- **Catálogo:** DiagTrack em Padrão ok. SysMain e WSearch = ganho baixo/nulo em SSD (SysMain atrás de guarda de HDD). dmwappushservice e os 4 serviços Xbox têm ganho nulo (já Manual e parados) e efeitos colaterais reais → Personalizado.
- **Padrões do Win11:** sem fonte oficial. Usados: leitura local dos serviços não alterados, `OriginalType` do winutil e doc Microsoft de Windows Server 2016 (padrões de Server diferem). A máquina de dev não é baseline limpo (SysMain, DiagTrack, WSearch, DPS, MapsBroker já Disabled).

## A. Galdino Tech

### A.1 Acesso
- YouTube `@GaldinoTech`: 29 vídeos longos + 48 Shorts; legendas automáticas (ASR, "sisman" = SysMain) lidas em 24 dos 27 vídeos de PC. Sem legenda: `Icm6vkZ79bo`, `dHfzoP7wYE4` (HTTP 429), `6ZebzX1DMo0`.
- Instagram: bloqueado (só meta tags). `p/DJowfeRuAyV` e `p/DJpyTYRv6Jz` (14/05/2025): "mande uma msg" (anúncio). `reels/DbyWqYxxq5i` (08/08/2026): "Windows Otimizado", atrás de login.
- TikTok: bloqueado; só títulos ("Desative e ganhe FPS!", "Desative a Otimização de Entrega do Windows para Melhorar o Ping") — conteúdo não verificado.
- windowsgamer.com.br: marketing ("+10.000 PCs otimizados"), checkout Hotmart, sem lista de tweaks.
- Vídeos linkados e indisponíveis: "Otimização do Windows 2024", "Script Secreto do Windows", "Otimizar GPU Nvidea", "Aumentar FPS Fortnite".

### A.2 O que ele recomenda
| Vídeo | Itens |
|---|---|
| COMPUTADOR TRAVANDO MUITO? PROCESSADOR E DISCO EM 100%? — https://www.youtube.com/watch?v=WlHcrldBJdY | **Serviços:** WSearch, SysMain, DiagTrack, DPS (capítulo "Desativando Serviços inúteis", 1:13). **Outros:** desligar Isolamento de núcleo; msconfig → ocultar Microsoft e desmarcar o resto (mantém RGB/placa-mãe e Vanguard); limpar inicialização; apps em segundo plano off; plano Desempenho Máximo; Disk Cleanup com tudo marcado; Malwarebytes. Manda **instalar** as atualizações de segurança. |
| COMO DESABILITAR A TELEMETRIA DO WINDOWS — https://www.youtube.com/watch?v=QyxPKbtHIdU | Nenhum serviço. Desativa 3 tarefas agendadas em Microsoft › Windows (pasta "Diagnos…", nome ambíguo na legenda). |
| CS2 — https://www.youtube.com/watch?v=QmlZLGznf2Y · Arena Breakout — https://www.youtube.com/watch?v=8D1skH_SHd8 | Pack/ferramenta de RAR como admin + `.reg` (prioridade de jogo, input delay, aceleração do mouse); opções "Disable Startup telemetry", "Timer Resolution Service", "Disable [un]necessary System Services". **Lista não mostrada.** |
| VALORANT — https://www.youtube.com/watch?v=CLFTR6rJbT0 | Desativa o HPET no Gerenciador de Dispositivos; ele mesmo diz que "mal chega a 1 %". Limpa arquivos do Windows Update. |

### A.3 Itens arriscados
- Isolamento de núcleo/HVCI off: perde proteção; ganho ~5 % (Tom's).
- Packs opacos (RAR + `.reg`/`.exe` de MediaFire como admin): supply chain + heurística de AV.
- msconfig "desmarcar tudo": pode desligar anti-cheat, antivírus, drivers, updaters.
- DPS off: diagnósticos param (MS).
- "WSearch atrasa a inicialização": não verificado; o indexador recua com usuário ativo (MS).
- Disk Cleanup "todas as caixas": remove componentes de update (impede desinstalar updates).
- **Não encontrado** nos vídeos lidos: desligar Windows Update/Defender, limpar Prefetch, chaves de Policies (os `.reg` dos packs não foram vistos).

## B. Tabela de serviços
Legenda: **G** Galdino · **Wn** winutil atual · **Wa** winutil até 2026-04-21 · **MS** doc Microsoft (Server 2016) · **VoD** valleyofdoom/PC-Tuning · **L** leitura local (Win11 Pro 26200) · **alt.** alterado nesta máquina · **\*** não verificado em fonte.

| Serviço | O que faz | Padrão Win11 | Recomendam desligar | Efeitos colaterais | Ganho p/ jogos | Preset |
|---|---|---|---|---|---|---|
| `SysMain` | Pré-carrega apps na RAM (Superfetch/Prefetch) | Auto | G sim; VoD só sem HDD; MS IoT tempo-real só RT; Wn/Wa não | Em HDD apps abrem mais devagar* | Baixo/nulo em SSD + 16 GB; sem benchmark | **Padrão (decisão do dono) com guarda: nunca em HDD** |
| `DiagTrack` | Telemetria | Auto | G, Wn, Wa sim | No Pro/Home o nível "off" não existe; desligar o serviço é o jeito | Baixo (privacidade/rede) | **Padrão** |
| `dmwappushservice` | WAP Push (MDM) | Manual (trigger) | ninguém; MS Server "OK" | Intune/MDM | Nulo (já Manual) | junto com DiagTrack (home PC) |
| `WSearch` | Indexação | Auto Delayed* | G, VoD sim; Wn/Wa não | Busca no Iniciar/Explorer/Outlook lenta ou vazia | Baixo; indexador recua com usuário ativo (MS) | **Alto** (aviso) |
| `XblAuthManager`, `XblGameSave`, `XboxNetApiSvc`, `XboxGipSvc` | Xbox Live auth/saves/rede/acessórios | Manual (2 trigger) | ninguém; MS Server "should disable" (só Server) | MS: apps podem não funcionar; saves não sincronizam; reativar deu erro 1068 (Q&A) | Nulo (parados) | **Personalizado** (removido do Alto) |
| `DPS` (+Wdi*) | Diagnóstico/troubleshooters | Auto | G sim; MS IoT RT; Wn/Wa não | MS: diagnósticos param | Baixo/nulo | **Personalizado** |
| `WerSvc` | Relatório de erros | Manual (trigger) | Wn mira `wermgr` (não existe → no-op) | MS Server "Do not disable" | Nulo | **NUNCA** |
| `MapsBroker` | Mapas offline | Auto Delayed | Wn → Manual; MS "OK" | apps de mapas offline | Nulo | fora |
| `PcaSvc` | Assistente de compatibilidade | Auto Delayed (trigger) | Wa → Manual; MS "OK" | PCA para (jogos antigos*) | Baixo (40 MB WS) | fora |
| `lfsvc` | Geolocalização | Manual (trigger) | Wn Disabled (Location); MS "OK" | apps perdem localização | Nulo | **Personalizado** (privacidade) |
| `TextInputManagementService`/`TabletInputService` | Entrada de texto/IME | Auto (trigger) | ninguém | MS "Do not disable" | Nulo | **NUNCA** |
| `WbioSrvc` | Biometria (Hello) | Manual (trigger) | Wa Manual | sem biometria | Nulo | fora |
| `RetailDemo`, `Fax`, `WMPNetworkSvc`, `PhoneSvc`, `wisvc` | legados/raros | Manual | Wa Manual (no-op) | PhoneSvc: VoIP; wisvc: Insider | Nulo | fora |
| `RemoteRegistry` | Registro remoto | **Disabled** | já é o padrão | — | Nulo | nada a fazer |
| `TrkWks`, `SCardSvr`, `Sensor*`, `SSDPSRV`/`upnphost`, `DusmSvc` | diversos | Manual/Auto | Wa Manual | SSDP: descoberta UPnP/DLNA (NAT P2P*) | Nulo | fora |
| `SessionEnv`, `TermService`, `UmRdpService` | RDP | Manual, parados | Wa Manual | MS "Do not disable" | Nulo | **NUNCA** |
| `Spooler` | Impressão | Auto | MSRC: mitigação PrintNightmare | sem impressão | Nulo | **NUNCA** (hardening ≠ jogo) |
| `DoSvc` | Otimização de Entrega (downloader de WU, Store, Game Pass) | Manual (trigger) | G (TikTok, não aberto); Wn só política | MS: é o downloader; P2P se desliga por **política** de download mode (0/99) | Baixo, por política | **NUNCA** o serviço; política = `Policies` (gatilho Kaspersky) → fora |
| `WpnService`, `CDPSvc`, `OneSyncSvc`+PIM | push, dispositivos conectados, sync | Auto/Manual | Wa Manual; MS "OK" | sem notificações/live tiles; Mail/Calendário param | Nulo | fora |
| `edgeupdate`, `GoogleUpdater` | updaters | Auto Delayed/Auto, parados | G implícito (Chrome) | navegador sem atualização | Nulo | **NUNCA** nos presets |
| `NVDisplay.ContainerLocalSystem` | backend do driver/Painel NVIDIA | Auto | ninguém | Painel NVIDIA para (fórum) | Nulo (maior CPU da lista: ≈0,03 %) | **NUNCA** |
| `NvContainerLocalSystem` | app NVIDIA/overlay | Auto | ninguém | perde overlay/ShadowPlay* | Nulo | fora |
| `CscService`, `SharedAccess`, `StorSvc` | Offline Files, ICS, Storage Sense | Manual/Auto Delayed | Wn Disabled/Manual | perde Offline Files/Hotspot | Nulo | fora |
| `wuauserv`, `BITS`, `UsoSvc`, `WaaSMedicSvc`, `WinDefend`, `mpssvc`/`BFE` | update, Defender, firewall | — | VoD/MS IoT só RT | MS: "não recomendamos desativar"; BFE "reduz significativamente a segurança" | — | **NUNCA** |
| `wlidsvc`, `ClipSVC`, `AppXSvc`, `iphlpsvc` | conta MS, Store, AppX, Teredo | Manual/Auto | ninguém | WU para de oferecer feature updates; Store quebra; Xbox/DO peering | — | **NUNCA** |

**Para o app:** `revert` restaura o valor pré-aplicação, então quem já tem serviço Disabled por outra ferramenta nunca volta ao padrão do Windows por esse caminho → "Restaurar padrão" usa a tabela `default_start` verificada.

**Medição local (somente leitura):** 18 serviços padrão em execução = 428 MB WS e 59,7 s de CPU em 3,18 h (~0,04 %). 83 instâncias de svchost, 396 MB privados.

## C. O que não fazer e mitos
1. Windows Update, Defender, Firewall, DoSvc: Microsoft não recomenda desativar. P2P do DO se controla por política, não pelo serviço.
2. Cadeia Store/Xbox (`wlidsvc`, `ClipSVC`, `AppXSvc`, `XblAuthManager`, `iphlpsvc`): efeitos documentados. Não desligar Teredo/IPv6.
3. Não copiar listas "OK to disable" de Server/IoT para PC gamer (incluem Windows Audio e Xbox "should disable" só no Server).
4. Packs opacos: o winutil recuou de 191 para 5 serviços.
5. Updaters de navegador: segurança.
6. NVIDIA Display Container: só fórum.
7. `OriginalType` do winutil ≠ padrão do Win11 (SharedAccess diverge; WSearch "Automatic"; `wermgr` não existe).
8. **Mitos e custos:** Nagle via MSMQ (só MSMQ, placebo) · limpar Prefetch (auto-mantido; só atrasa launches) · working set trim / "RAM cleaners" (o SO já faz; purge de standby é contestado e anedótico) · **VBS/HVCI off ≈ −5 % (±1), pior caso −8,1 % Project Cars 3; 13900K + 4090 até 5 % / 2 % em 4K** — única alavanca real, custa segurança, opt-in · HPET off ("mal chega a 1 %") · agrupar svchost (MS: economia pequena de memória, perde isolamento) · "SysMain desgasta SSD" (sem evidência).

## D. Fontes
Galdino: canal https://www.youtube.com/@GaldinoTech · WlHcrldBJdY · QyxPKbtHIdU · QmlZLGznf2Y · 8D1skH_SHd8 · CLFTR6rJbT0 · KiOvuJc-63A · LZRrTbI4NZA · Instagram (só meta tags) · https://windowsgamer.com.br/ · TikTok 7401077845709032709, 7316956701318597894, 7405551041128811782 (só títulos).
winutil: https://raw.githubusercontent.com/ChrisTitusTech/winutil/main/config/tweaks.json · `87a5779f~1` · commit 87a5779f · PR #4466 · commit 87a0eb86.
Microsoft: https://learn.microsoft.com/en-us/windows-server/security/windows-services/security-guidelines-for-disabling-system-services-in-windows-server · per-user services · configure-windows-diagnostic-data · manage-connections-from-windows · waas-delivery-optimization · windows-error-reporting · svchost-service-refactoring · iot soft-real-time · Mark Moro (Win7 SSD) · Windows Desktop Search (e7) · Q&A reativar Xbox · Q&A Nagle sem MSMQ · KB MSMQ Nagle · Q&A Prefetch · SetProcessWorkingSetSize · EmptyWorkingSet · RAMMap.
Benchmarks: TechTeamGB AtlasOS (2024-01-12) · Tom's Hardware VBS/HVCI (2021-10-09, 2023-03-14) · valleyofdoom/PC-Tuning.
Não acessíveis: Black Viper (404), Elevenforum (403), TikTok, mídia do Instagram.
