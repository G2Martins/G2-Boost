# Segurança › Antivírus (produtos, liberação no Defender, guias por fabricante)

**Última revisão:** 2026-10-25 · Rodada 9, bloco `maint` · Código: `crates/core/src/antivirus.rs`; shell `src-tauri/src/maint_cmds.rs` (`av_status`, `av_exclude`, `av_unexclude`, `av_reveal_exe`); UI `src/components/security/antivirus-tab.tsx` (aba **Antivírus** da página Segurança, rota `security`, aba `antivirus`; a auditoria de exposição ficou na aba **Exposição**, ver [exposure-audit.md](./exposure-audit.md)).

**Regra:** o G2 Boost **nunca desliga nem enfraquece** antivírus (dito na tela). A única mudança possível é excluir o **próprio exe** das verificações do Microsoft Defender, com confirmação, e só quando isso é seguro. Para antivírus de terceiros, só um guia escrito; nada é alterado neles.

## O que a aba mostra

1. **Antivírus deste PC**: `root\SecurityCenter2 AntiVirusProduct` (sem admin, prazo de 5 s, até 16 linhas): `displayName`, `instanceGuid`, `pathToSignedProductExe`, `productState`. Estado pelo mesmo decodificador da auditoria (`exposure::product_state`: bits 12 a 15 = ligado/desligado/pausado/vencido; bits 4 a 7 = definições). Registros repetidos do mesmo produto (versões antigas deixam entradas; nesta máquina o Kaspersky aparece 4 vezes) viram uma linha, a melhor. Defender = GUID `{D68DDC3A-831F-4fae-9E44-DA132C1ACF46}` ou `windowsdefender://`. Com outro antivírus ligado, o Defender aparece "de reserva".
2. **Liberar o G2 Boost no Microsoft Defender** (só quando o Defender está ativo, ou se a exclusão já existe).
3. **Guia do antivírus**: passos curtos por fabricante, escolhido pelo produto ativo (`vendor_of`: palavra inteira, prefixo só para nomes longos, ex. "NortonLifeLock"), com seletor para os outros; botões **Copiar caminho do exe** e **Mostrar na pasta** (`disk::reveal` do exe atual, sem argumento do chamador).
4. **Aviso do SmartScreen**: "O Windows protegeu o computador" → Mais informações → Executar assim mesmo (o instalador ainda não tem assinatura).

## Liberação no Defender

- **Como**: WMI `ROOT\Microsoft\Windows\Defender`, métodos estáticos `MSFT_MpPreference.Add` / `Remove` com `ExclusionPath = [exe]` (o provedor por trás de `Add-MpPreference`; parâmetros conferidos na classe desta máquina). `ReturnValue` ≠ 0 = falha. Depois, **leitura de volta** de `ExclusionPath` (só elevado: sem admin o Defender devolve um texto de "precisa ser administrador"); sem o caminho na lista = `not_applied` (política de empresa pode controlar exclusões).
- **Só o arquivo**: nada de pasta, nada de `ExclusionProcess` (que pularia todo arquivo que o app abre).
- **Quando é oferecida** (`block_reason`, reavaliada no core a cada `av_exclude`, nunca vinda da UI):
  1. o Defender é o antivírus ativo (`defender_inactive` se outro estiver ligado);
  2. o exe (`current_exe`, `\\?\` removido, `.` normalizado) está **estritamente dentro** de Program Files ou Program Files (x86), pastas lidas por `SHGetKnownFolderPath` (não por variável de ambiente), comparando componente a componente, sem `..` (`not_protected_folder`);
  3. o exe e **cada pasta até Program Files** pertencem a SYSTEM, Administradores ou TrustedInstaller e só eles têm direito de escrita (`GetNamedSecurityInfoW` dono + DACL; grants que contam: gravar/anexar, apagar filho, apagar, `WRITE_DAC`, `WRITE_OWNER`, `GENERIC_ALL`/`GENERIC_WRITE`; herança-só ignorada; DACL nula ou ilegível = gravável) (`user_writable`).
- **Por quê**: uma exclusão vale para o caminho. Numa pasta que um programa comum pode alterar (cópia portátil, Downloads, instalação por usuário em `%LOCALAPPDATA%`), outro programa trocaria o arquivo e herdaria a liberação.
- **Achado importante**: o instalador NSIS atual usa `installMode: "currentUser"` (`%LOCALAPPDATA%\G2 Boost`), então a liberação é **recusada por desenho** em toda instalação de hoje; a tela explica e sugere instalar em Arquivos de Programas. Para ela aparecer, o instalador precisaria de `perMachine` (decisão do dono: muda o UAC da instalação e o caminho de atualização).
- **Desfazer**: `Remove` com o caminho do exe atual (só ele), leitura de volta. Log em `%LOCALAPPDATA%\G2 Boost\antivirus-log.json` (data + `add`/`remove`, sem caminhos).
- Admin: sem elevação = `needs_admin` e a tela mostra Elevar (volta na aba `antivirus`).

## Guias por fabricante

Conferidos nas páginas oficiais de ajuda em 2026-10-25 (`av.guide.<fabricante>.<n>`, pt-BR e en):

| Fabricante | Fonte | Situação |
|---|---|---|
| Kaspersky (Standard/Plus/Premium, 21.x) | support.kaspersky.com 21.22/227390 (en e pt-BR) | conferido |
| Avast / AVG (mesmos menus, um guia) | support.avast.com e support.avg.com "scan exclusions" (en e pt-BR) | conferido |
| ESET (NOD32 / Smart Security / Ultimate 19) | help.eset.com `idh_performance_exclusions` (en e pt-BR) | conferido; usa exclusão de desempenho (arquivo), não de processo |
| Norton 360 | support.norton.com kb20100222230832 (en e pt-BR) | conferido |
| Bitdefender (interface clássica) | bitdefender.com answer 13427 (en e pt-BR) | conferido; a interface nova não pôde ser lida |
| Trend Micro | helpcenter.trendlife.com tmka-14498 (só en) | conferido; rótulos pt-BR traduzidos com o original entre parênteses |
| Malwarebytes 5.x | help.malwarebytes.com (403 para leitura automática; só o resumo da busca) | parcial; rótulos com o original em inglês |
| McAfee | páginas oficiais inacessíveis | não conferido: passos genéricos com os nomes em inglês |
| Panda Dome | ajuda oficial sem o caminho até as configurações avançadas | parcial |

O cartão sempre lembra que os nomes mudam entre versões (procurar Exclusões, Exceções, Lista de permissões) e o guia "Outro antivírus" cobre o resto (adicionar arquivo, não pasta; restaurar da quarentena).

## Testes (`antivirus::tests`)

- `productState` com valores reais desta máquina (Kaspersky ligado 266240, Defender desligado 393472) e conhecidos do Defender (ligado, definições velhas, pausado, vencido).
- Agrupamento (4 registros do Kaspersky = 1), quem está ativo (Defender sozinho, Defender + terceiro, desligado, lista vazia), GUID em qualquer caixa.
- Nomes de fabricante (inclusive "AVGuard" e "Reset" que **não** casam).
- Regra: Program Files sim; Downloads, `%LOCALAPPDATA%`, "Program Files Evil", `..`, UNC, outro disco, a própria raiz e caminho relativo = `not_protected_folder`; pasta ou exe graváveis = `user_writable`.
- Permissões com o formato real (SDDL) de `C:\Program Files` e de uma pasta criada por instalador elevado; Usuários com modificar, dono usuário, Usuários autenticados, Todos, só atributos (não conta), herança-só (não conta), DACL nula.
- Permissões reais, sem admin: Program Files e `kernel32.dll` fechados, `%TEMP%` gravável, caminho inexistente = recusado; o binário de teste nunca recebe a oferta; sem admin, `exclude`/`unexclude` = `needs_admin`. `#[ignore]` `live_antivirus_status` (só leitura).

## Não verificado (precisa do dono)

Adicionar/remover a exclusão de verdade (exige Defender como antivírus ativo, app instalado em Program Files e elevação; nesta máquina o Kaspersky está ativo). Para testar: instalar com `perMachine` numa VM só com o Defender, abrir elevado, Liberar → conferir em Segurança do Windows › Exclusões → Desfazer.
