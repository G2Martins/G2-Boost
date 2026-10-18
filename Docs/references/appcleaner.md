# AppCleaner 3.7 (macOS) — análise estática (clean-room) para o desinstalador do G2 Boost

**Última revisão:** 2026-10-18 (mapeamento implementado: [../cleanup/app-uninstall.md](../cleanup/app-uninstall.md)) · Fonte: `Referencias/Downloads/AppCleaner_3.7/AppCleaner.app` · Alvo da fase 3C (desinstalar + resíduo).

> **Método:** só dados do pacote — `Info.plist`/`BundlePaths.plist`/`InternetAccessPolicy.plist`, `Localizable.strings`, nibs decodificados pelo REA (`decode_interface_builder`, evidência `ev_bce3109f…`), e nomes de classe/método/strings das seções `__objc_classname`, `__objc_methname` e `__cstring` dos Mach-O (parser próprio, fatia x86_64). **Nenhum código foi desmontado nem executado** (o Ghidra do REA no Windows só aceita PE; Hopper exige macOS). Comportamento além do nome do símbolo está marcado **[inferido]**.
> **Licença:** "Copyright © 2026 FreeMacSoft. All rights reserved." Documentamos **mecanismo**; não copiar ícones, sons (`drag to trash.aiff`, `restore from trash.aif`), textos, nibs nem a lista literal de pastas.

## 1. Inventário
| Componente | O que é |
|---|---|
| `AppCleaner` 3.7.0 (build 4485) | App Cocoa em Objective-C, universal x86_64 + arm64, macOS 15.6+. Frameworks: AppKit, Security, ServiceManagement, DiskArbitration, QuickLookUI, UniformTypeIdentifiers. Log via CocoaLumberjack (`DD*`) |
| `achelper` | CLI privilegiado mínimo: `trash file...`, `restore src dst`, `cleanold`. Lê `SUDO_UID/SUDO_USER`, faz `setuid`/`setreuid` |
| `AppCleaner SmartDelete.app` 1.5 | Login item (`LSUIElement`) que vigia as Lixeiras |
| `Sparkle.framework` (B) | Atualizador; única conexão de rede declarada: `freemacsoft.net:443` (feed `updates.xml`, chave EdDSA) |

## 2. Fluxo de UI (nibs)
- **Drop view** ("Arraste seus apps aqui") ↔ **Lista** (`ListView`): popup de tipo (`bundleTypeChanged:` → Aplicativos / Widgets / Plugins, cada um "do usuário" e "do sistema"), busca (`filter:`), colunas nome/versão/caminho, "Mostrar no Finder".
- **Desinstalar** (`UninstallView`): árvore (`NSOutlineView`) com checkbox por arquivo (`checkboxChanged:`), tamanho, caminho, "Mostrar no Finder", popover de ajuda, **Remover/Cancelar**. Cabeçalho: "N arquivos encontrados • X MB".
- Texto de ajuda: move os itens marcados **para a Lixeira**; a última remoção pode ser **desfeita** (Editar → Desfazer); "em casos raros pode achar arquivos não relacionados — confira sempre".
- **Preferências**: Geral (tabela de **apps protegidos** com +/−, "proteger apps da Apple"), SmartDelete (interruptor), Atualizações.

## 3. Como acha o resíduo
- **Identidade do app**: bundle id normalizado (`normalizeBundleId`), `CFBundleName`/`CFBundleDisplayName`, versão; detecta bundles aninhados (`Found nested bundle`).
- **Raízes fixas** (~70): subpastas de `~/Library` e `/Library` (Application Support, Caches, Preferences + `ByHost`, Containers, Group Containers, LaunchAgents/Daemons, Logs + DiagnosticReports, Saved Application State, WebKit, Cookies, …), `/private/var/db/receipts` (recibos de pacote), `/Users/Shared`, lista de documentos recentes. Busca com **profundidade limitada** (`relatedFilesInDirectories:maxDepth:matching:`).
- **Casamento por nome**: regex `^<nome>(?:[.\-_\s\d]+.*)?$` (nome exato, ou seguido de separador/dígitos — pega `App 2`, `App.helper`), remoção de sufixo de versão/beta (`<nome>[0-9BETAbeta.-]*`), prefixo sem caixa (`hasIPrefix:`), string "amassada" (`mush`, normalização) e distâncias **Levenshtein** / `sequentialDistance` **[inferido: fuzzy com limiar]**.
- **Casamento forte por assinatura**: `SecCodeCopySigningInformation` → entitlement `com.apple.security.application-groups` → Group Containers do mesmo Team ID; Containers pelo bundle id exato.
- **Tamanho** alocado em disco (`getAllocatedSize:`), não o lógico. Opção `keepIfNoRelatedFiles`.
- **Dados preciosos**: diálogo extra "Você vai remover os dados de usuário de X — ficam em <pasta>" (`showUserDataRemovalConfirmationForApp:dataPath:`); exemplo embutido é o cofre de um gerenciador de senhas.

## 4. Proteções
- Lista do usuário (`ProtectedApps`, `ShowProtectedApps`), proteger apps da Apple, **proteger apps abertos** (`ProtectOpenedApps`, `runningApplicationsWithBundleIdentifier:`).
- Apps do sistema/SIP (`isSIPProtected`, `/System/Applications`) → "Não é possível remover apps do sistema".
- Sem Full Disk Access (checa `TCC.db`) → aviso com "Abrir Ajustes" / "Remover mesmo assim".

## 5. Remoção, desfazer, helper
- `trashItemAtURL:resultingItemURL:` guarda onde cada item caiu na Lixeira → `registerUndoWithTarget:handler:` = **Desfazer a última remoção** (restaura da Lixeira).
- Item sem permissão → `achelper` com autorização de admin, mesmos verbos (`trash`, `restore`). Superfície privilegiada mínima.
- Antes de apagar launch agent/daemon: `launchctl bootout gui/<uid>` (`ACLaunchdManager`). `HistoryManager` **[inferido: histórico de remoções]**.

## 6. SmartDelete
`FSEventStream` sobre as Lixeiras ("Started monitoring trashes"). Quando um `.app` aparece lá ("Trashed app detected"), abre a mesma tela de desinstalar com os associados. Ignora se o AppCleaner principal está aberto e se o app "já está instalado" (troca de versão pelo atualizador).

## 7. Mapeamento para o Windows (o que o G2 faz — desenho próprio)
| AppCleaner (macOS) | G2 Boost (Windows) |
|---|---|
| Apps em `/Applications`, `~/Applications` | Chaves `Uninstall` (HKLM, HKLM\WOW6432Node, HKCU): `DisplayName`, `Publisher`, `InstallLocation`, `DisplayIcon`, `UninstallString`/`QuietUninstallString`, `EstimatedSize`; Store via WinRT `PackageManager.FindPackagesForUser` (sem PowerShell) |
| Bundle id / Team ID | Chave Uninstall + exe principal; assinante Authenticode (`WinVerifyTrust`) ≈ Team ID; AppX: `PackageFamilyName` (`%LocalAppData%\Packages\<PFN>` = casamento exato) |
| `~/Library/{Application Support,Caches}` | `%AppData%`, `%LocalAppData%`, `AppData\LocalLow`, `%ProgramData%` (profundidade 1–2, padrão `<Publisher>\<App>` e `<App>`) |
| `Preferences/*.plist` | `HKCU\Software\<Publisher>\<App>` (+ HKLM/WOW6432Node com admin) |
| LaunchAgents/Daemons + `bootout` | Serviços (`ImagePath` dentro do `InstallLocation`) → parar/desabilitar via SCM; tarefas agendadas cuja ação aponta pro `InstallLocation`; Run/`StartupApproved` (`startup.rs`) |
| Logs/DiagnosticReports | `%LocalAppData%\CrashDumps\<exe>*.dmp` |
| Atalhos (Dock/recentes) | Menu Iniciar (usuário + comum), Área de Trabalho, fixados na barra; alvo resolvido por `IShellLinkW` |
| Regex de nome + Levenshtein | Mesma ideia em camadas com **confiança visível**: alta = dentro do `InstallLocation` ou alvo de `.lnk`/serviço/tarefa; média = pasta com nome normalizado (`^<nome>([.\-_\s\d].*)?$`, sem versão/arquitetura/"(x64)") sob publisher; baixa = fuzzy, **desmarcado por padrão**. Cada item mostra o motivo |
| Dados preciosos | Perfis de navegador, `Saved Games`, `Documents\My Games`, cofres → confirmação extra com caminho, desmarcado por padrão |
| Apps protegidos / SIP / abertos | `SystemComponent=1`, `ParentKeyName`/`ReleaseType` (updates), `NoRemove`, publisher do Windows; app em execução bloqueia até fechar; lista do usuário em `settings.json`; tudo passa por `SafePath` (System32, WinSxS, outros perfis = bloqueado) |
| Lixeira + Desfazer | `SHFileOperationW` + `FOF_ALLOWUNDO` (guarda do `disk.rs`), com `ConfirmToken`; **desfazer implementado**: metadados `$I` da Lixeira + `$R` renomeado de volta, registro reimportado do backup |
| `achelper` | Sem helper: caminhos HKLM/ProgramData/serviço retornam `needs_admin`; "Elevar" relança via `runas` (padrão do projeto) |
| SmartDelete (FSEvents na Lixeira) | **implementado**: `RegNotifyChangeKeyValue` nas chaves Uninstall + snapshot no data dir; entrada sumiu → aviso no app que abre a varredura; também botão "procurar sobras" |
| Arrastar `.app` | Arrastar `.exe`/`.lnk` (Tauri drag-drop) → resolver para a entrada Uninstall por prefixo do `InstallLocation` |

## 8. Não determinado
Limiar/pesos do fuzzy, profundidade máxima real, ordem das raízes, formato do histórico, como "dados preciosos" são escolhidos além do exemplo. Nada foi executado.
