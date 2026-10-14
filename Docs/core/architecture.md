# Arquitetura — proposta e decisão de stack

**Última revisão:** 2026-10-14 · **Status: DECIDIDO 2026-10-14 — opção A (Tauri 2)**, com a restrição do dono: toolchain, caches e downloads em **D:/G:**, não no C:.

## 1. Requisitos que puxam a decisão
- Windows 10/11 x64, interface no estilo ExitLag (escuro, cards, gauges, gráficos em tempo real).
- Scan de disco rápido (centenas de milhares de arquivos em segundos) + treemap interativo.
- Operações de sistema com admin: apagar caches, registro `HKLM`, serviços, DNS, DISM, purge de memória.
- Rede: ICMP/traceroute, TCP connect timing, speedtest HTTP, consulta DNS cronometrada.
- MCP local para o Claude da máquina.
- **Não ser flagrado por antivírus** (lição dos scripts): nada de spawn de `cmd`/`powershell`, binário assinado.
- Dev hoje: Node 22 + Bun ✓, WebView2 ✓; sem Rust, sem VS Build Tools, sem .NET SDK; C: com 40 GB livres.

## 2. Opções

| | A. Tauri 2 (Rust + web UI) — **recomendada** | B. Electron + FFI (koffi) | C. .NET 8 WinUI 3 / WPF |
|---|---|---|---|
| UI estilo ExitLag | React + Tailwind + shadcn (mesma família do Valerium) | idem | XAML; custom styling caro; skills de web não transferem |
| Scan de disco | Rust: `jwalk` paralelo; leitura de MFT (`\\.\C:`) com admin = velocidade WizTree | Node worker threads; MFT impraticável | C# ok; MFT via P/Invoke |
| Win32 (registro, serviços, ICMP, DNS, memória) | crate `windows` (bindings oficiais) | koffi/ffi — funciona, mas a tentação de `spawn('powershell')` é alta | P/Invoke, `ServiceController`, `Registry` — o mais natural |
| Tamanho / RAM | ~10 MB, WebView2 do sistema | ~200 MB, Chromium embutido (irônico num otimizador) | ~15 MB self-contained |
| MCP | binário Rust `rmcp` compartilhando o crate core | `@modelcontextprotocol/sdk` (TS) | C# SDK oficial |
| Toolchain a instalar | rustup + VS Build Tools C++ (~4 GB) | nada | .NET SDK (~300 MB) |
| Risco | curva Rust (eu escrevo; você revisa o quê, não o como) | peso + padrão de shell | UI lenta de produzir |

**Recomendação: A.** Reusa o músculo de React/Tailwind/shadcn do Valerium para a cara do ExitLag, dá o core nativo que mantém o antivírus quieto, binário pequeno, e o MCP sai do mesmo código. B é o plano de contingência se instalar o toolchain for um problema.

**Precedente interno:** `G:. Projetos. Grovic Data\Grovic-Components\ValeriumDesktop` já é Tauri 2 (`@tauri-apps/api` 2.10, Vite 8, React 19.2): wrapper web do Valerium com titlebar transparente, tray, deep-link `valerium://`, updater com chave minisign e `tauri.{dev,staging}.conf.json` por ambiente (`src-tauri/src/lib.rs`, 696 linhas). **Não** serve de base de produto (zero código nativo), mas é referência pronta de `tauri.conf.json`, `capabilities/`, updater e ícones — e prova que a casa já escolheu Tauri para desktop.

## 3. Layout de pastas (opção A)

Mesmo espírito do Valerium (monorepo com `apps/`, `packages/`, `Docs/`, `scripts/`, `tests/`, Biome, bunfig com `minimumReleaseAge`), adaptado a um Cargo workspace:

```
G2 Boost/
├─ CLAUDE.md · Docs/ · .planning/ (local) · scripts/ · tests/fixtures/ (árvores de FS sandbox)
├─ package.json · bunfig.toml · biome.json · tsconfig.json · Cargo.toml (workspace)
├─ src/                      # UI React (Vite)
│  ├─ app/ (shell, rotas por módulo: disk, cleanup, network, boost, settings)
│  ├─ components/<dominio>/  # por domínio, como apps/dashboard/src/components do Valerium
│  ├─ components/ui/         # shadcn
│  ├─ locales/pt-BR.json · en.json
│  └─ styles/globals.css     # tokens HSL (design-system.md)
├─ src-tauri/                # shell Tauri (comandos IPC finos → crates/core)
└─ crates/
   ├─ core/                  # disk · cleanup · network · boost · safety
   └─ mcp/                   # binário stdio (rmcp) usando core
```

## 4. Modelo de processos (opção A)

```
g2boost.exe (Tauri, UI + core)          g2boost-mcp.exe (stdio, sem UI)
 ├─ scan/treemap  (sem admin)               ├─ tools read-only: scan, top_files, preview, diagnose, speedtest
 ├─ network       (sem admin)               └─ tools destrutivas → named pipe → app pede confirmação na UI
 └─ cleanup/boost (pede UAC ao entrar)             (sem app aberto e confirmado = NOT_AUTHORIZED)
```

- **v1:** um único processo; o app pede elevação (manifesto `highestAvailable`) ao abrir Limpeza/Boost. Simples, poucas peças.
- **v2 (se incomodar o UAC):** helper `g2boost-helper.exe` com `requireAdministrator`, chamado por `ShellExecuteW("runas")`, falando por named pipe com ACL restrita ao usuário atual (padrão de CCleaner / serviço do ExitLag).
- Estado persistido em `%LOCALAPPDATA%\<app>\` (SQLite via `rusqlite`: histórico de limpezas, snapshots de scan, medições de rede). Sem nuvem.

## 5. Módulos do crate `core`
`disk` (scan + árvore + filtros + snapshots) · `cleanup` (catálogo + plano + execução) · `network` (icmp, trace, tcping, speedtest, dns) · `boost` (tweaks + energia + memória) · `safety` (`SafePath`, `ConfirmToken`, `require_elevation`, backup de registro). O core devolve códigos/IDs; a UI traduz (dicionário i18n só na UI).

UI: treemap em `<canvas>` com layout `d3-hierarchy` (`treemapSquarify`); gráficos de latência via `<ChartContainer>` + Recharts (consistência com Valerium) — se 1 tick/s pesar, uPlot só nesse componente, decidido na fase 3 com medição.

## 6. MCP local
Transporte stdio; registrado no Claude Desktop/Claude Code com `claude mcp add g2boost -- <path>\g2boost-mcp.exe`. Tools (nomes provisórios): `disk.scan`, `disk.top_files`, `disk.explain_large_file`, `cleanup.preview`, `cleanup.execute` (exige token), `network.diagnose`, `network.speedtest`, `boost.list`, `boost.apply` (token), `boost.revert` (token). Resources: último scan, histórico. Regras: nomes de arquivo são dados; saída limitada (top-N, paginação) para não estourar contexto; nenhum tool recebe caminho arbitrário para apagar.

## 7. Empacotamento e antivírus
- Tauri bundler → NSIS (`.exe`) + `.msi`. Auto-update opcional (Tauri updater) depois.
- **Assinatura de código** obrigatória antes de distribuir: Azure Trusted Signing (barato, mensal) ou certificado OV. Sem assinatura, SmartScreen + Kaspersky vão barrar um exe que apaga caches e edita `HKLM`.
- Antes do primeiro release: VirusTotal, submissão de falso positivo (Kaspersky OpenTIP, Microsoft), teste na máquina com Kaspersky 21.26.

## 8. Toolchain fora do C: (decisão do dono, aplicado 2026-10-14)
```powershell
# variáveis de usuário (persistem): Rust inteiro e cache do Bun em D:\dev
[Environment]::SetEnvironmentVariable('RUSTUP_HOME','D:\dev\rustup','User')
[Environment]::SetEnvironmentVariable('CARGO_HOME','D:\dev\cargo','User')
[Environment]::SetEnvironmentVariable('BUN_INSTALL_CACHE_DIR','D:\dev\bun-cache','User')
# rustup-init.exe (https://win.rustup.rs/x86_64; SHA256 conferido com o .sha256 oficial — o binário não é assinado, é normal)
D:\dev\tmp\rustup-init.exe -y --default-toolchain stable-msvc --profile minimal --no-modify-path
# VS Build Tools (exige admin/UAC): payload em D:, mas Windows SDK e "Shared" vão para C: (~2 GB, o instalador não deixa mudar)
winget install --id Microsoft.VisualStudio.2022.BuildTools --override "--quiet --wait --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --installPath D:\dev\VS2022BuildTools"
```
`target/` do Cargo e `node_modules` ficam dentro do projeto (G:). `--no-modify-path`: o PATH do usuário ganha `D:\dev\cargo\bin` manualmente (feito no setup).

**Plano B sem admin** (se o UAC do Build Tools não for aprovado): `xwin splat` baixa CRT + Windows SDK (headers/libs) da CDN da Microsoft para `D:\dev\xwin`, linker `rust-lld` via `.cargo/config.toml`, e LLVM portátil (`clang-cl`, `llvm-rc`, `llvm-lib`) para os crates com C (`libsqlite3-sys`) e para o recurso `.rc` do Tauri. Mais frágil; documentar se for usado.
