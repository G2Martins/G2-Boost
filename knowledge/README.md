# Base de conhecimento da Luli

**Última revisão:** 2026-10-18

Tudo nesta pasta (menos este README) entra no prompt da Luli, a assistente do G2 Boost, em toda conversa. É o manual da casa: como diagnosticar, o que é seguro, onde cada coisa fica no app. Editou aqui, a próxima build já usa.

## Regras para editar

- Um assunto por arquivo, em pt-BR, frases curtas. A Luli responde no idioma do usuário mesmo lendo isto em português.
- Só fato com fonte conhecida (Microsoft Learn/Suporte, fabricantes, Tom's Hardware, Gamers Nexus, TechPowerUp, Digital Foundry, PCGamingWiki). Número sem fonte não entra.
- Sem travessão como pontuação: a Luli imita o estilo do que lê.
- Nunca comandos de cmd, PowerShell ou regedit. O G2 Boost não entrega comandos: aponte a tela do app ou o caminho nas Configurações do Windows.
- As regras de segurança invioláveis ficam no código (`src/lib/ai.ts`), não aqui. Esta pasta orienta; não libera nada que o código proíbe.
- A pasta inteira vai em cada pergunta. Mantenha o total abaixo de ~25 KB.

## Como é carregada

`src/lib/knowledge.ts` junta os `.md` em ordem de nome (Vite `import.meta.glob`, embutido no build) e o prompt os coloca em `<knowledge>`. Arquivos começam com número para fixar a ordem.

O MCP (`G2 Boost.exe --mcp`) serve os mesmos arquivos pela tool `get_knowledge`, para o Claude Desktop, Cowork e Claude Code. Arquivo novo aqui precisa entrar na lista `KNOWLEDGE` de `crates/mcp/src/lib.rs` (um teste falha se esquecer).
