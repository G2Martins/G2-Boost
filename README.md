# G2 Boost

**Desempenho para jogos no Windows, sem truque e sem risco escondido.** O G2 Boost junta num só
app o que normalmente exige cinco ferramentas: diagnóstico completo do hardware, limpeza do
Windows, analisador de disco, ferramentas de rede e ajustes de desempenho — tudo com prévia,
backup e um clique para desfazer.

Windows 10 e 11 (x64) · Tauri 2 (Rust + React) · pt-BR e English · sem telemetria.

---

## O que ele faz

| Área | O que você ganha |
| --- | --- |
| **Painel** | Pontuação de otimização calculada para o *seu* hardware, pontos de restauração (ver e criar), quanto dá para limpar, uso de RAM e ações recomendadas. |
| **Monitor em tempo real** | CPU (carga, clock efetivo, potência), GPU (carga, VRAM, temperatura, clocks, potência), RAM, disco e rede, a cada segundo. Temperatura da CPU aparece quando o HWiNFO ou o LibreHardwareMonitor estão abertos — o G2 Boost não instala driver. |
| **Meu PC** | Processador (família/modelo/stepping, caches, instruções, microcode), cada pente de memória (tipo, velocidade configurada × nominal, fabricante), placa de vídeo (VRAM, driver, PCIe, limite de potência), monitores (resolução e taxa atual × máxima), discos (SSD/HD, NVMe/SATA), placa-mãe, **configurações da BIOS visíveis pelo Windows** (UEFI/legado, Secure Boot, TPM, virtualização, Resizable BAR, XMP/EXPO, idade da BIOS), nota do WinSAT e relatório em arquivo. |
| **Conselheiro** | Achados objetivos com números: XMP desligado, RAM em canal único, monitor abaixo da taxa máxima, Resizable BAR desligado, Windows em HD/SATA com NVMe sobrando, disco cheio, CPU/GPU quente, driver antigo, BIOS antiga, Secure Boot/TPM, e sugestões de upgrade. |
| **PC Boost** | Presets **Padrão / Alto desempenho / Personalizado** com ajustes reversíveis: modo de jogo, input lag, otimizações para jogos em janela, mouse e teclado, efeitos visuais em 4 níveis, telemetria e serviços, plano de energia, HAGS, MMCSS, latência de rede. Cada ajuste guarda os valores originais antes de mudar. |
| **Inicialização** | Liga/desliga o que abre com o Windows (mesmo mecanismo do Gerenciador de Tarefas, nada é apagado) e mostra os processos mais pesados agora. |
| **Limpeza de RAM** | Libera a memória em espera (standby) sob demanda ou ao abrir um jogo. |
| **Limpeza** | Temporários, logs e relatórios de erro, caches de shaders (NVIDIA/AMD/DirectX), miniaturas, caches de navegadores e apps, Windows Update, limpeza de componentes, Lixeira — com prévia em bytes, confirmação e histórico. |
| **Analisador de disco** | Mapa (treemap) de qualquer unidade, filtros (`*.mp4 >1gb >6m`), mostrar na pasta, mandar para a Lixeira com confirmação e explicação dos arquivos grandes do sistema (pagefile, hiberfil, WinSxS, Windows.old…). |
| **Rede** | Monitor de latência, jitter e perda (gateway, DNS e servidores de jogos), teste de velocidade com bufferbloat, ping para servidores Steam (CS2, Dota 2, Deadlock), MTR, Ping e TCPing, e **DNS Optimizer** (mede 10 provedores e aplica o mais rápido, com restauração). |
| **Rotinas** | Iniciar com o Windows na bandeja, limpeza agendada (ao ligar, a cada N horas, diária, semanal) e limpeza de RAM ao abrir um jogo. |
| **Assistente IA** | Pergunte sobre *este* PC ao Claude, ChatGPT ou Gemini (com a sua chave). A IA recebe um resumo sem dados pessoais, cita benchmarks de sites confiáveis, não inventa FPS e **nunca executa nada**: sugestões viram botões que passam pelo mesmo fluxo de prévia e backup. |
| **MCP** | `G2 Boost.exe --mcp` expõe o diagnóstico (só leitura) ao Claude Desktop e ao Claude Code. |

## Segurança e privacidade

- **Nada muda sem você ver antes.** Ajustes mostram o estado atual e guardam backup; limpezas mostram a prévia e pedem confirmação com token de uso único; o disco manda para a Lixeira.
- **Sem atalhos perigosos.** Não desliga Windows Update, Defender, firewall nem UAC; não limpa Prefetch; não mexe em pastas do sistema nem de outros usuários.
- **Nativo.** Tudo por API do Windows — nenhum `cmd`, PowerShell, `reg` ou `sc` por baixo.
- **Menor privilégio.** Roda como usuário comum; pede administrador (UAC) só para o que exige.
- **Sem driver de kernel.** Leituras de hardware usam o que o Windows já oferece.
- **Sem telemetria.** Diagnósticos ficam no PC. Chaves de IA ficam no Gerenciador de Credenciais do Windows.

Detalhes e como reportar vulnerabilidades: [SECURITY.md](SECURITY.md).

## Instalar

1. Baixe o instalador `G2 Boost_<versão>_x64-setup.exe` na página de releases.
2. Execute: instala para o seu usuário, sem pedir administrador. Requer Windows 10 22H2 ou 11 (x64) e WebView2 (já vem no Windows 11; o instalador cuida no Windows 10).
3. Enquanto o certificado de assinatura constrói reputação, o SmartScreen pode mostrar "O Windows protegeu o computador" — confira o editor **Gustavo Martins Gripaldi** em "Mais informações".

Para desinstalar: Configurações › Aplicativos. Antes, desligue em Configurações do app as rotinas que quiser remover; ajustes podem ser revertidos na própria tela do PC Boost.

## Usar com o Claude (MCP)

```bash
claude mcp add g2boost -- "C:\Users\<você>\AppData\Local\G2 Boost\G2 Boost.exe" --mcp
```

O app mostra o comando com o caminho certo em **Assistente IA › Usar no Claude Desktop / Claude Code**.

## Desenvolvimento

Pré-requisitos: Rust estável (MSVC), [Bun](https://bun.sh), WebView2. Toolchain e caches podem
ficar fora do C: — ver [Docs/core/architecture.md §8](Docs/core/architecture.md).

```bash
bun install
bun run tauri:dev     # app completo com hot reload (primeira compilação do Rust leva alguns minutos)
bun run dev           # só a interface no navegador (comandos nativos retornam vazio)
bun run check         # typecheck + Biome + clippy + testes (bun test + cargo test) — verde antes de fechar fase
bun run tauri:build   # instalador NSIS em target/release/bundle/nsis
```

| Pasta | Conteúdo |
| --- | --- |
| `src/` | Interface React 19 + TypeScript + Tailwind 4 (páginas, componentes, i18n em `locales/`) |
| `src-tauri/` | Shell Tauri: comandos IPC finos, bandeja, modo `--mcp` |
| `crates/core/` | Toda a lógica e as validações (Win32, disco, limpeza, rede, ajustes, hardware, IA context) |
| `crates/mcp/` | Servidor MCP só leitura (biblioteca usada pelo app e binário de desenvolvimento) |
| `tests/` | Testes da interface (`bun test`) |
| `tools/` | Utilitários de build (stub sem Visual Studio, gerador de avisos de terceiros) |
| `Docs/` | Documentação viva — comece por [Docs/README.md](Docs/README.md) |

Regras de engenharia (segurança, testes, i18n, docs): [Docs/core/global-rules.md](Docs/core/global-rules.md).
Roadmap: [Docs/core/next-steps.md](Docs/core/next-steps.md). Release e assinatura:
[Docs/release/distribution-and-trust.md](Docs/release/distribution-and-trust.md).

## Licença

Software proprietário — © 2026 Gustavo Martins Gripaldi. Todos os direitos reservados. Ver [LICENSE](LICENSE).
Componentes de código aberto e as suas licenças: [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
