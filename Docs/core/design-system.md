# Design system — "cara de ExitLag", convenções do Valerium

**Última revisão:** 2026-10-14 (noite) · **Fonte de tokens:** `src/index.css` (`:root` HSL + `@theme inline` do Tailwind 4) · **Referência de convenção:** `Valerium-Portal/Docs/core/design-system.md` e `packages/ui/src/globals.css` (mesmo mecanismo: HSL em CSS custom properties, consumo via classes Tailwind, zero hex em componente).

Referência visual: prints do ExitLag 5.23 em 2026-10-14 — escuro, cards de cantos suaves, abas sublinhadas, gauges em arco, toggles verdes, botão primário vermelho, `?` de ajuda ao lado de cada título. Aplicar com a skill `frontend-design`; evitar cara genérica de IA.

## 1. Paleta — tokens canônicos (HSL, sem `hsl()`, como no Valerium)

Consumir via Tailwind (`bg-background`, `bg-card`, `text-muted-foreground`, `border-border`, `bg-primary`…) ou `hsl(var(--token) / alpha)`. **Nunca** hex literal num componente. Dark-only na v1 (como o ExitLag); se vier tema claro, só muda o bloco `:root` — por isso a disciplina de token já.

| Token | Valor | Uso |
|---|---|---|
| `--background` | `228 14% 7%` | fundo da janela |
| `--surface` | `223 14% 10%` | sidebar / topbar / banner de seção |
| `--card` | `225 13% 12%` | cards |
| `--card-hover` | `222 14% 15%` | hover de card/linha |
| `--border` | `225 13% 18%` | bordas, divisores, trilho de aba |
| `--foreground` | `220 15% 92%` | texto |
| `--muted-foreground` | `222 8% 58%` | rótulos secundários, eixos |
| `--primary` | `356 78% 55%` | ação principal (Iniciar, Limpar, Aplicar), item ativo da sidebar |
| `--success` | `165 73% 46%` | "Estável", toggle ligado, check |
| `--warning` | `39 87% 62%` | atenção |
| `--destructive` | `358 100% 68%` | perda de pacote, erro, delete permanente |
| `--ring` | `165 73% 46%` | focus ring (borda verde do item selecionado, como no ExitLag) |
| `--radius` | `0.5rem` | raio padrão (cards 8px; botões 6px via `rounded-md`) |

Gráficos (grayscale no Valerium; aqui com 2 acentos porque as séries têm identidade — Google/Cloudflare):

| Token | Valor | Uso |
|---|---|---|
| `--chart-1` | `258 90% 66%` | série 1 (roxo) |
| `--chart-2` | `37 92% 55%` | série 2 (laranja) |
| `--chart-3` | `142 71% 45%` | disponibilidade |
| `--chart-grid-stroke` / `--chart-axis-text` | `225 13% 18%` / `222 8% 58%` | grid + eixos |
| `--gauge-download-start/end` | `262 83% 58%` → `280 70% 45%` | gauge de download |
| `--gauge-upload-start/end` | `330 75% 55%` → `350 70% 45%` | gauge de upload |

## 2. Tipografia
ExitLag usa Ubuntu. Aqui (skill `frontend-design`: nada de Inter/Roboto): **Chakra Petch** (`font-display` — títulos, abas, botões, valores grandes; quadrada, cara de console), **Manrope** variável (`font-sans` — corpo), **JetBrains Mono** (`font-mono` + classe `.num` com `tabular-nums` — ms, MB, IPs). Tudo empacotado via `@fontsource` (offline; CSP não libera Google Fonts). Escala: título de página 22px/600 · título de card 15px/600 · corpo 13px · rótulo 12px · valor de métrica 13px/600.

## 3. Gráficos — convenção obrigatória
Recharts envolto em `<ChartContainer>` + `ChartConfig` (portar `chart.tsx` do `packages/ui` do Valerium — Recharts v3, mesmos hurdles de tipos). Cores só por `--chart-*`. Gauge e treemap são canvas/SVG próprios (não existem no Recharts), mas também lêem tokens via `getComputedStyle`.

## 4. Estrutura — onde a UI vive
- `src/components/ui/primitives.tsx` — Button, Card/CardTitle, HelpHint (Radix Tooltip), Tabs (Radix, sublinhadas), Switch (Radix), StatusPill, Badge, MetricRow, EmptyState; `gauge.tsx` — arco 270° SVG com gradiente por token. Reusar antes de criar.
- `src/components/<dominio>/` — `disk/`, `cleanup/`, `network/`, `boost/`, `shell/` (como `apps/dashboard/src/components/<dominio>` no Valerium).
- Toda rota com **skeleton de loading** (padrão Valerium).
- Biblioteca de apoio: `G:. Projetos. Grovic Data\Grovic-Components` — `directory.md` (blueprint), `design-ref/` (40+ marcas: linear, raycast, nvidia, cursor, posthog… inspiração de layout/microinteração, não copiar pixel), `ValeriumDesktop/` (Tauri 2 de referência), `ValeriumPro/`. Consultar antes de inventar componente.

## 5. Componentes base (shadcn + custom)
- `AppShell`: sidebar fina de ícones (56px, item ativo com fundo `primary/15` e ícone `primary`) + topbar (toggle "ON", sino, busca) + versão no rodapé.
- `PageHeader` com banner `surface` sutil + `Tabs` sublinhadas (ativo = `foreground`, trilho `border`).
- `Card` com `HelpHint` (`?` + tooltip i18n).
- `StatusPill` (`Estável` em `success/15`) e `MetricRow` (rótulo `muted-foreground` · valor `foreground` · check `success`).
- `Gauge` (arco 270°, gradiente por token, valor central + unidade).
- `ToggleCard` (ícone, título, badge Beta, descrição, link "Não recomendado se…", switch `success`).
- `TreemapCanvas` (render próprio; cores por classe de arquivo via tokens `--class-*`; tooltip com caminho/tamanho/datas).
- `LatencyChart` (linha + marcadores de disponibilidade/perda na base) · `RamBar` (usado/livre) · `DnsRankRow` (posição, provedor, IPs, badge Atual/Mais rápido, resposta média, botão).

## 6. Regras
- Texturas: `.carbon` (grade 28 px a 2,5 %) no fundo, `.grain` (SVG feTurbulence a 3,5 %) no banner, `.banner` (radial vermelho a 18 % no canto). Animações: `animate-rise` + `rise(i)` para revelação escalonada; `animate-pulse-dot` no status ON.
- Estado perigoso = confirmação inline no card (sem `alert`); botão destrutivo `destructive` só depois da prévia.
- Toda string via i18n; ícones Lucide; `prefers-reduced-motion` respeitado nas animações de gauge/treemap.
- Antes de criar UI: (1) shadcn? (2) `Grovic-Components`? (3) inspiração `design-ref`/web — e re-amarrar nos tokens §1.
