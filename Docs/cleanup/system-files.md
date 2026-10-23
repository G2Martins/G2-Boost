# Limpeza — Arquivos do sistema (handlers da Limpeza de Disco)

**Última revisão:** 2026-10-22 · Código: `crates/core/src/volcache.rs` (+ `repair.rs`: vaga única e cancelamento) · Shell: `volcache_preview` / `volcache_confirm` / `volcache_execute`, `repair_cancel` · UI: aba **Arquivos do sistema** em `src/pages/cleanup.tsx` (`src/components/cleanup/system-files.tsx`); botão "Limpar com segurança" no WinSxS e no Windows.old dos essenciais grandes (`src/pages/disk.tsx`).

## O que é

Os mesmos módulos que a Limpeza de Disco do Windows usa (`IEmptyVolumeCache2`, registrados em `HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Explorer\VolumeCaches\<chave>`), chamados **dentro do processo**, sem `cleanmgr.exe` e sem shell. É o caminho que a Microsoft sanciona para limpar o repositório de componentes, o Windows.old e logs de instalação, e o que o antivírus reconhece (ver [kaspersky-false-positive.md](../security/kaspersky-false-positive.md)): quem apaga é o próprio módulo do Windows, não um `del /s` nosso.

## Fluxo (igual ao catálogo)

1. **Prévia** (`volcache_preview(ids)`): para cada id, carrega o handler, `InitializeEx(chave, "C:\", nome da chave)` → `S_FALSE` = nada a limpar; senão `GetSpaceUsed` (bytes) → `Deactivate`. **Admin obrigatório**: sem elevação devolve `needs_admin` em todos os itens e não carrega nada. Evento `volcache:progress {done, total}` a cada handler (Limpeza de atualizações leva minutos).
2. **Confirmação**: painel com total e, em vermelho, o que não tem volta (itens com aviso). `volcache_confirm(ids)` emite um `ConfirmToken` do **mesmo** `TokenStore` do catálogo (uso único, 2 min, amarrado aos ids). Os ids `sys_*` nunca coincidem com ids do catálogo (teste), então um token não serve para o outro fluxo.
3. **Execução** (`volcache_execute(token, ids)`): token consumido antes de qualquer coisa; `GetSpaceUsed` + `Purge(bytes medidos)` + `Deactivate` por handler, numa thread STA própria. Bytes liberados = último `PurgeProgress` (ou o medido).
4. **Histórico**: o mesmo `history.json` da limpeza (`cleanup::record_run`), gatilho `manual`.
5. **Parar**: `repair_cancel` → o callback devolve `E_ABORT` no próximo progresso.

Uma tarefa de manutenção por vez no app inteiro (`repair::exclusive`): a limpeza de arquivos do sistema e o DISM mexem no mesmo repositório de componentes. Ocupado = `busy`.

## Allowlist (verificada no Windows 11 26200 em 2026-10-22)

| id | Chave | DLL esperada (System32) | Risco | Padrão | Aviso |
|---|---|---|---|---|---|
| `sys_update_cleanup` | Update Cleanup | `scavengeui.dll` | médio | desligado | não dá mais para desinstalar atualizações |
| `sys_previous_installations` | Previous Installations | `setupcln.dll` | **alto** | desligado | remove a volta para a versão anterior (Windows.old) |
| `sys_driver_packages` | Device Driver Packages | `pnpclean.dll` | médio | desligado | não dá para reverter driver |
| `sys_upgrade_logs` | Windows Upgrade Log Files | `setupcln.dll` | baixo | ligado | — |
| `sys_temp_setup` | Temporary Setup Files | `setupcln.dll` | baixo | ligado | — |
| `sys_upgrade_discarded` | Upgrade Discarded Files | `setupcln.dll` | baixo | ligado | — |
| `sys_reset_logs` | Windows Reset Log Files | `setupcln.dll` | baixo | ligado | — |
| `sys_setup_logs` | Setup Log Files | `dataclen.dll` | baixo | ligado | — |
| `sys_chkdsk` | Old ChkDsk Files | `dataclen.dll` | baixo | ligado | — |
| `sys_defender` | Windows Defender | `dataclen.dll` | baixo | ligado | — |
| `sys_memory_dump` | System error memory dump files | `dataclen.dll` | médio | desligado | — (o `MEMORY.DMP` que o catálogo não alcança: `SafePath` bloqueia a raiz do `%SystemRoot%`) |

**Fora, de propósito:** `DownloadsFolder` (a pasta Downloads do usuário), `Recycle Bin` (já é `recycle`), `User file versions` (Histórico de Arquivos = dado do usuário), `Windows ESD installation files` (necessário para "Restaurar o PC"), `Language Pack`. **Já cobertos pelo catálogo (o catálogo vence):** `Temporary Files` (`temp_*`), `Thumbnail Cache` (`thumbs`), `D3D Shader Cache` (`shaders`), `Delivery Optimization Files` (`wu_cache`), `Windows Error Reporting Files` (`wer`), `System error minidump files` (`dumps`), `Internet Cache Files` (`inetcache`). Legados de IE/Media Player ficam de fora.

## Segurança: o app pode estar elevado

Uma chave `VolumeCaches` plantada carregaria uma DLL qualquer dentro do nosso processo. Defesa em camadas:

1. **Só chaves da allowlist.** O chamador manda ids (`sys_*`), nunca nomes de chave; `valid_selection` recusa id desconhecido, repetido ou vazio.
2. **CLSID parseado** como GUID e reformatado por nós antes de entrar num caminho de registro (nada de `{GUID}\..\x`).
3. **Registro da máquina, não do usuário:** `InprocServer32` lido de `HKLM\SOFTWARE\Classes\CLSID\{…}` (o `HKCR` mistura `HKCU\Software\Classes`, que o usuário comum escreve).
4. **Caminho:** só `%SystemRoot%`/`%windir%` são expandidos; qualquer outra variável, caminho relativo, `.`/`..`, UNC ou `\\?\` é recusado; depois `canonicalize` (links que saem do System32 caem) e a DLL tem que estar **direto** em `System32` **com o nome esperado** daquela chave.
5. **Carregamos o arquivo validado**, não o que o COM acharia: `LoadLibraryExW(caminho completo, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | SYSTEM32)` + `DllGetClassObject` + `IClassFactory::CreateInstance`. Nenhum `CoCreateInstance`.

Limite conhecido: os handlers do `dataclen.dll` (data-driven) leem valores da própria chave (`Folder`, `FileList`). Quem consegue editar `HKLM\...\VolumeCaches` já é administrador; fora do modelo de ameaça.

## Testes

- `volcache.rs`: allowlist sem Downloads/Lixeira/Histórico de Arquivos/ESD e sem sobreposição de ids com o catálogo; itens com aviso nunca pré-selecionados; seleção validada (id do catálogo, nome de chave, repetido, vazio, `../evil`); caminhos de `InprocServer32` (aceita `%SystemRoot%\System32\x.dll`, recusa perfil, subpasta, `..`, `.`, relativo, `%TEMP%`, UNC, `\\?\`, nome errado); CLSID reformatado; **todos os handlers desta máquina** passam pela checagem de confiança (registro + `canonicalize`, nada carregado); sem admin: prévia `needs_admin` sem carregar e execução grava `needs_admin` no histórico (`G2BOOST_DATA_DIR`).
- `measure_live` (`#[ignore]`, **elevado**): carrega cada handler e chama só `GetSpaceUsed`. **`Purge` nunca roda em teste automático.**

**Não verificado pelo agente:** prévia e execução reais (precisam de admin). Passos elevados em [../core/maintenance.md](../core/maintenance.md#verificação-elevada).
