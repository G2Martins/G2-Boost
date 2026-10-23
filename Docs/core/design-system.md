# Design system — grafite quente, um accent, motion curto

**Última revisão:** 2026-10-22 (mark G²) · 2026-10-18 (fase 3A: "tirar a cara de IA") · **Fonte de tokens:** `src/index.css` (`:root` HSL + `@theme inline` do Tailwind 4) · **Referência de convenção:** `Valerium-Portal/apps/dashboard/src/styles/app.css` (route-enter, home-tile, pulse-panel) — mesmo mecanismo: HSL em CSS custom properties, consumo via classes Tailwind, zero hex em componente.

Direção: app de diagnóstico, não "gaming dashboard" genérico. Neutros quentes (grafite, texto cor de papel), **um** accent ("brasa", herdado da marca), status com parcimônia. Cards são **poços** — mais escuros que a página, com um filete de luz de 1px no topo: no hover eles clareiam (vêm à tona), nunca "flutuam". Aplicar com a skill `frontend-design`.

## 1. Paleta — tokens canônicos (HSL, sem `hsl()`)

Consumir via Tailwind (`bg-background`, `text-muted-foreground`, `border-border-strong`, `bg-primary`…) ou `hsl(var(--token) / alpha)`. **Nunca** hex literal num componente (canvas lê o token via `getComputedStyle`). Dark-only na v1.

| Token | Valor | Uso |
|---|---|---|
| `--background` | `30 6% 6.5%` | página (com `.carbon`) |
| `--surface` | `30 6% 8.5%` | sidebar / topbar / banner / modal |
| `--card` | `30 7% 4.5%` | preenchimento sólido equivalente ao poço (o `.panel` usa preto 42% translúcido) |
| `--card-hover` | `30 6% 9.5%` | poço no hover (clareia) |
| `--border` / `--border-strong` | `30 6% 15%` / `30 6% 22%` | hairline / hairline em hover e controles |
| `--foreground` | `38 22% 91%` | texto (papel) |
| `--muted-foreground` | `32 7% 58%` | rótulos, eixos |
| `--primary` | `12 86% 56%` | **o** accent: CTA principal, marcador da sidebar, aba ativa, seleção, foco, fio de marca, série principal |
| `--success` / `--warning` / `--destructive` | `148 40% 52%` / `38 82% 60%` / `352 72% 63%` | só status (chip tingido, indicador, texto de erro) |
| `--ring` | `12 86% 62%` | focus ring |

Séries: `--chart-1` = accent, `--chart-2` = neutro claro quente, `--chart-3` = aço — legíveis pela luminância, não só pelo matiz. Gauges: download = gradiente do accent, upload = neutro. Treemap: `--class-*` categóricas dessaturadas.

**Chip tingido** (`<Badge tone>`): fundo 12% + texto + anel interno 25% da mesma cor (`neutral | accent | success | warning | danger`). Nada de `className` de cor solta em Badge.

**Toggle ligado** é monocromático (trilho claro, botão escuro): com muitos toggles na tela, o accent viraria ruído.

## 2. Tipografia
**Chakra Petch** (`font-display`) só em títulos de página/card e números grandes (KPI, gauge). Rótulos e botões em **Manrope** (`font-sans`), caixa normal — nada de "11px uppercase espaçado" em todo card (era o que dava cara de template gamer). Eyebrows de seção pequenos (10px uppercase) só onde agrupam dados. **JetBrains Mono** + `.num` (`tabular-nums`) para ms, MB, IPs. Tudo via `@fontsource` (offline).

## 3. Superfície
- `.panel`: poço translúcido + `--shadow-card` (filete de luz no topo + anel de 1px).
- `.panel-hover` (cards interativos): no hover o fundo clareia, `::before` sobe um gradiente do accent de baixo e `::after` cresce um fio de 1px do centro (550 ms). Filhos ficam acima dos pseudo-elementos (`isolation`).
- `.halo`: brilho radial que segue o ponteiro (KPIs). Um único listener em `App.tsx` escreve `--mx/--my`; a intensidade é `@property --halo` (animável) num `background-image` — convive com o `.panel-hover`.
- `.rule-accent`: fio de marca (segmento accent curto em repouso, cresce no hover do card pai).
- `.grain` global no shell (feTurbulence a 2%, sem rede) e `.carbon` (grade 28 px) na página.
- `.press`: tudo clicável que não é `Button` desce 1px ao pressionar (nav, abas, segmentados).

## 4. Motion
Tokens: `--motion-fast` 200 ms · `--motion-base` 350 ms · `--motion-slow` 550 ms · `--ease-out-expo` `cubic-bezier(.16,1,.3,1)` (utilitário `ease-out-expo`).

| Peça | Como |
|---|---|
| Entrada de rota | `.route-enter` no wrapper com `key={route}` (só o conteúdo, não o shell): 200 ms, 4 px, **sem fill-mode** |
| Lista de cards | `className="animate-rise" style={rise(i)}` → `--i`; atraso `min(i,8) × 60 ms`, 550 ms; `translate` (não `transform`, não briga com hover) |
| Números | `<CountUp value format>` / `KpiCard count+format` / `Gauge format`: rAF, ease-out cúbico, 600 ms; o 1º valor conta a partir de 0, os seguintes a partir do que está na tela; `null` = skeleton (nunca um 0 provisório) |
| Gráficos | linhas com `pathLength=1` + `.draw-line` (desenha em 700 ms, +260 ms) quando há dados; área sob a 1ª série com `.fade-late` (+520 ms) |
| Loading | `<Skeleton>` (shimmer 2,5 s; cor de repouso dentro dos stops do gradiente) e `<LoadingScope loading>` (todo `MetricRow` dentro vira skeleton) — toda rota tem skeleton |
| Modal | `<Modal>` sobre `<dialog>` nativo (foco preso, Esc, página inerte): abre 300 ms, fecha 200 ms, fundo com blur 4 px |
| Monitor ao vivo | `LiveLine` com `seq`: o grupo de dados remonta um passo à direita a cada amostra e volta em 1 s linear (`.live-scroll`, recorte nas bordas), então a linha rola sem pular. `Gauge`: ponteiro por `transform: rotate` e arco por `stroke-dasharray` do arco inteiro, mesma transição (`.gauge-anim`, 700 ms); `x2/y2` e `d` não animam de forma confiável. |

**Segurança das animações:** o estado de repouso é sempre o visível — `opacity: 0` só dentro de `@keyframes`. Tudo que se move vive em `@media (prefers-reduced-motion: no-preference)`; em `reduce` nada desloca, desenha ou brilha (cor ainda muda no hover). O único `fill-mode` é o `backwards` do stagger (segura o 1º quadro só durante o atraso, ≤ 480 ms).

## 5. Estrutura — onde a UI vive
- `src/components/ui/primitives.tsx` — Button, Card/CardTitle, HelpHint, Tabs, Switch, StatusPill, Badge (tons), Skeleton, LoadingScope, MetricRow, Modal, EmptyState, `selectClass`; `count-up.tsx` — `useCountUp`/`CountUp`/`reducedMotion`; `gauge.tsx`, `live-line.tsx`, `kpi-card.tsx`. Reusar antes de criar.
- `AppShell`: sidebar de ícones (ativo = fundo neutro + marcador accent à esquerda) + topbar fina com a marca e o chip de elevação real (`AppInfo.elevated`). Sem busca/sino de enfeite.
- `PageHeader`: banner `surface` com brilho do accent a 12% + abas sublinhadas (ativa = borda accent).

### 5.1 Marca: mark "G²" (2026-10-22)
Linguagem da família Grovic/Valerium (letra geométrica grossa e arredondada, "bola" na ponta da haste como no V do Valerium, hexágono dividido na diagonal em dois tons como o da Grovic). **G** grosso com a ponta superior em bola, barra curta terminando num **núcleo hexagonal** bicolor (accent + accent escurecido) e o **2 em expoente** no accent, recortado do G por um vão (máscara, não cor de fundo, então funciona sem placa). Leitura: "G ao quadrado", potência.
- Fonte única: [src-tauri/icons/g2boost.svg](../../src-tauri/icons/g2boost.svg) (placa grafite arredondada, 1024). Ícones do app: renderizar o SVG em PNG 1024 (`node tools/render-svg.mjs src-tauri/icons/g2boost.svg <saida.png> 1024`, Edge via Playwright) e `./node_modules/.bin/tauri icon <png> -o src-tauri/icons`. `public/favicon.svg` = cópia do SVG.
- Na UI: `Logo` em [app-shell.tsx](../../src/components/shell/app-shell.tsx), mesma geometria sem placa, cores por token (`--foreground`, `--primary`, metade escura = `--primary` + `--background` a 38%), máscara com id de `useId`.
- Em 32 px o mark ainda lê (verificado no `32x32.png`); abaixo disso, só a placa com o G.

## 6. Regras
- Estado perigoso = confirmação (inline no card ou `<Modal>`); botão destrutivo só depois da prévia.
- Toda string via i18n; ícones Lucide.
- Antes de criar UI: (1) primitiva existente? (2) `Grovic-Components`? (3) inspiração `design-ref` — e re-amarrar nos tokens §1.
