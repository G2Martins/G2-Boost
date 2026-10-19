# Temperatura e energia

**Última revisão:** 2026-10-18

- Use sempre os limites de `thermal_limits_c` do contexto (aviso e crítico). Não use outros números.
- Temperatura da CPU só existe quando HWiNFO (grátis, recomendado: modo sensores com Memória compartilhada ligada; na versão grátis a memória compartilhada desliga a cada 12 h e é só religar) ou LibreHardwareMonitor está rodando. O G2 Boost lê deles e não instala driver nenhum, por decisão do projeto. Não recomende Core Temp nem MSI Afterburner para isso: os drivers deles são marcados como vulneráveis por antivírus. Sem isso, diga que é desconhecida e sugira abrir um deles. Temperatura da GPU vem do driver (NVIDIA) e aparece no monitor do Painel.
- Acima do aviso sob carga: o processador ou a GPU reduz o clock (throttling) e o FPS cai. Ordem de correção: limpar poeira, conferir fluxo de ar e curva das ventoinhas, trocar pasta térmica em máquina com anos de uso, limitar FPS. Nada que aumente carga antes disso.
- Notebook: base elevada ajuda; na tomada com plano Equilibrado e limite de FPS costuma render mais estável que Desempenho máximo, que esquenta e entra em throttling.
- Plano Desempenho máximo mantém a CPU em clock alto mesmo parada: mais consumo e calor no dia a dia. Sugira só para quem joga e aceita isso, e diga o custo.
- Overclock, undervolt e mudança de limite de potência na BIOS nunca são a primeira sugestão. Se o usuário pedir: um ajuste por vez, testar estabilidade, ter caminho de volta (reset da BIOS), e avisar de garantia.
- Fonte (PSU) fraca ou velha causa desligamentos sob carga (evento Kernel-Power 41 sem tela azul). Vale citar quando há desligamentos em jogo.
