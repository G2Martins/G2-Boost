# Desinstalar apps (AppCleaner para Windows)

**Última revisão:** 2026-10-18 · Código: `crates/core/src/apps.rs` (+ `registry::export_tree/delete_tree`, `disk::shell_recycle`, `boost::delete_orphan_service`, `cleanup::TokenStore`); shell `src-tauri/src/lib.rs` (`AppsState`, `apps_*`); UI `src/pages/apps.tsx` (rota **Desinstalar apps**), drop em `src/App.tsx` + `src/lib/nav.ts`. Conceito de origem (clean-room): [../references/appcleaner.md](../references/appcleaner.md).

## O que faz
1. **Lista** os apps como Configurações > Apps: chaves `Uninstall` (HKLM 64/32 bits, HKCU) + **apps da Store** do usuário (WinRT `PackageManager`, sem PowerShell). Esconde componentes do sistema (`SystemComponent=1`), atualizações (`ParentKeyName`, `ReleaseType`) e entradas sem desinstalador; deduplica por nome + versão.
2. **Protege** (não desinstala daqui): runtimes/frameworks/drivers (`runtime` — VC++, .NET, Edge/WebView2, SDKs, GameInput, "driver/chipset/firmware/redistributable"), **software de segurança** (`security` — os mesmos marcadores da Inicialização), o próprio G2 Boost (`self`), pacotes da Store essenciais (Store, Instalador de Aplicativo, Segurança do Windows, `Microsoft.Windows.*`) e o que o usuário marcar (`user`, `settings.json → protected_apps`).
3. **Desinstala** com o desinstalador do próprio app, após o `<Modal>` de confirmação:
   - `uninstaller()` analisa o `UninstallString` (dado hostil): caminho entre aspas ou até o `.exe` que fecha o token; **recusa** `cmd`, `powershell`, `pwsh`, `wscript/cscript`, `mshta`, `rundll32`, `regsvr32`, `reg`, `sc`, `net`, `wmic`, `schtasks`, `certutil`… e qualquer programa sob `%SystemRoot%`; exige `.exe` absoluto que exista.
   - **MSI:** sempre `%SystemRoot%\System32\msiexec.exe /X{código do produto}` (o código é extraído e validado; `/I`, `/quiet` e o resto são descartados).
   - Lançado por `ShellExecuteExW` (visível; o manifesto do desinstalador decide o UAC; UAC cancelado = `cancelled`). A UI consulta `apps_installed` a cada 2 s; quando a entrada some, refaz a busca de sobras.
   - **Store:** `RemovePackageAsync` para o usuário atual (bloqueia até o Windows terminar).
4. **Procura sobras** (`leftovers`) só em raízes fixas — AppData (Roaming), LocalAppData, LocalLow, ProgramData, `LocalAppData\Programs`, Menu Iniciar (usuário e todos), Área de Trabalho (via `SHGetKnownFolderPath`, funciona com OneDrive) e Pública — mais a pasta de instalação, `CrashDumps` e o registro:

| Confiança | Quando |
|---|---|
| **Alta** | pasta de instalação que ficou; atalho cujo alvo (`IShellLinkW`) aponta para ela; pasta/chave com o nome do app (normalizado: sem marcas, arquitetura, `Inc/LLC`, versão no fim), com o "nome do produto" sem as palavras do fabricante (`Google Chrome` → `Google\Chrome`) ou com o nome do exe; dump `exe.*.dmp`; `Packages\<família>` de app da Store; serviço do app cujo programa já não existe |
| **Média** | nome + palavra de versão (`App 2`); pasta sem versão de um app versionado (`EA SPORTS FC` para `FC 27`); atalho com o nome do app |
| — | Nunca: versão diferente de app versionado (`FC 26` ≠ `FC 27`), outra pasta do mesmo fabricante, a pasta do fabricante em si, nomes curtos demais (< 4 letras), pastas/chaves compartilhadas (`Microsoft`, `Windows`, `Packages`, `Temp`, `Classes`, `Policies`…) |

   Alta e Média vêm marcadas; cada item mostra o motivo. Registro: `Software\<Nome>` e `Software\<Fabricante>\<Nome>` em HKCU, HKLM e WOW6432Node. Serviços: só processo próprio/compartilhado (drivers nunca), fora do `%SystemRoot%`.
5. **Remove** só depois do app desinstalado (`still_installed` caso contrário — apagar a pasta de um app instalado seria desinstalação forçada) e com **`ConfirmToken`** de uso único amarrado ao app + ids:
   - arquivos/pastas → **Lixeira** (`SHFileOperationW` + `FOF_ALLOWUNDO`), cada caminho **re-verificado na hora** (pode ter virado junção depois da prévia);
   - chave de registro → **backup JSON completo** (`uninstall_backups/<ts>.json`, todos os tipos de valor) e depois `RegDeleteTreeW`; só chaves logo abaixo das bases pesquisadas;
   - serviço → `DeleteService` (admin), só se o programa continua inexistente;
   - itens de todos os usuários (ProgramData, HKLM, serviços) sem admin → contados como `needs_admin`.
   - Histórico em `cleanup_history.json` (gatilho `app_leftovers`).
6. **Sobras de apps removidos** (SmartDelete v1): cada listagem grava `apps_snapshot.json`; a aba compara com o que está instalado agora e mostra os apps que sumiram e deixaram algo. Removeu tudo → o app sai do snapshot.
7. **Arrastar e soltar**: um `.exe` ou `.lnk` solto em qualquer lugar da janela abre o desinstalador com o app dele (exe = `DisplayIcon`, ou a pasta de instalação mais longa que contém o alvo).

## Guarda de caminho (`fs_allowed`)
Toda sobra de arquivo, mostrada ou removida: não é reparse point (junção/symlink nunca é seguida nem removida) → `canonicalize` → `SafePath` (System32, WinSxS, outro usuário, UNC = fora) → fora do `%SystemRoot%` → nunca uma raiz (as bases, o perfil, `Documents`, `Downloads`, unidade, `Program Files`, `Users`, `ProgramData`) → dentro de uma base; ProgramData só abaixo dela. A pasta de instalação passa só se estiver abaixo de `Program Files*`, de `LocalAppData\Programs` ou, em outra unidade, a dois níveis ou mais (`D:\Jogos\X`). Snapshot e nomes de família da Store são revalidados (arquivo editável pelo usuário).

## Fora (backlog)
Desfazer a última remoção (restaurar da Lixeira e reimportar o backup de registro) e vigia contínua das chaves `Uninstall` → fase 16 (descobertas da análise). Tarefas agendadas órfãs (falta enumerar ações via COM), remover entrada `Uninstall` quebrada, apps protegidos por estarem abertos → backlog.

## Testes
`apps`: normalização e casamento (Firefox/Chrome/FC 27 × FC 26, nomes curtos), validação do desinstalador (todas as recusas + MSI reescrito + desinstalador inexistente + protegido), runtimes, `%VAR%`, família da Store; **sandbox** de perfil fabricado (pasta do app, pasta do fabricante, versão, ProgramData, Menu Iniciar, dump, pasta de instalação; outro app do mesmo fabricante fica); **segurança**: raízes, perfil, System32, **junção** plantada (não aparece e não é removida), pasta de instalação hostil apontando para o perfil, **junção trocada depois da prévia** (recusada na remoção, nada atrás dela tocado); listagem ao vivo (ids, deduplicação, Store). `registry::tree_export_then_delete` (backup + apagar numa chave de teste do HKCU). `leftovers_live` (ignorado) imprime as sobras reais, só leitura.
