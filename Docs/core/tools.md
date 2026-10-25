# Ferramentas (teste do mouse, teste de monitor, perda de pacotes)

**Última revisão:** 2026-10-25 · Código: UI `src/pages/mousetest.tsx`, `src/pages/monitortest.tsx`, `src/pages/packetloss.tsx`, `src/components/tools/` (`stat`, `mouse-figure`, `monitor-patterns`, `packet-chart`), contas puras `src/lib/mouse.ts`, `src/lib/refresh.ts`, `src/lib/pktloss.ts`; core `crates/core/src/pktloss.rs`; shell `src-tauri/src/tools_cmds.rs`. Testes: `tests/mouse.test.ts`, `tests/refresh.test.ts`, `tests/pktloss.test.ts`, `e2e/mousetest.spec.ts`, `e2e/monitortest.spec.ts`, `e2e/packetloss.spec.ts`.

Três páginas do grupo **Ferramentas** da barra lateral. Só a de perda de pacotes tem comando no core (ICMP e arquivo); o mouse e o monitor são só interface (a webview já entrega ponteiro, teclado, tela cheia e quadros).

## Teste do mouse (`mousetest`)

Duas abas: **Taxa de polling** e **Botões e roda**.

### Taxa de polling

- **Fonte dos relatórios:** `pointerrawupdate` quando existe (um evento por relatório do mouse, só em contexto seguro: `localhost` e `*.localhost` valem, então o app e o dev server servem), senão `pointermove`. Os dois passam por `getCoalescedEvents()`: cada evento agrupado traz o próprio `timeStamp`. Só `pointerType === "mouse"` conta. A página mostra qual fonte está em uso (selo "Eventos brutos" ou "Eventos agrupados", este com aviso de que a leitura pode ficar abaixo do real).
- **Sem renderizar a 1000 Hz:** os eventos só alimentam um `RateMeter` (ref); a tela segue a cada 100 ms.
- **`RateMeter` (`src/lib/mouse.ts`, puro):**
  - timestamp que não se ordena (NaN, voltou no tempo) é ignorado;
  - intervalo >= **50 ms** é pausa da mão (`gaps`), não é intervalo de relatório;
  - intervalo maior que **3x a mediana** dos últimos 64 (com pelo menos 8) é travada da página (`stalls`): contada, fora da média; **5 travadas seguidas** significam que a taxa mudou de verdade (botão de taxa do mouse, movimento lento): a janela recomeça dali;
  - intervalo **0** é real (dois relatórios com o mesmo carimbo), então um mouse de 4000 Hz num relógio grosseiro ainda tem média certa;
  - **Agora** = 1000 / média da janela (64); **Média** = 1000 / média de todos os intervalos aceitos; **Máximo** = maior "Agora" visto com a janela pelo menos pela metade (um intervalo minúsculo sozinho não prova nada); **Resposta** = intervalo médio (ms); **Jitter** = média da diferença entre intervalos vizinhos (a mesma definição do monitor de rede); **Amostras** = relatórios ordenáveis.
- **Gráfico:** os últimos 200 intervalos (`LiveLine`, sem `seq`: a linha só redesenha). **Régua** logarítmica 125 a 8000 Hz com marca da média e do máximo.
- **Limite honesto (dito na ajuda):** o número é o que chegou à janela, não passa do que o Windows e o navegador entregam. Movimento lento gera menos relatórios (o ponteiro só mexe quando muda um pixel), por isso a instrução "círculos rápidos". Medido nesta máquina pelo Playwright (CDP, não é um mouse): ~115 Hz.

### Botões e roda

- Botões 0 a 4 (`MouseEvent.button`: esquerdo, meio, direito, voltar, avançar) acendem na silhueta enquanto apertados; o `mouseup` é ouvido na janela inteira (soltar fora da área não deixa o botão aceso). `mousedown` com `preventDefault` (sem seleção nem rolagem automática do botão do meio), menu do botão direito e `auxclick` desligados na área, `mouseup` com `preventDefault` para voltar/avançar não navegarem (o app não empilha histórico, então no Tauri não haveria para onde voltar de qualquer jeito).
- **Clique duplo falso:** dois `mousedown` do mesmo botão a menos de **80 ms** (`BOUNCE_MS`) um do outro. Uma pessoa não passa de ~100 ms; abaixo de 80 ms costuma ser o micro-switch gasto. Cada botão tem o próprio relógio; o aviso mostra o botão, quantas vezes e o último intervalo. Com 6 cliques ou mais e nenhum falso, a tela diz que não achou nenhum.
- **Roda:** cada evento de rolagem é um passo (para cima: `deltaY < 0`); o listener não é passivo para a página não rolar embaixo do teste. Rodas de alta resolução mandam vários eventos por clique (dito na ajuda).

## Teste de monitor (`monitortest`)

A página é um lançador (7 testes, cada um com uma amostra das cores e a explicação do que procurar, mais o cartão "Esta tela": resolução física = `screen` x `devicePixelRatio`, escala, bits de cor, e a última taxa medida). Cada teste abre numa **superfície** (`role="application"`, portal em `document.body`, `fixed inset-0`).

- **Tela cheia:** `requestFullscreen()` na própria superfície, dentro do clique (a superfície é criada com `flushSync` antes). No Tauri 2 o `tauri-runtime-wry` já trata o `ContainsFullScreenElementChanged` do WebView2 e põe a **janela** em tela cheia, então ocupa o monitor inteiro. Se o navegador recusar, a superfície continua cobrindo a janela (nenhum erro: o usuário só não ganha a tela cheia).
- **Teclas:** `←` `→` ou `Espaço` trocam de padrão dentro do teste (dá a volta); `↑` `↓` trocam de teste (começa no primeiro padrão); `Esc` sai (em tela cheia real o navegador consome o Esc e avisa com `fullscreenchange`; na superfície sobreposta o próprio listener sai); um clique também avança. A **dica** (nome do teste, padrão `n de total`, teclas) aparece ao entrar e a cada tecla ou movimento do mouse, some em 3,5 s (fade de 700 ms só em `motion-safe`; com "reduzir movimento" some de uma vez) e o cursor some junto. Um `aria-live` anuncia o padrão.
- **Padrões** (as cores são **sinais de teste**, geradas de números 0 a 255 em código com um comentário dizendo por quê; o resto da página usa só tokens):
  1. **Pixels mortos e travados:** preto, branco, vermelho, verde, azul, tela cheia.
  2. **Pretos:** quadrados de 0 a 10 sobre o preto 0, cada um com o número. O 0 some; se o 1, 2 e 3 também somem, os pretos estão esmagados.
  3. **Brancos:** quadrados de 245 a 255 sobre o branco 255; o 255 some; se vários da direita somem, estão estourados.
  4. **Degradê:** quatro faixas (cinza, vermelho, verde, azul) de **256 degraus discretos** desenhados num canvas (CSS poderia ter dithering do navegador, o canvas mostra o degrau do monitor).
  5. **Uniformidade e vazamento de luz:** preto 0, cinza escuro 24, cinza médio 128 (três passos).
  6. **Nitidez e geometria:** canvas em **pixel físico** (`largura x devicePixelRatio`): xadrez de 1 px, linhas verticais e horizontais de 1 px, textos de 9 a 28 px (branco no preto e preto no branco), círculo (precisa ser redondo) e moldura de 1 px na última linha de pixels (as quatro bordas precisam aparecer: senão há overscan).
  7. **Movimento e taxa de atualização:** quatro quadrados a 240, 480, 960 e 1920 px/s (pixels **físicos**), a posição vem do relógio do `requestAnimationFrame` (não de contar quadros: um quadro perdido vira salto visível, não um quadrado mais lento). Mostra a taxa medida, "perto de N Hz" quando bate com uma taxa padrão, tempo do quadro (mediana), quadros perdidos e pior quadro, e por linha quantos **px por quadro** (`velocidade / taxa`).
- **Contas (`src/lib/refresh.ts`, puras):** o quadro usual é a mediana; a taxa é a média dos quadros no prazo (<= 1,5x o usual), então poucas perdas não a derrubam; quadro > 1,5x perdeu `round(delta/usual) - 1` quadros (mínimo 1); quadro > **10x** o usual é pausa (janela escondida, movida) e não conta como perda; lixo (NaN, 0, negativo) é ignorado. `nearestStandardHz` aceita até 1,5% de diferença (59,94 lê 60) senão devolve nada. A contagem de perdas acumula a cada quadro depois dos 30 primeiros; ao sair, o resultado (com pelo menos 30 quadros) fica no cartão "Esta tela".
- **A página diz, e a regra vale:** o teste de movimento **anima de propósito, mesmo com "reduzir movimento" ligado**, e só roda quando o usuário o inicia (não há `requestAnimationFrame` na página antes disso: o e2e confere que o leitor de taxa não existe até o clique). Tudo o mais respeita `prefers-reduced-motion` (entradas, fade da dica, anéis do mouse).

## Perda de pacotes (`packetloss`)

### Perfis e alvo

| Perfil | Pacotes/s | Duração | Pacotes |
|---|---|---|---|
| `quick` (Rápido) | 10 | 10 s | 100 |
| `standard` (Padrão) | 15 | 20 s | 300 |
| `long` (Longo) | 10 | 60 s | 600 |

O perfil é uma **palavra** (enum serde): o chamador nunca escolhe taxa nem duração. O alvo (roteador pelo gateway do adaptador primário, 1.1.1.1, 8.8.8.8 ou texto digitado) passa por `pktloss::parse_target`:

- só ASCII `[A-Za-z0-9.-]`, até 253 caracteres, sem espaço, controle, porta, URL, IPv6 nem letras parecidas (Unicode);
- IP literal só no formato estrito `a.b.c.d` (sem zero à esquerda); nome com rótulos de 1 a 63, sem hífen nas pontas, **último rótulo com ao menos uma letra** (RFC 1123: `127.1` e `2130706433` são grafias antigas de IP, não nomes);
- depois de resolver (IPv4), o endereço também é conferido: `0.0.0.0/8`, multicast `224/4` e o bloco reservado com o broadcast `240/4` são recusados (um eco ali seria respondido por vários hosts). Loopback, privado, link local e CGNAT valem (o roteador é privado).
- Códigos de erro (texto do `Display`, vão como estão para a UI): `empty`, `too_long`, `invalid`, `not_allowed`, `resolve`, `busy`.

### Agenda e classificação (`crates/core/src/pktloss.rs`)

- **Uma thread agenda** os envios em prazos absolutos (`t0 + n x intervalo`), então a taxa média se mantém mesmo se um `sleep` atrasar; **cada sonda roda na própria thread** (`network::ping`, `IcmpSendEcho2`, sem `ping.exe`), então um pacote lento ou perdido nunca atrasa o próximo envio. Cada sonda carimba o instante do envio e o resultado volta por canal; quem coleta emite um evento por sonda. Tempo limite da sonda: **1000 ms** (sem resposta = perdido). Uma execução por vez (`busy`); cancelar vale em até 50 ms e **não conta** as sondas em voo.
- **Atrasado:** resposta acima de **150 ms**, ou acima de **2x a mediana e pelo menos 10 ms acima dela** (a mesma regra de picos do monitor de rede). *Desvio do pedido original ("> 2x a mediana ou > 150 ms"):* sem o piso de 10 ms, um roteador de 1 ms chamaria 3 ms de atrasado e a LAN saudável apareceria cheia de atrasos. A regra relativa só vale com **5 respostas ou mais**.
- **Fora de ordem:** a resposta de um pacote **enviado depois** chegou claramente antes dela (RFC 4737). O instante de chegada é **envio + tempo de resposta da API** (não a ordem em que as threads acordaram, que num PC cheio de jogo varia), e respostas com menos de **2 ms** de diferença contam como o mesmo instante (a API arredonda para o milissegundo e uma fila segurada no caminho sai toda de uma vez), então só reordenação de verdade aparece. Pacote perdido nunca é "fora de ordem".
- **Resumo (`Summary`):** enviados (com desfecho), recebidos, perdidos, perda %, atrasados, fora de ordem, média/mín/máx/mediana, **p95 por posto mais próximo** (`ceil(0,95 x n)`-ésimo), jitter (média da diferença entre respostas vizinhas **na ordem de envio**, igual ao do monitor), e a **maior sequência perdida** (números de sequência seguidos). Sem respostas, médias e jitter são `null` (nunca NaN no JSON).
- **Eventos:** `packetloss:probe` por sonda (`seq`, `rtt_ms` ou `null`, `late`, `out_of_order`, e o `summary` até ali). Ao fim, `packetloss_run` devolve o `Report` (amostras por número de sequência já com a classificação final, resumo, `cancelled`, início em hora local, duração).

### Comandos (`src-tauri/src/tools_cmds.rs`)

| Comando | O que faz |
|---|---|
| `packetloss_run(host, profile)` | valida, roda a rajada (até 60 s), emite os eventos e devolve o `Report` |
| `packetloss_cancel()` | para a rodada em andamento (sem efeito se não há) |
| `packetloss_export(locale)` | grava o CSV do **último relatório do core** em Documentos e abre o Explorer com o arquivo selecionado; devolve só o nome do arquivo |

### CSV

- O conteúdo é o que o core mediu: o comando **não recebe dados** (só o idioma, validado: `pt-BR` ou qualquer outro), então a UI não escolhe o que vai para o arquivo nem para onde.
- **pt-BR:** separador `;` e vírgula decimal (o Excel brasileiro abre direto); **en:** `,` e ponto. Fim de linha `\r\n`.
- Uma linha por pacote (`seq,result,rtt_ms,out_of_order,arrival`, com `result` = `ok`, `late` ou `lost`), uma linha em branco e um bloco `metric,value` (alvo, endereço, perfil, início, duração, cancelado, enviados, recebidos, perdidos, perda %, atrasados, fora de ordem, média, mínima, p95, jitter, maior sequência perdida).
- Células de texto: caracteres de controle somem, prefixo `=` `+` `-` `@` ganha `'` (fórmula de planilha), aspas e separador viram campo entre aspas.
- Nome `G2Boost-perda-de-pacotes-<alvo>-<data>_<hora>.csv` (alvo com no máximo 40 caracteres, qualquer coisa fora de `[A-Za-z0-9.-]` vira `_`), pasta Documentos pelo `SHGetKnownFolderPath` (pode estar no OneDrive), `create_new` (nunca sobrescreve: `-1`, `-2`, até `-9`). A UI mostra só o nome no aviso ("Salvo em Documentos: ...").

### Veredito, em palavras simples (`src/lib/pktloss.ts`)

O veredito só sai **quando o teste termina** (durante a rodada: "Medindo", com os números ao vivo; parou com menos de 20 pacotes: "Poucos pacotes"). Depois: sem nenhuma resposta; perda **>= 3%** ou **5 pacotes seguidos perdidos** = alta; **> 1%** = perceptível; **> 0** = mínima; sem perda mas **> 5% atrasados**, jitter **> 30 ms** ou fora de ordem em **3 ou mais pacotes e mais de 1%** (um pacote reordenado solto é normal) = instável; senão, limpa. O conselho depende do alvo: perda no roteador aponta a rede local (cabo, Wi-Fi, reiniciar); perda na internet manda comparar com o roteador e usar **Analisar rota**.

### Analisar rota (MTR existente + a regra honesta de `route.ts`)

Roda `trace_host` (30 saltos, 3 sondas, 1200 ms) no **IP que o core já validou**, passa por `analyzeRoute` e `routeVerdict`:

- **Perda real:** a perda persiste até o destino (`lossFrom`): aponta o primeiro salto (TTL, endereço, zona sua rede / provedor / internet).
- **A rota não mostrou onde:** a rajada perdeu pacotes mas o trace (poucas sondas por salto) não flagrou: diz isso em vez de chutar.
- **Sem perda real:** saltos do meio sem resposta, destino respondendo: são roteadores que só ignoram ping.
- **Rota limpa:** nenhum salto perdeu.

## Segurança (cliente hostil)

Alvo validado no core (sintaxe e endereço resolvido); perfil fechado; taxa fixa (15 pacotes/s no máximo, no máximo ~16 threads vivas por causa do tempo limite de 1 s); uma rodada por vez; nenhum `cmd`, `powershell`, `ping` nem `tracert`; CSV sem conteúdo nem caminho vindos do chamador, nome sem separador, sem sobrescrever; erros são códigos curtos, a UI traduz.

## Testes

- **Rust (`cargo test -p g2boost-core -- pktloss`):** perfis (números e `serde` recusando "turbo" e números), sintaxe do alvo com 31 textos hostis (injeção, porta, URL, IPv6, NUL, RTL, Unicode, hífen, rótulo longo, grafias antigas de IP, zero à esquerda), IPs recusados e aceitos, regra de atrasado (LAN, piso de 10 ms, 150 ms, com e sem mediana), fora de ordem com sequências feitas à mão (reversa, intercalada, com perdas) e com instantes de chegada (a hora decide, não a ordem do relato; fila liberada de uma vez não é reordenação; 2,5 ms já é), p95 por posto mais próximo, maior sequência perdida (inclusive por número de sequência, não por chegada), jitter na ordem de envio, vazio e perda total sem NaN no JSON, CSV (linhas, bloco de resumo, `;` e vírgula, células hostis, nome do arquivo, nunca sobrescrever, pasta inexistente), agenda com sonda falsa (todas reportadas uma vez, resposta lenta (600 ms) chegando depois das novas vira fora de ordem e atrasada, cancelar descarta o que está em voo, probe que devolve valor absurdo), uma execução por vez, alvo ruim não envia nada. **Ao vivo (`--ignored pktloss::tests::live`):** loopback 100/100 sem perda; cancelar devolve relatório parcial; **1.1.1.1 nesta máquina (2026-10-25): 100/100, média 2,17 ms, p95 3 ms, jitter 0,28 ms, 0 atrasados.**
- **TS (`bun test tests`):** `mouse.test.ts` (1000/125/outras taxas, pausa, travada, mudança de taxa, relatórios com o mesmo carimbo, lixo, jitter, máximo, régua, detector de clique duplo e contagem por botão, imutável), `refresh.test.ts` (144/60/240 Hz, ruído, quadro perdido, pausa, lixo, padrão mais próximo, px por quadro), `pktloss.test.ts` (perfis iguais ao core, células, todos os degraus do veredito, conselho por alvo, `routeVerdict` com `analyzeRoute`).
- **e2e:** as três specs rodam sem erro de console: lista e ajuda, círculos do mouse até o "Agora" sair do traço, contagem de botões e do clique duplo falso (dois `mousedown` no mesmo instante), roda, superfície com setas/Esc/clique, canvas dos padrões (rampa começa em preto e termina em branco, xadrez alterna, moldura nas bordas), medição de taxa (com o leitor inexistente antes do clique) e o cartão "Esta tela" guardando a última medição, controles de perda de pacotes desabilitados fora do app, reduced motion nas três.

## Pendente (passos que precisam do app e do hardware do dono)

- Abrir o app (`bun run tauri:dev`) e conferir a **tela cheia de verdade** pelo WebView2 (janela inteira no monitor, Esc, volta ao estado anterior) e o teste de movimento num monitor de taxa alta.
- Mouse real de 1000 Hz (e de 4000/8000 Hz, se houver): comparar com a taxa da fabricante; o Playwright só simula (~115 Hz).
- Rodar a perda de pacotes no app contra o roteador, 1.1.1.1 e um host que ignora ping; **Gravar em CSV** com a pasta Documentos do dono (testado em pasta temporária; o caminho do `SHGetKnownFolderPath` foi só conferido como existente) e abrir o arquivo no Excel.
- `Docs/README.md` (índice) e `CLAUDE.md` (módulos e comandos) ainda não apontam para este documento.
