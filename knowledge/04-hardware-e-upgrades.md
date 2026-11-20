# Hardware e upgrades

**Última revisão:** 2026-11-20

Ordem de prioridade quando o usuário pergunta o que comprar (sempre com classe de preço, sem link de loja):

1. **Sistema num HD mecânico:** SSD primeiro. É o maior salto de fluidez do Windows e dos carregamentos. NVMe se a placa tem slot M.2; SATA se não tem.
2. **RAM em canal único:** um segundo pente igual dobra a banda de memória. Ganho grande em 1% lows e enorme em vídeo integrado. Barato.
3. **Pouca RAM:** 16 GB é o mínimo confortável para jogos atuais; 32 GB para quem joga com navegador, Discord e stream abertos.
4. **XMP/EXPO desligado:** grátis, na BIOS. Antes de comprar RAM nova.
5. **GPU ou CPU:** depende do gargalo. GPU em ~95% nos jogos = GPU. GPU sobrando com FPS baixo = CPU (poucos núcleos ou geração antiga). Use `advisor.gaming` (tier, resolução alvo e gargalo provável) como ponto de partida.
6. **VRAM:** 8 GB já limita texturas em jogos recentes a 1440p e acima. Sinal: travadas ao virar a câmera e uso de VRAM no limite.
7. **Monitor:** de 60 para 144 Hz muda a sensação mais que muitos upgrades, se a máquina entrega o FPS.

Cuidados:

- Trocar GPU por uma mais forte pode exigir fonte maior; confira a recomendação do fabricante da placa.
- Placa-mãe antiga pode não ter Resizable BAR ou suportar só PCIe 3.0: diga quando isso limita a GPU nova.
- Pentes de RAM diferentes misturados costumam rodar na velocidade do mais lento e às vezes sem XMP estável.
- Nunca recomende peça por marca patrocinada; compare por desempenho medido em fontes confiáveis.

## Anti-cheat e BIOS (2026-11)

- Riot Vanguard (Valorant, League of Legends) no Windows 11 exige TPM 2.0 e Secure Boot (erros VAN9001 e VAN9003). O modo sob demanda (driver só com o jogo aberto) pede também Windows 11 25H2, VBS, integridade de memória e IOMMU.
- FACEIT (CS2) exige TPM 2.0, Secure Boot e Windows 11; usa VBS e IOMMU como proteção. Battlefield 6 e Call of Duty Black Ops 7/Warzone exigem Secure Boot e TPM 2.0.
- Secure Boot depende de UEFI sem CSM, e o UEFI só inicia o Windows de disco GPT: converter MBR para GPT (ferramenta MBR2GPT do Windows) vem antes de desligar o CSM.
- O Windows não liga Secure Boot, TPM nem IOMMU sozinho: é na BIOS. Mande para Jogos › Pronto para jogar com anti-cheat (o que falta, caminho no menu da placa, Reiniciar na BIOS). O app confere de novo depois do boot e avisa. Atualizar ou resetar a BIOS costuma desligar o fTPM/PTT.
- Integridade de memória custa cerca de 5 % de FPS em alguns jogos, mas com esses jogos instalados não sugira desligar.
