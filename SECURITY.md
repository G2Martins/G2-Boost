# Segurança — G2 Boost

## Reportar uma vulnerabilidade

Não abra issue pública. Use **Security › Report a vulnerability** (relato privado) no repositório
do GitHub ou fale diretamente com o autor, Gustavo Martins Gripaldi. Inclua versão, passos para reproduzir e impacto.
Resposta inicial em até 5 dias úteis; correções de severidade alta saem em release próprio.

Versões suportadas: a última release publicada.

## Modelo de ameaças (resumo)

| Origem não confiável | Defesa |
| --- | --- |
| Interface (webview), MCP, arquivos de configuração, nomes de arquivo/pasta, respostas de IA | Toda validação no core em Rust: ids só do catálogo, caminhos por `SafePath` (bloqueia System32/WinSxS, perfis de outros usuários, UNC, `..`, junções), limites em todo parâmetro numérico. |
| Ação destrutiva | Prévia + confirmação + token de uso único (limpeza) ou confirmação + Lixeira (disco) + histórico. Ajustes guardam backup exato antes de gravar. |
| Respostas da IA | Texto renderizado sem HTML; ações aceitas só se estiverem no catálogo (ajustes beta excluídos, no máximo 6 por lista) e revalidadas no Rust; a IA não executa nada. |
| Servidor MCP | Somente leitura; não aplica ajustes nem apaga arquivos. Strings devolvidas são dados, não instruções. |
| Rede | HTTP só pelo cliente Rust com lista de hosts permitidos; CSP restrita na webview. |
| Segredos | Chaves de IA no Gerenciador de Credenciais do Windows (BYOK); nunca em arquivo, log ou repositório. |
| Privilégio | Roda como usuário (`asInvoker`); caminhos que exigem admin retornam `needs_admin`; elevação só por UAC visível (`runas`), nunca por PowerShell. |
| Antivírus/heurística | Nenhum spawn de `cmd`/`powershell`/`reg`/`sc`/`net`; sem driver de kernel; sem empacotador; build assinado (ver Docs/release). |

Regras completas: [Docs/core/global-rules.md](Docs/core/global-rules.md) · Driver de sensores
(decisão de não instalar): [Docs/security/kernel-driver-options.md](Docs/security/kernel-driver-options.md).
