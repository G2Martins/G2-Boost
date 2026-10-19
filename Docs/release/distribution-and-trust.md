# Distribuição, assinatura e confiança do Windows

**Última revisão:** 2026-10-18 (§6.1 autor pessoa física, Store individual grátis, Artifact Signing fora do Brasil; §8 = estado real do repositório)

> Pesquisa web (não testada na prática). Legenda: **[V]** = confirmado em doc oficial da Microsoft/Tauri; **[C]** = fonte comunitária/revendedor; **[?]** = não verificado, confirmar antes de decidir. Preços em USD, de revendedores, variam.

## 0. Resumo executivo

1. **Nenhum certificado dá "zero aviso" no dia 1.** Desde 2024 EV = OV para SmartScreen; reputação se acumula por hash do arquivo + reputação do publisher (certificado) [V]. Assinar é obrigatório, mas o aviso some com **tempo + downloads limpos** (semanas, centenas de instalações, sem limiar exato) [V].
2. **Azure Artifact Signing (ex-Trusted Signing) provavelmente NÃO está disponível para empresa brasileira**: organizações só nos EUA, Canadá, UE e Reino Unido [V]. "Brazil South" aparece só como região de deploy do recurso, não como elegibilidade [V/C]. Confirmar com a Microsoft; plano realista = **certificado OV de CA tradicional**.
3. **Smart App Control (SAC) bloqueia binário não assinado** sem reputação positiva, inclusive DLLs/uninstaller; assinatura precisa encadear a CA do Microsoft Trusted Root Program [V]. Assinar tudo é pré-requisito para rodar com SAC ligado.
4. **Microsoft Store (MSIX) é o único caminho sem aviso algum** (Microsoft re-assina) [V], mas a política para "otimizadores" não foi encontrada; risco de reprovação alto [?] (ver §3).
5. Plano: OV + assinatura em CI + GitHub Releases + winget + submissões de falso positivo + evitar heurísticas ruins.

## 1. Assinatura de código e reputação SmartScreen

### 1.1 Opções

| Opção | Custo | Disponibilidade p/ Brasil | SmartScreen | Notas |
|---|---|---|---|---|
| Azure Artifact Signing (ex-Trusted Signing) | Basic US$ 9,99/mês (5 mil assinaturas); Premium US$ 99,99/mês [V/C] | **Org.: EUA, CA, UE, UK. Indivíduo: EUA, CA** [V]. Brasil: não listado [?] | Reputação cresce com o tempo; sem bypass [V] | Certs de vida curta (~3 dias) renovados automaticamente [C]; sem token; ótimo p/ CI. Validação de identidade: dias úteis [V] |
| OV (Sectigo, DigiCert, SSL.com, GlobalSign) | ~US$ 130-550/ano conforme revendedor; cloud-signing extra (SSL.com eSigner +US$ 249; DigiCert KeyLocker ~US$ 44/mês) [C] | Mundial [V] | Igual ao Azure [V] | Chave em HSM/token obrigatória (CA/B Forum, jun/2023) [V]; validade máx. 1 ano desde 15/02/2026 [C] |
| EV | US$ 400+/ano [V] | Mundial | **Igual ao OV desde 2024** (OIDs EV removidos do Trusted Root Program) [V] | Só vale para driver kernel / compras corporativas. Não pagar por SmartScreen [V] |
| SignPath Foundation | Grátis | Só open source [V] | Nível OV | Só se o repo virar open source [?] |
| Autoassinado / nenhum | Grátis | - | Bloqueio forte [V] | Nunca p/ público |

### 1.2 Pontos práticos

- **Chave em HSM/token:** nenhum PFX exportável novo. Para CI precisa de **cloud HSM** (SSL.com eSigner, DigiCert KeyLocker, Azure Key Vault Managed HSM) ou token USB local. O guia PFX/GitHub Actions do Tauri vale só para certs emitidos antes de 01/06/2023 [V].
- **Identidade:** CA valida empresa (razão social, CNPJ, endereço, telefone verificável, às vezes D-U-N-S ou carta de advogado/contador) [?]. O nome do certificado deve ser **Grovic Data** com a razão social exata; manter igual em todas as releases (identidade consistente carrega reputação) [V].
- **Certificados ICP-Brasil (e-CNPJ) não servem** para Authenticode público [?] (não encontrei fonte; confirmar com a CA).
- **Reputação:** assinar toda release com o mesmo certificado; não alterar arquivo após assinar; não assinar nada com comportamento PUA, senão o cert ganha reputação **negativa** [V]. Não existe submissão manual de reputação para consumidor [V]; admins empresariais podem submeter via portal WDSI [V].
- **Smart App Control:** libera se a inteligência da Microsoft reconhece o app ou se assinado por CA do Trusted Root Program; certs confiados manualmente em certmgr/GPO não valem; vale para todo executável, não só baixados [V]. Logs: Event Viewer > CodeIntegrity > Operational, ID 3076 (avaliação) e 3077 (enforce) [C/V]. Assinar: .exe, .dll, instalador, uninstaller, sidecar.

### Fontes

- [Code signing options (MS Learn, 2026-08-29)](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/code-signing-options)
- [SmartScreen reputation (MS Learn)](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation)
- [Distribution feature status (EV change)](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/distribution-feature-status)
- [Artifact Signing quickstart (regiões)](https://learn.microsoft.com/en-us/azure/artifact-signing/quickstart) · [Q&A elegibilidade](https://learn.microsoft.com/en-us/answers/questions/6020935/artifact-signing)
- [Smart App Control para devs](https://learn.microsoft.com/en-us/windows/apps/develop/smart-app-control/overview) · [testar com SAC](https://learn.microsoft.com/en-us/windows/apps/develop/smart-app-control/test-your-app-with-smart-app-control)
- Preços de revendedores: [SSL.com OV](https://www.ssl.com/products/software-integrity/code-signing/ov/), [SignMyCode](https://signmycode.com/), [SSL Dragon](https://www.ssldragon.com/ssl-certificates/code-signing/)

## 2. Integração com Tauri 2 (Windows)

### 2.1 `tauri.conf.json`

Cert em token/cert store (thumbprint) [V — [doc Tauri](https://v2.tauri.app/distribute/sign/windows/)]:

```json
"bundle": { "windows": {
  "certificateThumbprint": "<SHA-1 do certmgr>",
  "digestAlgorithm": "sha256",
  "timestampUrl": "http://timestamp.digicert.com"
}}
```

Cloud/HSM via comando custom (`%1` = arquivo) [V]:

```json
"signCommand": "<ferramenta do provedor> ... %1"
```

Exemplos oficiais: Azure Key Vault `relic sign --file %1 --key azure --config relic.conf`; Artifact Signing `artifact-signing-cli -e <endpoint> -a <conta> -c <perfil> -d "G2 Boost" %1` (env `AZURE_CLIENT_ID/SECRET/TENANT_ID`). Para SSL.com eSigner / DigiCert KeyLocker, usar o CLI do provedor (`CodeSignTool` / `smctl`) no `signCommand` [?].

- **Timestamp é obrigatório na prática**: sem ele a assinatura morre quando o cert expira. Usar o URL RFC 3161 da CA.
- `signCommand` precisa **falhar com exit != 0** se não assinar. Houve casos de sidecar saindo sem assinatura com exit 0 [C].

### 2.2 Sidecar e uninstaller

- Tauri assina uninstaller NSIS e sidecars (`externalBin`) via o mesmo hook desde tauri-bundler 1.4.0 [C/V]. Caminho do sidecar chega **relativo a `src-tauri`** — cuidado com `cd` dentro do script [C].
- **Bugs abertos:** [#16208](https://github.com/tauri-apps/tauri/issues/16208) (DLLs de plugin NSIS — `System.dll`, `nsDialogs.dll`, etc. — ficam sem assinatura mesmo com `signCommand`, CLI 2.11.2) [C]; [#7348](https://github.com/tauri-apps/tauri/issues/7348) (uninstaller, histórico). Com SAC isso pode importar. **Verificar o instalador pronto**: extrair com 7-Zip e rodar `Get-AuthenticodeSignature` em cada PE (exige `Valid` + `TimeStamperCertificate` não nulo). Se DLLs NSIS ficarem sem assinatura, assinar pós-build ou aceitar (são do NSIS, hash conhecido) e testar com SAC.
- Assinar também o crate/exe `*-mcp.exe` se for distribuído.
- Também assinar o instalador final (`*-setup.exe`) depois de gerado.

### 2.3 Updater (`tauri-plugin-updater`)

[V — [doc](https://v2.tauri.app/plugin/updater/)]

- Chaves **minisign** próprias do Tauri (`bun tauri signer generate`), independentes do Authenticode. `pubkey` no `tauri.conf.json`; `TAURI_SIGNING_PRIVATE_KEY` (+ `_PASSWORD`) como variável de ambiente (arquivo `.env` não funciona). **Perder a chave privada = não dá mais para atualizar quem já instalou.** Guardar backup offline e secret no GitHub.
- `bundle.createUpdaterArtifacts: true` gera instalador + `.sig`.
- Manifesto `latest.json` hospedado em GitHub Releases: endpoint `https://github.com/<org>/<repo>/releases/latest/download/latest.json`; `tauri-action` com `includeUpdaterJson: true` anexa o arquivo [C]. Repo precisa ser **público** (updater baixa sem credencial) e release não pode ser draft/pre-release [C]. Se o repo for privado, hospedar em outro lugar [?].
- Updater instala o NSIS silencioso: o instalador baixado também precisa estar **assinado Authenticode**, senão SAC/SmartScreen podem barrar.

## 3. Canais de distribuição

### 3.1 Microsoft Store

- MSIX: Microsoft re-assina, sem aviso de SmartScreen, sem custo de certificado, mundial [V]. Win32 MSI/EXE na Store: **você** assina com CA do Trusted Root Program; sem SmartScreen na instalação pela Store (UAC pode aparecer) [V].
- Conta de desenvolvedor: [storedeveloper.microsoft.com](https://storedeveloper.microsoft.com) [V]; taxa de registro empresa [?] (tipicamente ~US$ 99 única, confirmar).
- **Política para otimizadores/cleaners:** li as [Store Policies 7.20](https://learn.microsoft.com/en-us/windows/apps/publish/store-policies) (2026-09-14): **não achei** cláusula específica sobre "PC optimizer/registry cleaner". Há: 10.2 (não malware/PUA; Unwanted and Malicious Software criteria), desinstalação limpa, 10.5 privacidade, e declarações de capacidade devem se relacionar à função do produto (incl. `runFullTrust`/admin) [V]. Fora da política, a Microsoft **não suporta** registry cleaners [V/C] e a categoria é historicamente scrutinada [?]. Risco de reprovação por claims de "turbo/otimização" — **evitar linguagem de marketing de "acelerar PC"**. Não verificado se há rejeições recentes.
- Viabilidade técnica: app que escreve HKLM, mexe em serviços/plano de energia e exige elevação em MSIX full-trust é mais complexo (virtualização de registro do MSIX, elevação) [?]. Deixar a Store para pós-v1.

### 3.2 winget (microsoft/winget-pkgs)

- PR com manifestos YAML (`wingetcreate new <url>` gera) [V — [doc](https://learn.microsoft.com/en-us/windows/package-manager/package/repository)]. Tipos aceitos: MSIX, MSI, APPX, EXE; scripts não [C]. NSIS = `InstallerType: nullsoft`, `/S` silencioso.
- Exige URL pública estável, **SHA256 do instalador** (`winget hash`), instalação silenciosa funcionando, validar com `winget validate` e testar local (Sandbox) [V].
- Pipeline de validação faz scan de malware + SmartScreen na URL; reprovação por reputação ruim é comum em apps novos [C]. Instalador não assinado é aceito (anedotas), mas mais sujeito a flag [C/?].
- **Hash muda a cada release**: automatizar com `wingetcreate update` / action `vedantmgoyal9/winget-releaser` em CI [?].
- Winget serve o mesmo `.exe` do GitHub; o usuário final não vê SmartScreen se `winget` baixa (a Mark-of-the-Web não é aplicada em geral) [?].

## 4. Reduzir falso positivo de antivírus

### 4.1 Submissões

| Vendor | Onde | Notas |
|---|---|---|
| Microsoft Defender | [WDSI file submission](https://www.microsoft.com/en-us/wdsi/filesubmission) → "Software developer - false positive"; ou portal Defender > Submissions | Anexar arquivo, info do cert, site da empresa, descrição. Resultado dá contato para reinvestigação [C]. Requer conta Microsoft; programa de desenvolvedor (MSDAP) para acompanhar [?] |
| Kaspersky | [OpenTIP](https://opentip.kaspersky.com) → "Submit to reanalyze"; [support.kaspersky.com/1870](https://support.kaspersky.com/1870) | Não há form de allowlist de dev dedicado; relatos de form de suporte sem upload [C]. E-mail legado `newvirus@kaspersky.com` [C/?] |
| Avast / AVG / Norton (Gen Digital) | [Avast submit](https://support.avast.com/en-us/article/submit-file-url/) (form de falso positivo); [file whitelisting](https://support.avast.com/en-ww/article/threat-lab-file-whitelist/) para devs | Avast e AVG compartilham engine. Norton: [?] (form de "submit file" da Gen; não verifiquei) |
| Bitdefender | [Report incorrect detection](https://www.bitdefender.com/consumer/support/answer/29358/) | Correção "em horas" segundo o vendor [C] |
| ESET | [KB141](https://support.eset.com/en/kb141-submit-a-virus-website-or-potential-false-positive-sample-to-the-eset-lab) | Zip com senha `infected` para `samples@eset.com`; oferece whitelisting para vendors [C] |

Rotina: depois de assinar o build final, subir ao **VirusTotal** (pré-scan) e ao OpenTIP; submeter FP aos vendors que acusarem **antes** de anunciar. Submeter de novo a cada release se necessário (reputação por hash). Para Kaspersky aqui (já flagou os .bat), manter uma VM com Kaspersky como ambiente de teste.

### 4.2 Comportamentos que disparam heurística (evitar/mitigar)

| Evitar | Situação do G2 Boost |
|---|---|
| Packers (UPX, Themida), binário ofuscado | Não usar. `strip`/`lto` ok |
| Auto-elevação sorrateira / `powershell Start-Process -Verb RunAs` | Já usa `ShellExecuteExW("runas")` nativo, visível no UAC. Manter |
| Spawn de `cmd`/`powershell`/`reg`/`sc`/`net` | Regra do projeto já proíbe. Revisar com `strings`/Process Monitor |
| Escrever chave `Run` | Usa tarefa agendada de logon; também é sinal de persistência. Opt-in explícito, com UI clara, e remover no uninstall |
| Baixar e executar executáveis | **Updater**: só baixa instalador assinado (minisign + Authenticode), de URL fixa. Não baixar outro EXE em runtime |
| Limpeza de Prefetch/logs/eventos, `net stop` de serviços de segurança/Update, tocar em política do Defender | Já fora do escopo. Manter fora |
| Purge da standby list via `NtSetSystemInformation`, acesso a processos de outros usuários | Chamada NT nativa é incomum; documentar; pode atrair heurística [?] |
| IOCTLs de disco/WMI em massa, enumerar processos | Normal em "system info", baixo risco |

Boas práticas: recurso **VERSIONINFO** completo (CompanyName "Grovic Data", ProductName, FileVersion, LegalCopyright, OriginalFilename) e ícone em todos os exes [?]; manifesto `asInvoker` (menor privilégio) já alinhado; identidade do publisher idêntica em cert, installer, VersionInfo, site e GitHub; releases reproduzíveis; instalador per-user NSIS sem pedir admin; desinstalador limpo; site com política de privacidade e página do produto; **sem telemetria por padrão**; build com Rust estável sem crates suspeitas; não mudar assinatura entre releases.

## 5. Testar em máquina limpa

| Ferramenta | Requisitos / notas |
|---|---|
| Windows Sandbox | Pro/Enterprise/Education, **não Home**; virtualização habilitada; descartável, estado zerado ao fechar; rede ligada por padrão; 1 instância por vez; arquivo `.wsb` para mapear pasta read-only [V — [doc](https://learn.microsoft.com/en-us/windows/security/application-security/application-isolation/windows-sandbox/windows-sandbox-overview)]. Já traz Defender; **não** reproduz Kaspersky/SAC fielmente [?] |
| VM Windows 11 (Hyper-V/VirtualBox) | ISO de avaliação do Microsoft Evaluation Center (90 dias) [?] ou Dev VM; tirar snapshot "limpo" e reverter a cada teste. Instalar Kaspersky/Avast/Bitdefender/Norton (trials) em VMs separadas |
| Smart App Control | Só ativo em instalação **nova** do Windows 11 em modo avaliação (aprende e liga/desliga sozinho); em 2026 há notícias de poder alternar sem reinstalar [C — [ciaops](https://blog.ciaops.com/2026/04/16/existing-systems-can-now-enable-windows-smart-app-control-and-you-should/)]; confirmar no seu build. Modo evaluation só **loga** (Event 3076); enforcement bloqueia (3077). Testar: instalador, app, sidecar, uninstaller, updater [V] |
| SmartScreen | Baixar o `.exe` pelo navegador (aplica Mark-of-the-Web) num Windows com SmartScreen "Verificar aplicativos" ligado; testar também via `winget install` |

Checklist de teste: instalar per-user sem admin; abrir; elevar; aplicar/reverter tweak; atualizar via updater; desinstalar; checar sobras no registro/tarefas.

## 6. Checklist de release (de hoje até v1.0)

| # | Passo | Dono | Prazo / custo |
|---|---|---|---|
| 0 | **Decidir** (ver §7): entidade publicadora, orçamento, CA, repo público ou não | Dono | 1 dia |
| 1 | Confirmar com Microsoft se Artifact Signing aceita CNPJ brasileiro (tentar abrir conta/validação); se não → comprar OV | Dono | 1-3 dias; US$ 0-10 |
| 2 | Comprar OV + cloud HSM/token (SSL.com, DigiCert ou Sectigo via revendedor); concluir validação de identidade (docs da Grovic Data: CNPJ, contrato social, telefone/endereço verificáveis) | Dono + dev | ~3-15 dias úteis; ~US$ 130-550/ano + cloud signing ~US$ 250/ano (ou token USB) |
| 3 | Gerar chaves do updater (`tauri signer generate`), backup offline; `pubkey` no config; `createUpdaterArtifacts: true` | Dev | 1 h |
| 4 | Recurso VERSIONINFO + ícone em app, sidecar, mcp; revisar manifestos (`asInvoker`) | Dev | 0,5 dia |
| 5 | `signCommand` + `timestampUrl` no `tauri.conf.json`; teste local assinando um build | Dev | 0,5-1 dia |
| 6 | Workflow GitHub Actions (`windows-latest`): `tauri-action`, secrets (updater key, credenciais do HSM), `includeUpdaterJson`, release em **draft** | Dev | 1 dia |
| 7 | Script de verificação pós-build: extrair NSIS (7-Zip) e `Get-AuthenticodeSignature` em todo PE; falhar se não `Valid`/sem timestamp | Dev | 0,5 dia |
| 8 | Auditar heurísticas (§4.2): strings de `cmd`/`powershell`, Run keys, updater | Dev | 0,5 dia |
| 9 | VirusTotal + OpenTIP no build assinado; submeter FP (Defender WDSI primeiro, depois Kaspersky/Avast/Bitdefender/ESET/Norton) | Dev | 1-3 dias de espera |
| 10 | Testes: Sandbox, VM limpa, VM com cada AV, Windows 11 com SAC (evaluation), SmartScreen via navegador | Dev | 1-2 dias |
| 11 | Site/README: página do produto, privacidade, "sem telemetria", changelog, hashes SHA256; orientação ao usuário sobre possível aviso inicial | Dono + dev | 1 dia |
| 12 | Publicar **v0.9 beta pública** (pre-release) com assinatura; juntar instalações limpas para reputação (semanas) | Dev | 2-6 semanas (reputação) |
| 13 | PR no winget-pkgs (`wingetcreate`), depois automatizar `wingetcreate update` por release | Dev | 1-2 dias + revisão da moderação (dias) [?] |
| 14 | **v1.0** em GitHub Releases (não-draft) + winget + updater ativo | Dev | - |
| 15 | Pós-v1: reavaliar Microsoft Store (MSIX/Win32), Artifact Signing se Brasil for liberado | Dono | opcional |

**Linha do tempo realista:** 2-4 semanas até primeiro build assinado e distribuível; **reputação SmartScreen limpa leva semanas após** (sem prazo garantido) [V]. **Custo ano 1:** ~US$ 400-800 (OV + HSM) + 0 de infra (GitHub). Store: possível taxa de conta [?].

> **Fora do roadmap (decisão do dono, 2026-10-18):** "baixar sem o aviso do Windows" (certificado, Store, SmartScreen) não será feito agora. Esta doc fica como referência.

## 6.1 Revisão 2026-10-18: autor pessoa física

**Decisão do dono (2026-10-18):** o autor do software é **Gustavo Martins Gripaldi** (`bundle.publisher`, `bundle.copyright`, `authors` do Cargo, README, SECURITY). O `identifier` `com.grovicdata.g2boost` fica (mudar apagaria os dados locais do WebView).

Fatos revisados (pesquisa de 2026-10-18):

- **SmartScreen** [V] [Microsoft, 2026-08](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation): reputação = assinatura do editor + hash; EV **não** dá reputação instantânea; OV/EV avisam até acumular (semanas e centenas de instalações limpas). Só a Store não tem aviso. Trocar de certificado enfraquece o sinal.
- **Artifact Signing** (ex-Trusted Signing, US$ 9,99/mês): pessoa física **só EUA e Canadá**; organizações numa lista de países **sem o Brasil** [V] [quickstart, 2026-10-08](https://learn.microsoft.com/en-us/azure/artifact-signing/quickstart). Fora para o dono, com ou sem CNPJ.
- **OV para pessoa física:** Certum (SimplySign em nuvem, ~€ 170–190/ano), SSL.com (IV/sole proprietor), Sectigo por revenda (~US$ 210/ano) [?]. Chave em token/HSM obrigatória desde 06/2023.
- **Microsoft Store:** conta de desenvolvedor **individual gratuita** desde 2025-09-10 (verificação por documento + selfie) [V] [blog Windows](https://blogs.windows.com/windowsdeveloper/2025/09/10/free-developer-registration-for-individual-developers-on-microsoft-store/). Duas rotas: **EXE/MSI** (a Store baixa nosso instalador de uma URL https fixa; "recomenda fortemente" assinatura por CA confiável e exige declarar drivers/serviços) ou **MSIX** (a Store re-assina: sem aviso, e de quebra resolve a identidade de pacote do winget COM), mas capabilities restritas (`allowElevation`, escrita fora da virtualização) têm aprovação difícil para um otimizador [?].
- **Kaspersky Allowlist Program** pede endereço legal da empresa [V] [Kaspersky](https://www.kaspersky.com/partners/allowlist-program): com pessoa física pode haver atrito; o portal da Microsoft ([WDSI](https://www.microsoft.com/en-us/wdsi/filesubmission)) aceita qualquer um.

**Arquitetura recomendada para baixar sem susto:**

1. **Agora (grátis):** publicar na **Microsoft Store pela rota EXE** (conta individual gratuita): quem instala pela Store não passa pelo SmartScreen do navegador. Pré-requisito: instalador assinado (passo 2) e URL https estável (GitHub Releases).
2. **Certificado OV em nome de Gustavo Martins Gripaldi** (Certum SimplySign, sem token físico): `bundle.windows.signCommand` assina o exe, o instalador NSIS e o desinstalador; mesma identidade em todas as versões. Download direto do site ainda avisa nas primeiras semanas; some com reputação.
3. **Cada release:** enviar o instalador ao WDSI (Microsoft) e ao Kaspersky; winget (`winget install` não marca o arquivo como baixado da internet).
4. **Depois, se valer:** MEI/CNPJ (abre Allowlist da Kaspersky e Partner Center para o driver) e avaliar MSIX na Store.

## 7. O que o dono precisa decidir

1. **Publisher:** ~~Grovic Data (CNPJ)~~ → **Gustavo Martins Gripaldi** (pessoa física, decidido em 2026-10-18; ver §6.1). Certificado OV no mesmo nome exato.
2. **Orçamento:** OV ~US$ 130-550/ano + cloud signing, ou token USB (entregue por courier ao Brasil, atraso/aduana [?]).
3. **Repo público?** Facilita updater via GitHub e winget; repo privado exige hospedar `latest.json` e instalador em outro lugar.
4. **Aceitar aviso inicial** durante a fase de reputação (comunicar em README) ou investir em Store depois.
5. **Persistência/telemetria:** manter autostart e updater como opt-in explícito.
6. **Escopo antivírus:** ter VMs com Kaspersky/Avast/Bitdefender/Norton para QA a cada release.

## 8. Estado no repositório (2026-10-16)

| Item | Estado |
|---|---|
| Licença | `LICENSE` (proprietária, pt-BR prevalece + tradução en). `bundle.licenseFile` → página de aceite no instalador. **Revisar com advogado** e confirmar razão social/CNPJ e contato antes da v1. |
| Avisos de terceiros | `THIRD_PARTY_NOTICES.md` gerado por `python -I tools/third_party_notices.py` (246 crates + 68 pacotes npm, textos de licença deduplicados). Sem GPL/LGPL/AGPL; MPL-2.0 só em bibliotecas não modificadas (cssparser/selectors/lightningcss) e fontes OFL-1.1. Regenerar a cada release. Instalado junto (`bundle.resources`) com `LICENSE.txt`. |
| VERSIONINFO | `bundle.publisher` = Grovic Data, `bundle.copyright`, `productName`, versão do workspace. |
| MCP | **Sem sidecar:** `G2 Boost.exe --mcp` serve o MCP (crate `g2boost-mcp` como biblioteca). Um exe a assinar; `cargo test`/clippy não dependem de binário pré-compilado. |
| Instalador | NSIS `currentUser` (sem admin), pt-BR/en com seletor, WebView2 por bootstrapper (padrão do Tauri: baixa se faltar). Ferramentas NSIS do Tauri em `D:\dev\tauri` (junção a partir de `%LOCALAPPDATA%\tauri`, regra "downloads fora do C:"). |
| Assinatura | **Ainda não.** Snippets em §2.1; decidir CA (§7). |
| Updater | Não configurado (decidir hospedagem/repo público). |

### Instalar em outro PC hoje (build não assinado)

1. `bun run tauri:build` → `target\release\bundle\nsis\G2 Boost_<versão>_x64-setup.exe`.
2. Copie o `setup.exe` (pendrive/rede). No PC de destino: SmartScreen mostra "O Windows protegeu o computador" → **Mais informações → Executar assim mesmo** (esperado sem assinatura). Navegadores podem marcar o download como incomum.
3. **Smart App Control ligado bloqueia** binário sem assinatura e sem reputação — não há contorno seguro além de assinar (desligar o SAC não volta sem reinstalar o Windows; não recomendar).
4. Antivírus de terceiros podem pedir confirmação; se acusar, enviar como falso positivo (§4.1) antes de distribuir mais.
5. Teste limpo sem risco: **Windows Sandbox** (Pro/Enterprise; ativar o recurso opcional "Área Restrita do Windows" uma vez, exige admin) com `tools/sandbox-test.wsb` — mapeia a pasta do instalador só leitura; tudo some ao fechar.

## Fontes adicionais

- [Windows Code Signing (Tauri 2)](https://v2.tauri.app/distribute/sign/windows/) · [Updater (Tauri 2)](https://v2.tauri.app/plugin/updater/)
- [winget: enviar manifesto](https://learn.microsoft.com/en-us/windows/package-manager/package/repository) · [Troubleshoot de validação](https://github.com/microsoft/winget-pkgs-submission-test/blob/master/Troubleshoot.md)
- [EV não dá reputação instantânea (todesktop)](https://www.todesktop.com/blog/posts/windows-apps-psa-ev-certs-do-not-grant-immediate-reputation-anymore) · [DigiCert KB](https://knowledge.digicert.com/alerts/ev-signed-application-showing-microsoft-defender-smartscreen-warnings)
- [Microsoft: registry cleaners](https://support.microsoft.com/en-au/topic/microsoft-support-policy-for-the-use-of-registry-cleaning-utilities-0485f4df-9520-3691-2461-7b0fd54e8b3a)
- [Submeter FP ao Defender (Q&A)](https://learn.microsoft.com/en-us/answers/questions/5635032/urgent-help-windows-defender-falsely-flags-our-dig)
- [Sidecar assinado: launcher-rust PR #355](https://github.com/decentraland/launcher-rust/pull/355)
