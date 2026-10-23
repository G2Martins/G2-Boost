# Desinstalar apps (AppCleaner para Windows)

**Última revisão:** 2026-10-23 (aba Atualizações) · 2026-10-18 · Código: `crates/core/src/apps.rs` (+ `registry::export_tree/delete_tree`, `disk::shell_recycle`, `boost::delete_orphan_service`, `cleanup::TokenStore`); shell `src-tauri/src/lib.rs` (`AppsState`, `apps_*`); UI `src/pages/apps.tsx` (rota **Desinstalar apps**), drop em `src/App.tsx` + `src/lib/nav.ts`. Conceito de origem (clean-room): [../references/appcleaner.md](../references/appcleaner.md).

## O que faz

A página tem uma terceira aba, **Atualizações** (2026-10-23: apps pelo winget e Windows Update), descrita em [../core/updates.md](../core/updates.md).

0. **Ícones (2026-10-18):** `icons.rs` extrai o ícone do `DisplayIcon` (`caminho[,índice]`, só `.exe/.dll/.ico` absolutos; senão o exe do app) com `ExtractIconExW` e codifica PNG via WIC; apps da Store usam o `Logo` do pacote (variante `scale-100` ou a menor). Nada é executado. O shell guarda em cache por id (`apps_icon`); a UI mostra a inicial num quadrado enquanto carrega.
1. **Lista** os apps como Configurações > Apps: chaves `Uninstall` (HKLM 64/32 bits, HKCU) + **apps da Store** do usuário (WinRT `PackageManager`, sem PowerShell). Esconde componentes do sistema (`SystemComponent=1`), atualizações (`ParentKeyName`, `ReleaseType`) e entradas sem desinstalador; deduplica por nome + versão.
2. **Protege** (não desinstala daqui): runtimes/frameworks/drivers (`runtime` — VC++, .NET, Edge/WebView2, SDKs, GameInput, "driver/chipset/firmware/redistributable"), **software de segurança** (`security` — os mesmos marcadores da Inicialização), o próprio G2 Boost (`self`), pacotes da Store essenciais (Store, Instalador de Aplicativo, Segurança do Windows, `Microsoft.Windows.*`) e o que o usuário marcar (`user`, `settings.json → protected_apps`).
3. **Desinstala** com o desinstalador do próprio app, após o `<Modal>` de confirmação:
   - `uninstaller()` analisa o `UninstallString` (dado hostil): caminho entre aspas ou até o `.exe` que fecha o token; **recusa** `cmd`, `powershell`, `pwsh`, `wscript/cscript`, `mshta`, `rundll32`, `regsvr32`, `reg`, `sc`, `net`, `wmic`, `schtasks`, `certutil`… e qualquer programa sob `%SystemRoot%`; exige `.exe` absoluto que exista.
   - **MSI:** sempre `%SystemRoot%\System32\msiexec.exe /X{código do produto}` (o código é extraído e validado; `/I`, `/quiet` e o resto são descartados).
   - **App aberto não desinstala** (`app_running:<exe>`): se o exe do app ou qualquer programa dentro da pasta dele está rodando, pede para fechar antes.
   - Lançado por `ShellExecuteExW` (visível; o manifesto do desinstalador decide o UAC; UAC cancelado = `cancelled`). A UI consulta `apps_installed` a cada 2 s; quando a entrada some, refaz a busca de sobras.
   - **Store:** `RemovePackageAsync` para o usuário atual (bloqueia até o Windows terminar).
4. **Procura sobras** (`leftovers`) só em raízes fixas — AppData (Roaming), LocalAppData, LocalLow, ProgramData, `LocalAppData\Programs`, **Jogos salvos** e **Documentos\My Games** (known folders), Menu Iniciar (usuário e todos), Área de Trabalho (via `SHGetKnownFolderPath`, funciona com OneDrive) e Pública — mais a pasta de instalação, `CrashDumps` e o registro:

| Confiança | Quando |
|---|---|
| **Alta** | pasta de instalação que ficou; atalho cujo alvo (`IShellLinkW`) aponta para ela; pasta/chave com o nome do app (normalizado: sem marcas, arquitetura, `Inc/LLC`, versão no fim), com o "nome do produto" sem as palavras do fabricante (`Google Chrome` → `Google\Chrome`) ou com o nome do exe; dump `exe.*.dmp`; `Packages\<família>` de app da Store; serviço do app cujo programa já não existe |
| **Média** | nome + palavra de versão (`App 2`); pasta sem versão de um app versionado (`EA SPORTS FC` para `FC 27`); atalho com o nome do app |
| **Baixa** | quase igual (Levenshtein ≤ 1 a cada 8 letras: `Spotfy` para `Spotify`), só para nomes sem versão — **nunca vem marcado** |
| — | Nunca: versão diferente de app versionado (`FC 26` ≠ `FC 27`), outra pasta do mesmo fabricante, a pasta do fabricante em si, nomes curtos demais (< 4 letras), pastas/chaves compartilhadas (`Microsoft`, `Windows`, `Packages`, `Temp`, `Classes`, `Policies`…) |

   Alta e Média vêm marcadas; cada item mostra o motivo. **Dados preciosos** (`precious`): pasta/arquivo de saves, perfis (`Profiles`, `User Data` do navegador), cofres, carteiras, backups — ou tudo em Jogos salvos/My Games — ganha o chip **Seus dados**, nunca vem marcado e, se escolhido, o modal avisa em destaque. Registro: `Software\<Nome>` e `Software\<Fabricante>\<Nome>` em HKCU, HKLM e WOW6432Node. Serviços: só processo próprio/compartilhado (drivers nunca), fora do `%SystemRoot%`.
5. **Remove** só depois do app desinstalado (`still_installed` caso contrário — apagar a pasta de um app instalado seria desinstalação forçada) e com **`ConfirmToken`** de uso único amarrado ao app + ids:
   - arquivos/pastas → **Lixeira** (`SHFileOperationW` + `FOF_ALLOWUNDO`), cada caminho **re-verificado na hora** (pode ter virado junção depois da prévia);
   - chave de registro → **backup JSON completo** (`uninstall_backups/<ts>.json`, todos os tipos de valor) e depois `RegDeleteTreeW`; só chaves logo abaixo das bases pesquisadas;
   - serviço → `DeleteService` (admin), só se o programa continua inexistente;
   - itens de todos os usuários (ProgramData, HKLM, serviços) sem admin → contados como `needs_admin`.
   - Histórico em `cleanup_history.json` (gatilho `app_leftovers`).
   - **Desfazer a última remoção** (`apps_undo`, botão no topo da página): grava `uninstall_undo.json` (caminhos reciclados + backups de registro). Os arquivos voltam da Lixeira lendo os metadados `$I` (v1 Vista/7 e v2 Windows 10+: data da exclusão + caminho original) e renomeando o `$R` de volta — nada de shell; só itens apagados a partir da hora da remoção e só para destino que não existe, fora do `%SystemRoot%`. As chaves voltam do backup com tipo e bytes originais, passando pela mesma regra de caminho da remoção (base\\X ou base\\Fabricante\\X, nunca `Microsoft`/`Classes`/`Policies`, nome de subchave de um segmento só).
6. **Sobras de apps removidos** (SmartDelete): cada listagem grava `apps_snapshot.json`; a aba compara com o que está instalado agora e mostra os apps que sumiram e deixaram algo. Removeu tudo → o app sai do snapshot. **Ao vivo:** uma thread vigia as três chaves `Uninstall` (`RegNotifyChangeKeyValue`, sem polling, rajadas agrupadas em 1,5 s); um app conhecido que some vira o aviso "X foi desinstalado — ver sobras?" no topo do app, que abre a aba e já varre.
7. **Arrastar e soltar**: um `.exe` ou `.lnk` solto em qualquer lugar da janela abre o desinstalador com o app dele (exe = `DisplayIcon`, ou a pasta de instalação mais longa que contém o alvo).

## Apps companheiros (2026-10-18, caso Parsec)

O instalador do Parsec coloca dois apps separados, **Parsec Virtual Display Driver** e **Parsec Virtual USB Adapter Driver**, cada um em `C:\Program Files\<nome>`. Depois de desinstalados, as duas pastas ficaram com só o `uninstall.exe` (o desinstalador NSIS não apaga a si mesmo) e a busca de sobras não as achou: `Program Files` não era varrida e "Parsec + palavras que não são versão" era descartado. O dono apagou à mão (Lixeira, 2026-10-18 13:01).

- **Sobras de companheiro** (`Reason::Companion`): pastas em `Program Files`/`Program Files (x86)` (e nas pastas de dados) cujo nome é o nome inteiro do app seguido de outras palavras que não são versão (`Matcher::companion`; nunca para nomes curtos, menos de 5 letras, nem versionados), e que **não são a pasta de instalação de nenhum app ainda instalado**. Confiança alta quando só sobrou desinstalador/log (`only_uninstaller`), média caso contrário. A pasta do próprio app em `Program Files` com nome exato também entra (alta). A raiz `Program Files` nunca.
- **Companheiros ainda instalados** (`companions`): mesmo prefixo de nome, ou mesmo fabricante instalado no mesmo dia e com uma palavra do nome em comum; protegidos ficam de fora. A tela mostra "O X instalou também" com botão Desinstalar de cada um (cada app pelo próprio desinstalador); o MCP devolve `companions_still_installed` em `get_app_leftovers`.

**Bug corrigido (2026-10-18):** a prévia aceitava as pastas em `Program Files` pela regra de pasta de instalação, mas a remoção rechecava com essa regra só para `InstallFolder`, então companheiros e pastas com o nome exato em `Program Files` apareciam e falhavam na hora de remover. Agora cada sobra guarda o próprio flag (`Leftover::install_dir`) e a remoção usa o mesmo (`program_files_leftovers_can_be_removed`).

## Pastas sem dono em todos os discos (2026-10-18, pedido do dono)

O inverso das sobras de um app: parte das **pastas** e pergunta se alguém ainda usa (`apps::unowned`, placeholder `SWEEP_ID = "sweep:unowned"` no mesmo fluxo de prévia → token → Lixeira → desfazer). Acha o que apps removidos há anos, ou antes do G2 Boost existir, deixaram.

- **Onde:** filhos diretos de Roaming, Local, LocalLow, ProgramData, `LocalAppData\Programs`, `Program Files*`; nos **outros discos fixos**, `Program Files*`, `Games`, `Jogos`, `Programs`, `Programas`; e `steamapps\common` de **cada biblioteca da Steam** (`steam::library_folders`, com os `installdir` dos manifests).
- **Dono** (`Owners`): qualquer app instalado que case pelo nome (qualquer confiança), fabricante ou companheiro; pasta de instalação de app instalado (dentro ou contendo); pasta de onde roda um processo aberto ou o executável de um serviço. Nunca: componentes sem entrada de desinstalação (`SWEEP_DENY`, nomes que começam com `Windows`/`Microsoft`, `MSBuild`, `Microsoft.NET`, `VulkanRT`, `DigitalEntitlements`...).
- **Silêncio:** nada dentro mudou há **90 dias** (Steam: 14), leitura limitada a 300 mil entradas sem seguir junção. Sem isso, não aparece (ferramenta portátil em uso muda de arquivos).
- **Confiança:** alta quando só sobrou desinstalador, ou quando não há programa dentro (`.exe/.dll/.sys`) e é jogo da Steam sem manifest ou está parada há 1 ano ou mais. **Com programa dentro é baixa** (jogo instalado à mão não muda os arquivos ao ser jogado), nunca pré-marcada. LocalLow sempre conta como dado do usuário (saves de jogos).
- **Medido na máquina do dono (2026-10-18, só leitura, `unowned_live`):** ~80 pastas; maiores: `NBA 2K25` órfão na biblioteca G: (3,9 GB), Android SDK (4,4 GB, baixa), Ubisoft, VALORANT, Postman (baixa), Norton, Brave, Docker, VirtualBox (alta). Primeiro passe mostrou `MSBuild`/`Microsoft.NET`/`VulkanRT` → entraram no deny.
- **MCP:** `get_unowned_folders` (leitura) e `remove_unowned_folders(paths)` (refaz a varredura, só alta e não preciosa, janela nativa de confirmação).

## Guarda de caminho (`fs_allowed`)
Toda sobra de arquivo, mostrada ou removida: não é reparse point (junção/symlink nunca é seguida nem removida) → `canonicalize` → `SafePath` (System32, WinSxS, outro usuário, UNC = fora) → fora do `%SystemRoot%` → nunca uma raiz (as bases, o perfil, `Documents`, `Downloads`, unidade, `Program Files`, `Users`, `ProgramData`) → dentro de uma base; ProgramData só abaixo dela. A pasta de instalação passa só se estiver abaixo de `Program Files*`, de `LocalAppData\Programs` ou, em outra unidade, a dois níveis ou mais (`D:\Jogos\X`). Snapshot e nomes de família da Store são revalidados (arquivo editável pelo usuário).

## Fora (backlog)
Tarefas agendadas órfãs (falta enumerar ações via COM), remover entrada `Uninstall` quebrada (desinstalador inexistente), tamanho alocado em disco nas sobras (hoje: bytes lógicos), aviso do SmartDelete também como notificação do Windows com o app na bandeja.

## Testes
`apps`: normalização e casamento (Firefox/Chrome/FC 27 × FC 26, nomes curtos), validação do desinstalador (todas as recusas + MSI reescrito + desinstalador inexistente + protegido), runtimes, `%VAR%`, família da Store; **sandbox** de perfil fabricado (pasta do app, pasta do fabricante, versão, ProgramData, Menu Iniciar, dump, pasta de instalação; outro app do mesmo fabricante fica); **segurança**: raízes, perfil, System32, **junção** plantada (não aparece e não é removida), pasta de instalação hostil apontando para o perfil, **junção trocada depois da prévia** (recusada na remoção, nada atrás dela tocado); listagem ao vivo (ids, deduplicação, Store). `registry::tree_export_then_delete` (backup + apagar + **reimportar idêntico**, nome de subchave com `..\\` recusado). Desfazer: `$I` v1/v2 (inclusive truncado/versão desconhecida), caminhos de registro para remover/reimportar (Run, Classes, `..` recusados), dados preciosos marcados. `undo_live` (ignorado, à mão) recicla uma pasta do sandbox e a traz de volta pela Lixeira real — verificado em 2026-10-18. `leftovers_live` (ignorado) imprime as sobras reais, só leitura.
