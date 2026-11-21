# Armazenamento protegido e pasta de instalação (o que o app elevado confia)

**Última revisão:** 2026-11-21 · Código: `crates/core/src/protect.rs` (store, migração, DACL da pasta de instalação, regra da cadeia do exe), usuários em `registry.rs` (`backups_dir`, `load_backup`/`save_backup`), `dns.rs`, `apps.rs` (desfazer da remoção), `nicpower.rs`/`exposure.rs`/`peripherals.rs`/`gamemode.rs` (pelo `registry`); shell `src-tauri/src/lib.rs` (`setup`, `autostart_set`); UI `src/pages/settings.tsx`, `src/pages/boost.tsx` (DNS).

**Ameaça.** O G2 Boost quase sempre roda elevado ("sempre abrir como administrador"). O data dir (`%LOCALAPPDATA%\G2 Boost`) e pastas como `%LOCALAPPDATA%\Microsoft\WindowsApps` são graváveis por qualquer programa do usuário (malware de integridade média). Tudo o que o app elevado lê dali e transforma em ação de admin é escalonamento de privilégio.

## A. Store protegido para backups e desfazer

- **Onde:** elevado, `protect::state_dir()` = `%ProgramData%\G2 Boost\secure` (ProgramData pelo shell, `FOLDERID_ProgramData`, nunca variável de ambiente). Não elevado: o data dir, como antes.
- **O que mora lá:** `backups\<id>.json` (ajustes, correções da Segurança, GPU do Modo Jogo, mouse/teclado, placa de rede), `dns_backup.json`, `uninstall_undo.json` e `uninstall_backups\<unix>-<n>.json` (chaves de registro antes de remover sobras).
- **Permissões:** as duas pastas (`G2 Boost` e `secure`) nascem com `CreateDirectoryW` + descritor `O:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)`: dono Administradores, DACL protegida (nada herdado do ProgramData, que dá a Usuários "criar arquivos" em toda subpasta), só SYSTEM e Administradores, nem leitura para Usuários.
- **Pasta pré-criada:** qualquer programa pode criar `ProgramData\G2 Boost` antes do app. Se ela (ou `secure`) for junção, tiver dono fora de SYSTEM/Administradores/TrustedInstaller ou der escrita a outros (`antivirus::writable`), **não é usada nem lida**: é renomeada para `<nome>.untrusted-<unix>` (renomear move a junção, não segue) e uma nova é criada. Se não der (alguém recriou no meio), erro: sem store.
- **Elevado nunca usa o data dir:** sem store, `state_dir` devolve erro; `save_backup` falha (o ajuste não é aplicado sem backup), `load_backup` vê nada, o desfazer some. Teste: `protect::tests::elevated_store_is_protected_or_refused_never_the_data_dir`.
- **Migração (uma vez):** na primeira abertura elevada, o que uma versão antiga deixou no data dir vai para o store (`migrate`, marcador `secure\migrated`): só nomes nossos (`[a-z0-9_-]{1,64}.json`, até 500 por pasta, 1 MB por arquivo). Cada arquivo é **copiado como arquivo novo** (herda a DACL do store; mover manteria a DACL do usuário) e apagado **pelo mesmo handle**, só se `GetFinalPathNameByHandleW` bater com o caminho esperado: nada atravessado por junção é apagado (o arquivo é copiado e fica). Depois do marcador, o data dir nunca mais é lido elevado. As restaurações mantêm as validações de sempre (entradas fora do esperado são recusadas: `boost::restore_entries`, `nicpower::restore`, `dns::restore`, `apps::undo` com `reg_path_allowed`). Teste: `migration_moves_once_and_never_deletes_through_a_junction`.
- **Desfazer de remoção:** `undo` lê cada backup de chave **só pelo nome do arquivo** dentro do `uninstall_backups` atual (`key_backup_file`), nunca por um caminho do registro de desfazer. Uma chave apagada pela metade (`RegDeleteTreeW` não é atômico) entra no desfazer mesmo assim (`half_deleted_key_keeps_its_undo`).
- **Consequência aceita:** um backup gravado elevado não aparece numa execução sem admin (e vice-versa depois da migração). Desligar o ajuste sem backup usa o padrão documentado e a UI avisa, como já era.
- **Builds de debug** (testes, testes elevados) seguem `G2BOOST_DATA_DIR` (`<dir>\ProgramData\G2 Boost`); o release ignora a variável para o store.

## B. Pasta de instalação

- O instalador NSIS aceita qualquer pasta (a do dono é `G:\G2 Boost`, que herda "Modificar" para Usuários autenticados da raiz do G:). Ao abrir **elevado** numa cópia **instalada** (`selfupdate::install_kind == "installed"`, `uninstall.exe` ao lado), se a pasta ou um arquivo dela for gravável por outros, `protect::protect_install_dir()` aplica `O:BAD:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;BU)(A;OICI;0x1200a9;;;AU)` (SYSTEM/Administradores total, Usuários e Usuários autenticados ler e executar, herdável; `SetNamedSecurityInfoW` propaga para os arquivos) e confere depois.
- **Nunca** numa raiz de drive, num compartilhamento, dentro de Windows, `C:\Users` ou do perfil, numa junção, nem numa pasta com qualquer coisa além de exatamente `G2 Boost.exe` + `uninstall.exe` (`refuse_install_dir`). Cópia portátil e de desenvolvimento: nunca alteradas.
- **Iniciar com o Windows:** a tarefa de logon só ganha "privilégios mais altos" se o app está elevado, não é portátil e `protect::exe_chain_safe(exe)`: o exe e a pasta dele sem nenhuma escrita de outros (a mesma regra da liberação no Defender, `antivirus::writable_by_others`), e cada pasta acima até a raiz sem direito de outros a renomear ou trocar o que está no caminho (apagar, apagar filho, mudar permissão ou dono; na raiz, apagar não conta). Senão a tarefa é LUA e Configurações diz por quê (`settings.autostart.unsafe_folder`). Teste com formas reais de ACL: `chain_rule_from_real_acl_shapes`.

## Outras portas fechadas na mesma rodada

- **winget:** só o alias de execução do App Installer é iniciado (detalhe em [updates.md](../core/updates.md)).
- **Desinstalador de entrada do HKCU** (gravável pelo usuário): com o app elevado, abre pelo Explorer com os direitos do usuário (`games::run_unelevated`), nunca com o token de admin. O MCP `uninstall_app` só aceita apps da lista lida agora (o snapshot `apps_snapshot.json` é gravável).
- **MCP:** pedido com mais itens do que o diálogo mostra (14) é recusado antes do diálogo (`too_many_items`); ajuste beta só no app (`beta_in_app_only`); restaurar DNS mostra os servidores que vão voltar (no diálogo e na tela de DNS).

## Não testado aqui

Criação do store e DACL da pasta de instalação **elevados** (o teste do store cobre os dois caminhos e roda completo num shell elevado; `protect_install_dir` real só no app instalado).
