# Driver de kernel para sensores — opções e decisão

**Última revisão:** 2026-10-16 · **v1 implementada** (`crates/core/src/sensors.rs`)

> Pesquisa web feita em 2026-10-15. Legenda: **[V]** verificado em fonte primária citada; **[C]** relato de comunidade/terceiros; **[?]** não verificado — confirmar antes de decidir. Nada foi executado na máquina.

## 0. Estado

- **v1 feita (2026-10-16):** `sensors::read()` tenta HWiNFO (`Global\HWiNFO_SENS_SM2`: assinatura `HWiS`, offsets do cabeçalho validados contra o tamanho real do mapeamento via `VirtualQuery`, strings ANSI de 128/16 bytes, parser testado com blob sintético) e depois LibreHardwareMonitor (WMI `root\LibreHardwareMonitor`, `SELECT Name, SensorType, Value, Parent FROM Sensor`). Escolhe a temperatura da CPU (package → Tctl/Tdie → core max → média), devolve até 80 leituras de temperatura/ventoinha/potência/tensão. Nada é instalado; sem ferramenta aberta → `None` e a UI explica. Usado pelo monitor (a cada 5 s), pelo conselheiro (`cpu_hot`), pelo contexto da IA e pela tool MCP `get_sensors`.
- Lembrete de licença: o HWiNFO gratuito é para uso pessoal; a integração só lê o que o usuário já roda e não redistribui nada.
- **v2 (PawnIO opt-in)** e **v3 (driver próprio)** continuam como abaixo; pré-requisito da v2 = resposta por escrito do autor sobre uso em app fechado.

## 1. Problema

Faltam no modo usuário: temperatura de núcleo da CPU (MSR `IA32_THERM_STATUS` / SMN AMD), VID/multiplicador/bus, tensões e RPM de fan (Super I/O via LPC), SPD/XMP e timings reais da RAM (SMBus), registradores de chipset. CPU-Z/Speccy usam o `cpuz.sys`. Ambiente-alvo: HVCI ligado, blocklist de drivers vulneráveis ligada, Secure Boot, Kaspersky. Regras do projeto: menor privilégio, sem spawn de shell, sem truque de auto-elevação, todo caller hostil.

## 2. WinRing0 — nunca embarcar

- Microsoft classifica o driver como vulnerabilidade conhecida (CVE-2020-14979) e diz que a detecção do Defender (`VulnerableDriver:WinNT/Winring0`) é válida **[V]** [Microsoft Support](https://support.microsoft.com/en-us/windows/microsoft-defender-antivirus-alert-vulnerabledriver-winnt-winring0-eb057830-d77b-41a2-9a34-015a5d203c42). O rótulo `HackTool:Win32/Winring0` aparece em blogs/fóruns, sem página oficial **[C]**.
- Causa: IOCTLs de leitura/escrita arbitrária de MSR, porta de I/O e memória física acessíveis a usuário sem privilégio => escalada de privilégio **[V]** (NVD/MS).
- Com HVCI/blocklist ligados, o Windows bloqueia o carregamento; HWiNFO, LHM, MSI Afterburner, OpenRGB etc. migraram **[C]**. Ainda: o certificado de assinatura do WinRing0 teria sido revogado em 2024 **[C]**.
- Conclusão: shipar = AV flag + não carrega + passivo de segurança. Descartado.

## 3. PawnIO (namazso)

| Item | Achado |
|---|---|
| O que é | Driver de kernel genérico e **scriptável**: roda módulos em linguagem Pawn (interpretador PawnPP) dentro do kernel; o software usa o driver via IOCTL carregando módulos **[V]** [pawnio.eu](https://pawnio.eu/) |
| Restrição de acesso | Segurança vem do módulo, não do driver: cada módulo expõe só IOCTLs próprios. Ex. `IntelMSR.p` tem allowlist de **leitura de 35 MSRs** (THERM_STATUS, PACKAGE_THERM_STATUS, TEMPERATURE_TARGET, PERF_STATUS, APERF/MPERF, RAPL...) e **escrita de 6 MSRs** (OC_MAILBOX, PKG_POWER_LIMIT, MISC_FEATURE_CONTROL...) **[V]** [IntelMSR.p](https://github.com/namazso/PawnIO.Modules/blob/main/IntelMSR.p). Atenção: a escrita existe no módulo oficial; nós não precisamos carregar/expor isso se usarmos módulo próprio/filtrado. |
| Módulos oficiais | AMDFamily0F/10/17, RyzenSMU, IntelMSR, IntelMCHBAR, IntelPCHThermal, IntelOOBMSM, LpcIO (Super I/O), LpcACPIEC, IsaBridgeEC, SmbusI801, SmbusPIIX4, SmbusNCT6793, SmbusIntelSkylakeIMC, Nvidia, DellSMM, ZhaoxinMSR, ARMMSR, PcSpeaker, LedsValve etc. (25) **[V]** [PawnIO.Modules](https://github.com/namazso/PawnIO.Modules) |
| Assinatura | Edição "official" do instalador é "idêntica ao código aberto, mas assinada digitalmente"; existe edição "unrestricted" sem checagem de assinatura de módulo **[V]** pawnio.eu. Quem assina (Microsoft attestation/WHCP) **[?]** não declarado nas páginas lidas. |
| HVCI / blocklist | Vários projetos afirmam funcionar com Memory Integrity + Secure Boot e não estar na blocklist **[C]**; LHM 0.9.6 migrou para PawnIO por isso. Não encontrei declaração oficial MS **[?]**. |
| Licença | Driver: **GPL-2.0** com exceção que permite combinar com programas LGPL e módulos que falam com o driver **apenas via IOCTL**; programas que usam a "interface Pawn" devem ser compatíveis com a licença. Módulos: **LGPL-2.1**. Licenciamento alternativo sob contato (admin@namazso.eu) **[V]** [PawnIO](https://github.com/namazso/PawnIO), [Modules](https://github.com/namazso/PawnIO.Modules). Falar IOCTL com o driver de um app fechado parece permitido pela exceção, mas **[?]** — pedir confirmação por escrito/licença alternativa antes de redistribuir. |
| Instalação | Instalador separado (setup .exe) em pawnio.eu; escolha official/unrestricted. Instalação silenciosa/encadeável **[?]** (não documentada nas páginas lidas). LHM 0.9.6 **não** embute o driver; usuário instala à parte **[C]**. |
| API user-mode | `PawnIOLib` (wrapper + `PawnIOUtil`) vive no repo principal (repo separado arquivado em 2026-02-12) **[V]** [PawnIOLib](https://github.com/namazso/PawnIOLib). Assinaturas exatas **[?]** não lidas; é DLL C, viável via FFI em Rust, ou abrir o device e enviar IOCTL direto (a exceção de licença cita IOCTL). |
| Maturidade | Adotado por LibreHardwareMonitor e FanControl **[C]**; 423 estrelas, 104 commits **[V]**. Mantenedor único (risco de bus factor). AV: sem relato de detecção encontrado **[?]**; Kaspersky não testado. |
| Admin | Abrir o device/carregar módulos exige admin **[?]** (LHM exige admin para sensores MSR **[V]** [LHM](https://github.com/LibreHardwareMonitor/LibreHardwareMonitor)). |

Risco residual do PawnIO: superfície genérica no kernel (interpretador); um módulo permissivo ou "unrestricted" vira primitiva de escrita arbitrária. Mitigar: só módulos oficiais assinados, só os de leitura que usamos, nunca instalar edição unrestricted, tratar presença do driver como input hostil.

## 4. Driver próprio (KMDF)

**Assinatura** **[V]** [signing offerings](https://learn.microsoft.com/en-us/windows-hardware/drivers/dashboard/driver-signing-offerings), [attestation](https://learn.microsoft.com/en-us/windows-hardware/drivers/dashboard/code-signing-attestation):

- Precisa: Hardware Developer Program (Partner Center) + certificado **EV** + CAB assinado + submissão. Fluxo de dias, não semanas (tempo exato **[?]**). Custo EV típico algumas centenas de USD/ano **[?]**.
- Attestation: só Windows 10 Desktop e posteriores; **a documentação enquadra como "para testes"** e diz que não publica no Windows Update retail; não é "Windows Certified". Na prática carrega em Windows 10/11 retail com Secure Boot **[C]/[?]** (validar com VM Win11 limpa antes de investir).
- **Windows Server >= 2016 não carrega driver attestation**; só HLK-passed. Para servidor ou distribuição via Windows Update: WHCP/HLK (kit de testes, mais caro e lento).
- HVCI: sem páginas RWX, pool NX (`ExInitializeDriverRuntime(DrvRtPoolNxOptIn)` / `POOL_NX_OPTIN`), sem seções W+X, compatível com `/INTEGRITYCHECK`; validar com Driver Verifier + HVCI Readiness. **[?]** detalhes não verificados aqui.

**O que leva à blocklist** **[V]** [MS block rules](https://learn.microsoft.com/en-us/windows/security/application-security/application-control/app-control-for-business/design/microsoft-recommended-driver-block-rules): vulnerabilidades exploráveis para escalar privilégio, malware, ou comportamento "não malicioso mas que contorna o modelo de segurança" (ex.: R/W arbitrário de MSR/IO/memória física). Blocklist atualizada trimestralmente; enforçada com HVCI ligado. Reporte/revisão pelo MS Security Intelligence.

**Design seguro obrigatório (se fizermos o nosso):**

1. Device object com SDDL admin/SYSTEM-only (`D:P(A;;GA;;;SY)(A;;GA;;;BA)`), `FILE_DEVICE_SECURE_OPEN`, `IoCreateDeviceSecure`; sem acesso por usuário comum.
2. Somente leitura. **Zero** `wrmsr`, zero escrita de porta, zero mapeamento de memória física, zero `MmMapIoSpace` exposto.
3. MSR allowlist fixa no binário: `IA32_THERM_STATUS` (0x19C), `IA32_PACKAGE_THERM_STATUS` (0x1B1), `IA32_TEMPERATURE_TARGET` (0x1A2, TjMax), `IA32_PERF_STATUS` (0x198), `MSR_PLATFORM_INFO` (0xCE), APERF/MPERF; AMD: apenas leitura de SMN de temperatura (CCD temp / `F8` thermal) via offsets fixos. Nada de índice vindo do caller sem checar.
4. SMBus: somente leitura, endereços 0x50–0x57 (SPD) e comandos de leitura de byte/bloco; sem escrita (SPD write-protect/ bricking de RAM). Super I/O: apenas portas 0x2E/0x4E e 0x290-range de chips conhecidos, leitura de registradores de sensor via lista fixa; escrita de bank-select estritamente limitada, serializada e restaurada. (Super I/O por LPC tem estado compartilhado com ACPI/EC: usar mutex ACPI `\_SB.PCI0.LPCB` quando existir **[?]**.)
5. IOCTL `METHOD_BUFFERED` apenas (nunca `METHOD_NEITHER`), validar tamanhos, `ProbeFor*` desnecessário em buffered; saídas de tamanho fixo.
6. Um handle por vez (exclusivo), checar que o caller é o processo assinado do G2 Boost (verificar imagem/assinatura do cliente via `PsGetProcessImageFileName` + caminho de instalação protegido por ACL; defesa em profundidade, não única barreira), rate limit (ex.: <= 100 IOCTL/s).
7. Serviço sob demanda: carregar ao abrir a tela de sensores, descarregar ao sair (modelo cpuz.sys); nunca auto-start no boot.
8. Código aberto para auditoria, fuzzing (`IOCTLance`, Driver Verifier), revisão externa antes do release; canal para reportar vulnerabilidade; atualização rápida se o MS pedir.
9. Pipeline de compilação reprodutível; chave EV em HSM/token; sem commit de segredo.

**Toolchain** **[V]** [windows-drivers-rs](https://github.com/microsoft/windows-drivers-rs): WDK exige Visual Studio/Build Tools + WDK (ou eWDK, ~vários GB) — hoje o dev usa plano B sem VS. Rust em driver: `windows-drivers-rs` diz "early stages, **not yet recommended for production**", só testado com KMDF 1.33 / eWDK. Logo, driver em **C com KMDF** (superfície mínima, ~500–1000 linhas), cliente em Rust. Requer instalar VS Build Tools + eWDK (UAC/admin, C: por default) **[?]** tamanho exato.

## 5. Alternativas sem driver próprio

| Fonte | Licença / fit | Observações |
|---|---|---|
| **HWiNFO shared memory** | Grátis só uso pessoal/não comercial; uso comercial exige HWiNFO Pro (Engineer/Corporate); versão não-Pro desliga Shared Memory a cada 12 h **[V]** [licenças](https://www.hwinfo.com/licenses/). | Só ler quando o **usuário** já tem HWiNFO rodando. Ler a memória compartilhada não nos redistribui nada, mas não podemos exigir/embutir. Termos de integração **[?]**. |
| **LibreHardwareMonitor** | MPL-2.0 (lib e app) **[V]**; usa PawnIO para MSR/SMBus/LPC **[C]**; sensores precisam admin **[V]**. | MPL-2.0 permite uso em app fechado (arquivos MPL modificados continuam abertos). Biblioteca .NET: arrastaria runtime .NET; alternativa é ler o servidor web/WMI se o usuário já roda LHM **[?]**. |
| **AMD Ryzen Master Monitoring SDK** | SDK público, leitura apenas **[V]** [AMD](https://www.amd.com/en/developer/ryzen-master-monitoring-sdk.html). Redistribuição **[?]** (ver licença no SDK). | Usa `AMDRyzenMasterDriver.sys` (driver próprio da AMD, auto-instalado) **[C]**; já teve CVE-2024-21945 (permissões incorretas), corrigido na 2.13.0.2915 **[V]** [AMD SB-9004](https://www.amd.com/en/resources/product-security/bulletin/amd-sb-9004.html). Só AMD. Dependência de driver de terceiro assinado e bem-vindo ao ecossistema AMD. |
| Intel (XTU/DTT, Intel Power Gadget descontinuado) | **[?]** | Sem SDK de monitoramento público equivalente confirmado. |
| GPU: NVML, ADLX/ADL, Intel IGCL | Modo usuário | Já usamos NVML; ADLX/IGCL para AMD/Intel GPU. Não resolvem CPU. |
| WMI `MSAcpi_ThermalZoneTemperature` | Nativo, admin | Raramente reflete a CPU (relatos de ~28 C vs 76 C reais **[C]** [widbar#4](https://github.com/andelby/widbar/issues/4)). Não confiável. |
| API de sensores nativa do Windows (Sensor class / HLK) | **[?]** | Não encontrei API de temperatura de CPU genérica no Windows 11; Sensor APIs são para sensores de ambiente/movimento. Não contar com isso. |
| Contadores PDH "Thermal Zone Information"/"Processor Information" | Modo usuário | Já usamos PDH para frequência; a "thermal zone" tem o mesmo problema do ACPI. |

## 6. Ranking

Critérios (5 = melhor): segurança, licença p/ app fechado, esforço (5 = pouco), custo, risco AV/SmartScreen.

| # | Opção | Segurança | Licença | Esforço | Custo | AV/SS | Cobre |
|---|---|---|---|---|---|---|---|
| 1 | Ler HWiNFO shared memory / LHM se **já instalados** (opt-in) | 5 | 3 (HWiNFO comercial = Pro) | 5 | 5 | 5 | Tudo que a ferramenta expõe |
| 2 | **PawnIO** + módulos oficiais de leitura, com consentimento + admin | 4 | 3 ([?] GPL/IOCTL) | 4 | 5 | 4 | CPU temp, SMBus/SPD, Super I/O |
| 3 | AMD Ryzen Master SDK | 3 | 2 [?] | 3 | 5 | 3 | Só AMD CPU |
| 4 | Driver próprio minimalista, attestation | 3 (depende 100% do nosso código) | 5 | 1 | 2 (EV + VS/WDK + auditoria) | 2 (reputação nova, Kaspersky) | Sob medida |
| 5 | WinRing0 | 0 | — | — | — | 0 | **Proibido** |

## 7. Decisão e plano em fases

**Decisão:** não escrever driver agora; começar sem driver, depois PawnIO opt-in; driver próprio só se PawnIO falhar em licença/AV/estabilidade.

- **v1 (sem driver, baixo risco):** detectar HWiNFO (shared memory) e LHM/WMI já em execução; se existirem, mostrar temp/fan/volts/SPD. Texto na UI: "não instalamos nada". Sem HWiNFO Pro avisar do limite de 12 h. Nenhuma dependência nova no instalador. GPU via NVML/ADLX/IGCL (já). Mostrar "indisponível sem driver" para o resto.
- **v2 (PawnIO opt-in):**
  1. Antes: pedir ao namazso licença por escrito (uso de app fechado, redistribuir/encadear instalador, módulos LGPL via IOCTL) e confirmar assinatura/HVCI **[?]**.
  2. Testar na máquina de dev (HVCI + blocklist + Kaspersky) o instalador oficial; registrar detecções.
  3. Fluxo: tela de consentimento explicando o que o driver lê; sem instalar sozinho (usuário baixa/roda o instalador oficial ou o encadeamos só com aceite explícito); processo helper elevado separado, mínimo, só leitura, que fala IOCTL/PawnIOLib e devolve valores à UI por IPC local autenticado (sem elevar a UI inteira, sem `powershell`).
  4. Carregar apenas módulos de leitura necessários (IntelMSR/AMDFamily17/RyzenSMU leitura, SmbusI801/PIIX4 leitura, LpcIO leitura); nunca chamar IOCTL de escrita; nunca edição "unrestricted".
  5. Testes de segurança: helper rejeita qualquer comando fora da lista, tolera driver ausente, tratar saída do módulo como não confiável.
- **v3 (driver próprio, só se necessário):** C/KMDF minimalista seguindo o design da seção 4; EV + Partner Center attestation; teste em Win10/11 com HVCI, Secure Boot, blocklist ligada; fuzz + revisão externa; submeter a amostra ao Kaspersky/MS antes do release; sem suporte a Windows Server (attestation não carrega lá).

## 8. Pendências a verificar

1. Termos de redistribuição do PawnIO para app fechado (GPL-2.0 + exceção) **[?]**.
2. Quem assina o PawnIO (attestation vs WHCP) e se está fora da blocklist **[?]**.
3. Assinaturas de `PawnIOLib` e instalação silenciosa **[?]**.
4. Licença do AMD Ryzen Master Monitoring SDK **[?]**.
5. Attestation-signed carrega em Win11 retail com HVCI (doc oficial diz "testing") — validar com submissão de teste **[?]**.
6. Termos comerciais do HWiNFO para integração via shared memory **[?]**.
7. Comportamento do Kaspersky 21.26 com PawnIO **[?]**.
