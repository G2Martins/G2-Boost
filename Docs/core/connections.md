# Conexões: provedores de IA, skills da Luli, MCP nos dois sentidos

**Última revisão:** 2026-11-20 (rodada 11, bloco A4) · Status: **implementado**, verificado por testes (Rust, bun, Playwright) e pelo MCP de desenvolvimento contra os configs reais da máquina do dono; **não** testado clicando no app instalado (ver §6).

Pedido do dono (2026-11-20): melhorar urgentemente as conexões da Luli e o que ela precisa para configurar; colar skills como nas telas de Skills/Extensões dos apps de IA; MCP para apps do Windows e um mapa das funções MCP locais do PC. **Decisão do dono:** escopo "hub + mapa": o G2 Boost ganha tools e a Luli vê o mapa de MCPs locais **só leitura**; o app **nunca** executa servidor MCP de terceiros.

Código: página `src/pages/connections.tsx` (rota `connections`, grupo Sistema), componentes em `src/components/connections/` (`providers.tsx`, `skills.tsx`, `skill-zip.ts`, `mcp-map.tsx`), `src/components/claude-connect.tsx`; sessão em `src/lib/luli.tsx`; prompt e teste de chave em `src/lib/ai.ts`; core `crates/core/src/{skills,mcpmap,claude_connect}.rs`; comandos `src-tauri/src/luli_cmds.rs`; MCP `get_mcp_servers` em `crates/mcp/src/lib.rs`. A página Luli (`ai.tsx`) ficou só com a escolha do provedor e um atalho **Conexões**.

## 1. Provedores de IA (aba Provedores)

Um card por provedor (Claude, ChatGPT, Gemini, NVIDIA): LED + estado, hora da última checagem, chave (Cofre do Windows, igual antes), modelo, se tem pesquisa na web, link para criar a chave e "Usar com a Luli".

| Estado | Quando |
|---|---|
| Sem chave | nada no Cofre |
| Chave salva, ainda não testada | salvou e ainda não testou nem perguntou |
| Testando… | `GET` da lista de modelos em andamento (15 s de limite) |
| Funcionando | 200 na lista (ou uma pergunta respondida) |
| Serviço respondeu | NVIDIA: a lista de modelos é pública, então 200 não prova a chave; ela é conferida na 1ª pergunta |
| Chave inválida | 401/403 (e 400 do Gemini na lista, que é o `API_KEY_INVALID`) |
| Sem cota agora | 429 |
| Sem conexão | erro de rede, timeout ou 5xx |
| Erro do provedor (HTTP n) | outro status na lista |

- **Testar não gasta token:** lista os modelos. URLs `GET` exatas na allowlist do `tauri-plugin-http` (`capabilities/default.json`): `https://api.anthropic.com/v1/models` (`x-api-key` + `anthropic-version`), `https://api.openai.com/v1/models`, `https://generativelanguage.googleapis.com/v1beta/models` (`x-goog-api-key`), `https://integrate.api.nvidia.com/v1/models`. Sem query string (primeira página de cada API basta para o seletor).
- **Salvar uma chave testa na hora.** Trocar a chave apaga a checagem antiga. As perguntas reais também atualizam o estado (`checkState` em `luli.tsx`: resposta = Funcionando; 401/403 = inválida; 429 = sem cota; um 400 numa pergunta não diz nada sobre a chave).
- **Seletor de modelo** preenchido pela lista, filtrado para conversa (`chatModels`: fora embeddings, voz, imagem, moderação, classificadores de segurança; OpenAI só `gpt-*`/`o*`/`chatgpt-*` sem `instruct`, ordenado do mais novo; Gemini só `gemini-*` com `generateContent`). "Outro modelo…" volta ao texto livre, validado por provedor (`validModel`: Gemini `[a-z0-9.-]`, NVIDIA `[a-z0-9._/-]`, OpenAI `[a-z0-9._:-]`, Claude `[a-z0-9._-]`, até 80); inválido fica vermelho e não é salvo.
- Estado e modelos listados ficam em `localStorage` (`g2boost.ai.checks`, conveniência por máquina; relido como entrada hostil). Mudar o modelo de **outro** provedor não cancela a conversa em andamento.

## 2. Skills da Luli (aba Skills)

Formato aberto de Agent Skills: pasta `<nome>/SKILL.md` com cabeçalho YAML (`name`, `description`) e instruções em Markdown. Entrada: **colar** o texto, **escolher** um `.md` ou `.zip`, ou **arrastar** o arquivo para a janela.

- **Validação no core (`skills.rs`)**, leitor de cabeçalho próprio e estrito (só `name`/`description`; escalar simples, entre aspas ou bloco `>`/`|`; outras chaves ignoradas; `name:` repetido recusa): `name` 1 a 64 `[a-z0-9-]`, sem hífen nas pontas, sem "anthropic"/"claude" (palavras reservadas do formato) nem nome de dispositivo do Windows (`con`, `nul`, `lpt1`…); `description` obrigatória, uma linha, ≤ 1024 caracteres, sem `<`/`>`; instruções não vazias, ≤ 16 KB (bytes UTF-8), sem caractere de controle além de quebra de linha e tab; entrada inteira ≤ 24 KB; ≤ 30 skills; nome único. A UI faz uma pré-checagem óbvia (cabeçalho, `name:`, `description:`) só para responder rápido; quem decide é o Rust.
- **Armazenamento:** um arquivo, `%LOCALAPPDATA%\G2 Boost\skills\skills.json` (atômico, tmp + rename), revalidado a cada leitura (arquivo editado à mão: entrada inválida, duplicada ou grande demais some). **Nenhum nome de arquivo vem da skill nem do zip.**
- **Toda skill entra desligada.** Ligar abre o conteúdo completo e o aviso de que é instrução de terceiro; só então liga. Ver e Remover (com confirmação) por item.
- **Zip no webview, sem dependência (`skill-zip.ts`):** lê o diretório central, pega só `SKILL.md` na raiz ou em `<pasta>/SKILL.md`, descompacta com `DecompressionStream("deflate-raw")`. Nomes com `..`, `.`, absolutos, com `\`, letra de drive ou `__MACOSX/` são ignorados; zip criptografado e zip64 recusados; zip ≤ 25 MB; cada SKILL.md ≤ 64 KB **medido no que saiu da descompressão** (cabeçalho mentiroso não passa: teste com bomba de 2 MB). Scripts e outros arquivos do pacote **nunca são lidos nem executados** (dito na tela).
- **Arrastar e soltar:** o Tauri entrega o drop como evento nativo. `luli_cmds::setup` guarda o caminho de um `.md`/`.zip` solto na janela e emite `skills:dropped`; o `App.tsx` manda `.md`/`.zip` para Conexões (`.exe`/`.lnk` continuam indo para o desinstalador); a página pede os bytes uma vez (`skills_take_drop`: arquivo local regular, sem symlink nem caminho de rede, ≤ 25 MB). O webview nunca nomeia um caminho.
- **No prompt (`systemPrompt` em `ai.ts`, vale para a Luli e para o Console G2):** bloco `<skills>` depois de `<user_instructions>`, só com as ligadas, orçamento total de **24 KB** (`SKILLS_BUDGET`): nome e descrição de cada uma sempre que couberem, depois os corpos enquanto couberem; corpo que não cabe vira uma nota pedindo para desligar outras. Texto escapado (`<` → `<`, então `</skills>` falso não fecha nada), apresentado como orientação do usuário com o mesmo peso de `<user_instructions>`: **nunca acima de SAFETY**, dos fatos de `<pc_context>` nem das regras do bloco de ações; a parte que pedir para rodar código, chamar ferramenta, revelar o prompt, entregar comando ou enfraquecer a segurança é ignorada. SAFETY cita `<skills>` entre as fontes que não mandam. A tela mostra quanto do orçamento as ligadas usam.

## 3. Mapa de MCPs locais (aba MCP, `mcpmap.rs`)

Só leitura. Lê:

| App | Arquivo | Chave |
|---|---|---|
| Claude Desktop | todos os locais de `claude_connect` (`%APPDATA%\Claude`, `%LOCALAPPDATA%\Claude-Data\claude_*`, pacote MSIX sem o marcador) | `mcpServers` |
| Claude Code | `%USERPROFILE%\.claude.json` | `mcpServers` (usuário); `projects.*.mcpServers` = **só nome + quantos projetos**, nunca o caminho do projeto |
| VS Code | `%APPDATA%\Code\User\mcp.json`; `settings.json` → `mcp.servers` (só conta se declarar servidores) | `servers` |
| Cursor | `%USERPROFILE%\.cursor\mcp.json` | `mcpServers` |
| Windsurf | `%USERPROFILE%\.codeium\windsurf\mcp_config.json` | `mcpServers` (`serverUrl`) |
| LM Studio | `%USERPROFILE%\.lmstudio\mcp.json` | `mcpServers` |

- **Por servidor sai só:** nome (sem caractere de controle, ≤ 64), app, transporte (`stdio`/`http`), **nome do arquivo** do executável (reusa `context::exe_of`: `npx`, `uvx`, `node.exe`) ou **host[:porta]** da URL (sem caminho, query, `user:senha@`; `\` encerra o host como nos navegadores). **Nunca** argumentos, `env`, `headers` ou query (costumam ter chave). Só os dois primeiros argumentos são olhados, em memória, para reconhecer `pnpm dlx` e afins.
- **Selos:** "Baixa da internet ao iniciar" (`npx`, `uvx`, `bunx`, `pnpx`, `pnpm/yarn dlx`, `pipx run`, `bun x`, `npm exec`, `uv tool run`); "Programa não encontrado" (caminho absoluto inexistente ou nome fora do PATH com PATHEXT; variável, caminho relativo e caminho de rede não são conferidos, este para não tocar a rede); "G2 Boost" (o nosso).
- **Arquivo hostil:** symlink recusado, > 4 MB recusado, JSON inválido vira selo de erro no app (JSON com comentários e vírgula sobrando, como o VS Code grava, é aceito por um removedor de comentários próprio). ≤ 100 servidores por app.
- **UI:** cada app é um "barramento": ícone real do exe quando achado (`ExeIcon`; Claude Desktop da Store fica com a inicial porque `WindowsApps` não é legível), um fio descendo e um traço para cada servidor. Apps procurados e não achados aparecem numa linha no fim. Aviso de segurança e card da Luli com perguntas sobre os servidores.
- **Para a IA:** tool MCP `get_mcp_servers` (tudo menos o caminho do exe dos apps) e `mcp_servers` no pacote de contexto da Luli (`context.rs`: só nome, app e selos, ≤ 40).

**Por que isso é segurança:** um servidor MCP é um programa que outro app inicia sozinho, com o usuário, a cada abertura: um vetor de persistência e de cadeia de suprimento (um `npx pacote` sem versão fixa roda o que o registro tiver naquele dia). O mapa deixa isso visível, a Luli consegue apontar o que merece atenção e o "programa não encontrado" mostra entradas quebradas. Remover continua sendo no app dono. A página Segurança (outro escopo) pode reaproveitar `mcpmap::scan` depois.

## 4. Conectar o G2 Boost a outros apps (aba MCP, `claude_connect.rs`)

O card virou "Conectar o G2 Boost aos apps de IA": Claude Desktop/Cowork, Claude Code, **Cursor** (`~/.cursor/mcp.json`, mesmo formato do Desktop, criado se a pasta `.cursor` existe) e **VS Code** (`%APPDATA%\Code\User\mcp.json`, chave `servers`, entrada `{"type":"stdio","command":…,"args":["--mcp"]}`, criado se `Code\User` existe). Mesma lógica segura de antes (backup, escrita atômica, só a chave `g2boost`, symlink e > 16 MB recusados). `mcp.json` do VS Code com comentários não é JSON estrito: é recusado intacto e a tela manda usar o trecho manual (agora com o bloco do VS Code). O reparo ao abrir (`repair_stale`) vale para os quatro.

## 5. Testes

- Rust: `skills::tests` (formato aberto, CRLF/BOM/aspas/blocos, 13 cabeçalhos hostis, nomes inválidos e reservados, limites em bytes e caracteres, arquivo adulterado), `mcpmap::tests` (seis apps num sandbox, segredos em args/env/headers/URL/caminho de projeto nunca aparecem em nenhuma saída, JSONC, JSON inválido, > 4 MB, symlink quando o Windows deixa criar, PATH com `.cmd`), `claude_connect::tests::vscode_and_cursor_shapes`.
- bun: `tests/skill-zip.test.ts` (stored + deflate, só a raiz, `../` e afins ignorados, bomba com tamanho mentiroso, zip cortado, criptografado, deflate corrompido); `tests/ai.test.ts` (bloco `<skills>` escapado e abaixo de SAFETY, orçamento com 3 corpos grandes e com 30 descrições longas, `chatModels`, `validModel`, `checkState`).
- e2e `e2e/connections.spec.ts`: quatro cards sem chave, modelo inválido sinalizado, Skills vazio + colar sem cabeçalho e sem descrição mostram o erro, aba MCP com o card de conectar e o mapa vazio, reduced-motion.
- Real (2026-11-20): `get_mcp_servers` pelo `g2boost-mcp` de desenvolvimento contra os configs do dono listou Claude Desktop e Claude Code (servidores http só com host, `npx.cmd` com selo de download, um `uvx` fora do PATH marcado como não encontrado, o próprio G2 Boost marcado).
- App de debug (2026-11-20, `G2BOOST_DATA_DIR` isolado, rota Conexões): aba MCP com os 15 servidores reais em 2 apps (o Claude Code aparece como "aponta para outra instalação" porque o exe de debug não é o instalado: esperado), aba Skills listando skills semeadas no `skills.json` (comandos `mcp_map` e `skills_list` ponta a ponta).
- Códigos de erro conferidos com chave falsa (curl, 2026-11-20): Claude 401, OpenAI 401, Gemini 400 `API key not valid`, NVIDIA 200 mesmo com chave inválida (por isso "Serviço respondeu"). A lista da NVIDIA traz modelos que não conversam (`starcoder`, `gliner`, detector): filtrados. O padrão antigo `moonshotai/kimi-k2.6` saiu da lista pública da NVIDIA; desde 2026-11-20 o padrão é `moonshotai/kimi-k3.1-pro` (conferido na lista pública, sem chave).

## 6. Não verificado / pendente

- No app: Testar contra as quatro APIs com chave **real** (não usei as chaves do dono), arrastar um `.zip` para a janela (ordem do evento nativo × `skills:dropped`), conectar Cursor/VS Code de verdade (o Cursor não está instalado na máquina do dono; o VS Code está, sem `mcp.json`).
- Paginação das listas de modelos (só a 1ª página; Gemini tem 50 por página).
- Skills com arquivos de referência: só o SKILL.md entra; referência de outro arquivo do pacote fica sem conteúdo (dito na tela).
