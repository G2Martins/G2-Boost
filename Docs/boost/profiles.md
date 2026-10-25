# Perfis, restauração, mouse e teclado

**Última revisão:** 2026-10-25 · Código: `crates/core/src/profiles.rs`, `peripherals.rs` (+ campo `profiles` em `settings.rs`); shell `src-tauri/src/profiles_cmds.rs`; UI `src/pages/profiles.tsx`, `src/pages/peripherals.tsx`, `src/components/profiles/*`, `src/components/periph/fader.tsx`, `src/lib/boost-hooks.ts`. Rotas `profiles` ("Perfis e restauração") e `peripherals` ("Mouse e teclado"). Os ajustes em si continuam em [tweaks.md](tweaks.md): estas páginas só juntam, desfazem e registram.

## Perfis (`profiles.rs`)

- **Um perfil** é `{ name, tweaks: [ids], fx_level }` em `settings.json` (`profiles`). `fx_level` (`minimal` | `balanced` | `performance`) só vale com `visual_fx` no perfil; sem ele, `balanced`. Nome: 1 a 40 caracteres, letras/números/espaço e `_ . ( ) + -`, sem caractere de controle, separador ou de direção de texto; único sem diferenciar caixa; no máximo **50** perfis, até 200 ids cada. Ids passam por `registry::safe_id` (`[a-z0-9_]`). Id que esta versão não conhece **fica no perfil** (uma versão nova pode ter) e é pulado ao aplicar, com aviso.
- **Prontos para usar** (não ficam no settings; a UI monta a lista de ids):

| Perfil | Ids | Regra do "desligar o resto" |
|---|---|---|
| Padrão | tweaks com `presets` ∋ `default` em `src/data/catalogs.ts`, efeitos visuais `balanced` | pergunta, vem marcado (igual ao PC Boost) |
| Alto desempenho | `presets` ∋ `high`, efeitos visuais `performance` | pergunta, vem marcado |
| Sem IA | `AI_SET` em `components/profiles/shared.ts`: `copilot_off`, `recall_off`, `click_to_do_off`, `recall_feature_off`, `paint_ai_off`, `notepad_ai_off`, `edge_ai_off` | nunca (só liga esses) |
| Padrão do Windows | nenhum | sempre: desliga tudo que está ligado, cada um pelo backup exato ou, sem backup, pelo padrão documentado |

  Padrão e Alto desempenho leem a tabela de presets do catálogo, então **ajuste novo no catálogo entra sozinho**. Ids do Sem IA que a versão não tem aparecem como "fora desta versão" no cartão.
- **Plano** (`profiles::plan`, função pura testada): dado o alvo `{ ids, fx_level, off_others }` e o estado real (`boost::detect_all`), devolve `steps` (ligar primeiro, desligar depois), `blocked` (precisa de administrador e o app não está elevado), `unknown`, `unavailable` e `already`. Ligar = tweak que não está `On` (Parcial completa); `visual_fx` conta como pronto só se o nível bate. Desligar o resto = tudo `On`/`Parcial` fora do perfil; o passo de `visual_fx` guarda o nível que tinha, para poder voltar.
- **Aplicar** (`ApplyModal`): pede o plano, mostra o que liga, o que desliga, o que fica de fora por falta de administrador, o que não existe aqui, e avisa quando um "desligar" **não tem backup** (grava o padrão do Windows). Antes do primeiro passo, com a opção `restore_before_preset` ligada e o app elevado, cria o ponto de restauração (`restore_create`). Cada passo roda sozinho (`profiles_step`): o mesmo caminho do cartão do PC Boost (`boost::apply`: backup antes, depois muda), então a barra de progresso é real e um passo que falha não derruba os outros. Resultado por ajuste (`needs_admin`, `cancelled`, erro). O aviso "Perfil aplicado" tem **Desfazer**: roda os passos ao contrário (ligados voltam pelo backup; desligados religam).
- **Exportar:** `Documents\G2 Boost\Perfis\perfil-<nome>.json` (a pasta Documentos vem de `SHGetKnownFolderPath`, que acompanha o OneDrive), mostrado no Explorer; o mesmo texto aparece numa janela com botão **Copiar**. `{ "g2boost_profile": 1, "name", "tweaks", "fx_level" }`. O prefixo `perfil-` mantém o nome longe de nomes de dispositivo (`CON`, `NUL`). Não escreve através de link: pasta `Perfis` ou `G2 Boost` que seja junction/symlink, ou arquivo no lugar do perfil que seja link, é recusado (o app pode estar elevado).
- **Importar:** escolher um arquivo da pasta `Perfis` (só nomes simples `*.json`, até 64 KB, arquivo comum, nunca link) ou colar o texto (também até 64 KB). Entrada hostil: tem de ser um objeto JSON; nome inválido é erro (o usuário vê o motivo), ids e nível inválidos são descartados, nome repetido ganha "(2)". O perfil importado só liga o que o usuário mandar aplicar.
- **Gravação:** `profiles_save/delete/import` mudam `settings.json` **e** a cópia viva (`Routines.settings`) juntas (`store()` em `profiles_cmds.rs`), e `settings::save` limpa a lista de novo (`sanitize_list`).

## Restauração

- **Ajustes ativos:** `boost::detect_all` filtrado para `On`/`Parcial`, com selo **Backup exato** ou **Sem backup** (o ajuste já estava assim antes do G2 Boost: desfazer grava o padrão do Windows) e **Precisa de administrador**. **Desfazer** (por linha) = passo `off` com origem `@undo`. **Desfazer tudo** abre uma janela com a lista, o aviso de quantos não têm backup e quantos ficam de fora sem administrador; confirmar roda os passos com origem `@undo_all`.
- **Outras mudanças guardadas** (somente leitura, cada uma com **Abrir** para a tela que desfaz; a tela é a rota, a aba está escrita no texto):

| Tipo | De onde vem | Desfaz em |
|---|---|---|
| `dns` | adaptadores com `dns::has_backup` | PC Boost, aba DNS |
| `gpu_driver` | `gpu_settings_backup.json` (lido como JSON simples: contagem e alvos, sem abrir o driver) | GPU |
| `gpu_pref` | backup `gpu_pref` (preferência de GPU por jogo) | Jogos |
| `exposure` | `exposure::reverts()` | Segurança |
| `qos` | `qos::list_ours()` (política `g2boost-*` no ActiveStore; some ao reiniciar) | Jogos |
| `apps_undo` | `apps::undo_status()` (última remoção de app) | Apps |
| `game_mode` | `gamemode::session()` (Modo Jogo em andamento) | Jogos |
| `peripherals` | originais guardados do mouse e teclado | Mouse e teclado |

- **Histórico** (`profiles-history.json` em `%LOCALAPPDATA%\G2 Boost`): só acrescenta; guarda os **últimos 300** registros; carregado como entrada hostil (elemento inválido é pulado, texto sem caractere de controle e com tamanho limitado, id só `[a-z0-9_]`). Registra o que passou por estas páginas: passos de perfil, desfazer e os ajustes de mouse e teclado (`periph_<ajuste>`), com data, origem (nome do perfil ou `@undo`, `@undo_all`, `@periph`), ligado/desligado e o erro quando falhou.

## Mouse e teclado (`peripherals.rs`)

`SystemParametersInfoW` com `SPIF_UPDATEINIFILE | SPIF_SENDCHANGE` (grava no perfil do usuário e avisa os programas, como o Painel de Controle). Sem admin. Valores vindos de fora são **cortados** para a faixa do Painel de Controle (`clamp`):

| Ajuste | API | Faixa | Padrão do Windows |
|---|---|---|---|
| Velocidade do ponteiro | `SPI_GET/SETMOUSESPEED` (valor em `pvParam`) | 1 a 20 | 10 (1:1 com a aceleração desligada) |
| Linhas por giro da roda | `SPI_GET/SETWHEELSCROLLLINES` | 1 a 100 (ou uma tela por vez, valor do Windows que o app só devolve) | 3 |
| Clique duplo | `GetDoubleClickTime` (user32, declarado à mão) + `SPI_SETDOUBLECLICKTIME` | 200 a 900 ms | 500 ms |
| Espera para repetir | `SPI_GET/SETKEYBOARDDELAY` | 0 a 3 (250 ms a 1 s) | 1 |
| Velocidade da repetição | `SPI_GET/SETKEYBOARDSPEED` | 0 a 31 (cerca de 2,5 a 30 por segundo) | 31 |

- **Backup:** a **primeira** mudança de cada ajuste guarda o valor que ele tinha (`backups/peripherals.json`, o formato `registry::Backup`, pseudo-caminho `spi:peripherals`). Mudar de novo não sobrescreve; voltar ao valor guardado remove o registro. **Voltar ao original** (por ajuste ou todos) escreve o guardado, de novo cortado para a faixa (o arquivo é do usuário, entrada hostil). Ordem: backup, depois muda.
- **Página:** card Mouse (velocidade com a marca 1:1, aceleração = tweak `mouse`, linhas da roda, clique duplo com área de teste, atalho para o **Teste do mouse**), card Teclado (espera e velocidade, caixa para testar a repetição, tweak `keyboard`) e **Atalhos que atrapalham no jogo**: `alt_shift_off`, `narrator_shortcut_off`, `accessibility_shortcuts_off`, ligados pela mesma API de tweaks. Tweak que a versão não tem fica desabilitado com aviso; os do catálogo passam pelo passo de perfil (`@periph`), então entram no histórico e podem ser desfeitos na Restauração. O controle só grava ao soltar (ponteiro ou tecla), não a cada passo do arrasto.
- `keyboard` (esperar 0, repetir 31) mexe nos mesmos valores dos controles; depois de ligar ou desligar o tweak a página lê tudo de novo.

## Comandos (IPC, `profiles_cmds.rs`)

`profiles_list/save/delete` · `profiles_plan` (só leitura) · `profiles_step` · `profiles_restoration` · `profiles_history` · `profiles_export` · `profiles_files` · `profiles_import` · `periph_state/set/revert`. Tudo que vem da UI é revalidado no core.

## Testes

- `profiles`: nome (limites, caracteres de controle e de direção), limpeza da lista (duplicados, 50 perfis, 200 ids, ids e níveis inválidos), `upsert` (renomear, nome repetido, limite), `unique_name`, exportar e importar (ida e volta; JSON hostil: grande demais, não objeto, tipos errados, aninhamento absurdo, nomes e ids maliciosos), nomes de arquivo, pasta de perfis (arquivos inválidos, grandes, pastas, `..`), **junction no lugar da pasta, da pasta pai e do arquivo** (nada é escrito atrás do link), plano (ligar, parcial, indisponível, desconhecido, bloqueado por administrador, desligar o resto, Windows padrão, nível dos efeitos), passo com id hostil, histórico (limite, ordem, limpeza, arquivo adulterado), resumo do backup da GPU, `settings.json` com perfis ruins. `peripherals`: faixas com valores extremos, valor "uma tela" só na roda, arquivo de backup hostil, ida e volta no armazenamento de backup, leitura sem admin.
- À mão (`--ignored`, mexem no PC de verdade e desfazem): `profiles_live_step_roundtrip` (liga e desliga "Mostrar extensões" pelo passo de perfil, confere plano, backup e histórico) e `peripherals_live_set_and_revert` (os cinco ajustes, com guarda que repõe os valores se algo falhar). Os dois passaram em 2026-10-25.
- E2E (`e2e/profiles.spec.ts`, `e2e/peripherals.spec.ts`): sem backend (modo navegador), cada tela mostra seus estados vazios sem erro no console; janelas de criar, aplicar, importar; busca por nome sem acento; área do clique duplo; atalhos travados; link para o Teste do mouse.

## Não verificado pelo agente / pendências

- **Com administrador:** aplicar e desfazer ajustes `HKLM`, de serviço, energia e tarefas pelo passo de perfil (o caminho é o do `boost::apply`, já verificado elevado em rodadas anteriores), o ponto de restauração antes do lote, e "Desfazer tudo" com ajustes de administrador.
- **Ids novos do catálogo** (`copilot_off` e os outros do Sem IA; `alt_shift_off`, `narrator_shortcut_off`, `accessibility_shortcuts_off`): vêm do bloco de ajustes; até lá os cartões mostram "fora desta versão" e os atalhos ficam desabilitados.
- `settings_set` (Configurações) devolve o `Settings` que a página tinha lido: se alguma página guardar uma cópia antiga durante a vida toda e salvar depois de perfis mudarem, leva a lista antiga. Hoje nenhuma faz isso (cada página lê ao abrir). Se um dia fizer, preservar `profiles` da cópia viva ali, como já é feito com `game_tweaks`.
