# G2 Boost

**Desempenho para jogos no Windows, sem truque e sem risco escondido.** O G2 Boost junta num só
app o que normalmente exige várias ferramentas: diagnóstico do hardware, limpeza do Windows,
debloat, analisador de disco, rede, jogos e ajustes de desempenho, tudo com prévia, backup e um
clique para desfazer.

Windows 10 e 11 (x64) · Tauri 2 (Rust + React) · pt-BR e English · sem telemetria · sem driver de kernel.

---

## O que ele faz

| Área | O que você ganha |
| --- | --- |
| **Painel** | Pontuação de otimização calculada para o *seu* hardware, **Boost agora** (ajustes recomendados + limpeza + RAM, com confirmação e ponto de restauração), pendências, monitor em tempo real e ações recomendadas. |
| **Pós-formatação** | Acabou de formatar? Apps em lote, debloat, ajustes recomendados e extras de conforto numa tela só. |
| **Meu PC + Check-up** | Processador, memória (cada pente, XMP), placa de vídeo, monitores, discos, BIOS visível pelo Windows, nota do WinSAT e um check-up de ~20 medições com o que corrigir. |
| **Monitor de hardware** | Árvore de sensores por dispositivo (cada núcleo, GPU, memória, discos, rede) com mínimo, máximo e exportação em CSV. Temperatura da CPU quando o HWiNFO ou o LibreHardwareMonitor estão abertos. |
| **Manutenção** | Revisão semanal num clique (disco, arquivos do sistema, imagem do Windows), reparo completo, redefinir o Windows Update, destravar a impressora, reiniciar o áudio, verificar e otimizar unidades. |
| **Segurança** | Auditoria de exposição (RDP, SMBv1, portas abertas no roteador, apps de acesso remoto) e **Antivírus**: detecta o que está ativo, libera o G2 Boost no Defender com confirmação ou mostra o passo a passo do seu antivírus. |
| **Debloat** | Telemetria, anúncios, Widgets, Copilot, Recall e outros extras que o Windows liga sem perguntar, em níveis Essencial, Recomendado e Avançado, mais a remoção de apps pré-instalados (reinstaláveis pela Store). |
| **PC Boost** | Mais de 60 ajustes reversíveis com busca, filtros e **Ver o que muda** (as chaves e serviços exatos), presets, energia, inicialização, processos, limpeza de RAM. |
| **Perfis e restauração** | Perfis com nome (Sem IA, Padrão do Windows e os seus), exportar e importar, e a lista de tudo que o app mudou com **Desfazer** um a um ou tudo de uma vez. |
| **Mouse e teclado** | Velocidade do ponteiro (com a marca 1:1), aceleração, repetição do teclado e os atalhos que atrapalham no jogo (Alt+Shift, Narrador, teclas de aderência). |
| **Jogos** | Biblioteca com capas de todos os launchers, **Preparar e jogar** (Modo Jogo + o jogo abre como usuário normal), núcleos e GPU por jogo, prioridade, rota até o servidor. |
| **Overlay de jogo** | FPS, tempo de quadro e 1% low lidos pelo próprio Windows, CPU, GPU e temperaturas numa janela por cima do jogo (F7). Nada é injetado no jogo. |
| **GPU** | Configurações 3D do driver NVIDIA (globais e por jogo) e AMD (beta), com backup. |
| **Apps** | Instalar apps de um catálogo curado (winget, fontes oficiais), atualizar tudo, desinstalar com limpeza de sobras. |
| **Limpeza e disco** | Temporários, logs, caches de shaders e de apps, Windows Update e arquivos do sistema com prévia e histórico; mapa do disco (treemap), filtros e duplicados. |
| **Rede e ferramentas** | Monitor de latência/jitter/perda, teste de velocidade com bufferbloat, perda de pacotes, MTR/Ping/TCPing, DNS mais rápido, dispositivos na rede, teste do mouse (polling) e teste de monitor. |
| **Luli (IA) e MCP** | Pergunte sobre *este* PC ao Claude, ChatGPT ou Gemini (sua chave); `G2 Boost.exe --mcp` leva o diagnóstico ao Claude Desktop e ao Claude Code. A IA nunca executa nada sozinha. |

## Baixar e instalar

Na página de **Releases** do GitHub há três formas, todas com a mesma versão:

| Arquivo | Para quem |
| --- | --- |
| `G2-Boost_<versão>_x64-setup.exe` | **Recomendado.** Instala em Arquivos de Programas para todos os usuários (pede administrador uma vez) e já traz o instalador do WebView2. |
| `G2-Boost_<versão>_x64_pt-BR.msi` / `_en-US.msi` | O mesmo app em MSI, para quem instala por política (GPO, Intune) ou prefere o Windows Installer. |
| `G2-Boost_<versão>_x64-portable.zip` | Sem instalar: extraia numa pasta (ou pendrive) e abra o `G2 Boost.exe`. Configurações e backups ficam na pasta `data` ao lado do exe. |

Confira o arquivo baixado com o `SHA256SUMS.txt` da mesma release:
`Get-FileHash .\G2-Boost_<versão>_x64-setup.exe -Algorithm SHA256`.

**Requisitos:** Windows 10 22H2 ou Windows 11, 64 bits, e o WebView2 (já vem no Windows 11 e chega pelo Windows Update no Windows 10; o instalador cuida dele). Internet só para teste de velocidade, capas de jogos, instalar apps e a IA.

### "O Windows protegeu o computador"

O G2 Boost ainda não tem assinatura digital paga, então o SmartScreen avisa na primeira execução.
Clique em **Mais informações** e depois em **Executar assim mesmo**. Pelo mesmo motivo, a janela de
permissão do Windows mostra "Fornecedor desconhecido"; antes dela, o próprio G2 Boost explica por
que pede administrador.

## Perguntas frequentes

**É seguro? Dá para desfazer?** Sim. Cada ajuste mostra o que muda (chaves de registro, serviços,
tarefas) e guarda o valor original antes de gravar. Em **Perfis e restauração** você desfaz um a
um ou tudo de uma vez. Antes de mudanças em lote, o app oferece um ponto de restauração do Windows.
Limpezas mostram a prévia em bytes e pedem confirmação.

**Pode dar ban em jogo com anti-cheat?** O G2 Boost não injeta nada nos jogos e não instala driver.
O overlay é uma janela separada por cima do jogo e o FPS vem do rastreamento de eventos do próprio
Windows. Os ajustes por jogo usam APIs normais do Windows (prioridade, núcleos, preferência de GPU).

**O antivírus reclamou.** Na aba **Segurança › Antivírus** o app mostra o antivírus ativo e o passo
a passo para marcar o G2 Boost como confiável, sem desligar a proteção. No Microsoft Defender ele
mesmo faz isso, com confirmação e desfazer (só na versão instalada).

**Por que pede administrador?** Ajustes de sistema, limpezas de áreas do Windows e reparos mexem em
pastas e chaves protegidas. Você pode desligar "Sempre abrir como administrador" em Configurações:
o app abre em modo limitado e pede permissão só quando precisar.

**Desliga o Windows Update, o Defender ou o firewall?** Não. Também não mexe no UAC nem limpa o
Prefetch. O debloat só desliga extras (telemetria, anúncios, recursos de IA) e tudo volta atrás.

## Segurança e privacidade

- **Nada muda sem você ver antes.** Prévia, confirmação, backup e desfazer em tudo que altera o sistema.
- **Nativo.** Tudo por API do Windows: nenhum `cmd`, PowerShell, `reg` ou `sc` por baixo (exceções documentadas: `winget.exe`, `sfc.exe`, `mrt.exe`, só por clique).
- **Menor privilégio.** O que precisa de administrador avisa e pede elevação pelo UAC; o resto roda como usuário comum.
- **Sem driver de kernel e sem telemetria.** Diagnósticos ficam no PC. Chaves de IA ficam no Gerenciador de Credenciais do Windows.

Detalhes e como reportar vulnerabilidades: [SECURITY.md](SECURITY.md).

## Usar com o Claude (MCP)

```bash
claude mcp add g2boost -- "C:\Program Files\G2 Boost\G2 Boost.exe" --mcp
```

O app mostra o comando com o caminho certo em **Luli › Usar no Claude Desktop / Claude Code**.

## Desenvolvimento

Pré-requisitos: Rust estável (MSVC), [Bun](https://bun.sh), WebView2. Toolchain e caches podem
ficar fora do C:, ver [Docs/core/architecture.md §8](Docs/core/architecture.md).

```bash
bun install
bun run tauri:dev     # app completo com hot reload (a primeira compilação do Rust leva alguns minutos)
bun run dev           # só a interface no navegador (comandos nativos retornam vazio)
bun run check         # typecheck + Biome + clippy + testes (bun, cargo, Playwright): verde antes de fechar fase
bun run release       # instalador NSIS + MSI + zip portátil + SHA256SUMS em release/
```

| Pasta | Conteúdo |
| --- | --- |
| `src/` | Interface React 19 + TypeScript + Tailwind 4 (páginas, componentes, i18n em `locales/`, overlay em `overlay/`) |
| `src-tauri/` | Shell Tauri: comandos IPC finos, bandeja, overlay, modo `--mcp` |
| `crates/core/` | Toda a lógica e as validações (Win32, disco, limpeza, rede, ajustes, hardware, jogos) |
| `crates/mcp/` | Servidor MCP (biblioteca usada pelo app e binário de desenvolvimento) |
| `tests/` · `e2e/` | Testes da interface (`bun test`) e de ponta a ponta (Playwright) |
| `tools/` | Utilitários de build e de release |
| `Docs/` | Documentação viva: comece por [Docs/README.md](Docs/README.md) |

Regras de engenharia: [Docs/core/global-rules.md](Docs/core/global-rules.md) · Roadmap:
[Docs/core/next-steps.md](Docs/core/next-steps.md) · Histórico de versões: [CHANGELOG.md](CHANGELOG.md).

## Licença

Software proprietário. © 2026 Gustavo Martins Gripaldi. Todos os direitos reservados.
