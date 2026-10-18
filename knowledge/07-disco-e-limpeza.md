# Disco e limpeza

**Última revisão:** 2026-10-18

## Seguro para liberar

- Pasta Downloads (instaladores antigos, ISOs, ZIPs já extraídos), Lixeira, temporários do usuário e do Windows.
- Caches de navegador e de apps (o app recria; não perde login nem configurações).
- Limpeza do Windows Update e otimização de entrega, pela tela Limpeza.
- Gravações de jogo esquecidas: pasta Vídeos › Capturas (Xbox Game Bar), pastas do NVIDIA App ou do AMD ReLive.
- Jogos que não joga mais: desinstale pelo launcher ou por Desinstalar apps, não apagando a pasta.

## Nunca à mão

- `C:\Windows\WinSxS`, `C:\Windows\Installer`, `C:\Windows\System32`, DriverStore: quebram atualizações, reparo e desinstalação. A aba Essenciais grandes explica cada uma.
- `pagefile.sys` e `hiberfil.sys`: são gerenciados pelo Windows. O hiberfil some se a hibernação for desligada, o que também desliga a Inicialização rápida.
- Pastas em Arquivos de Programas: use o desinstalador.
- AppData de apps que ainda usa (saves, perfis, cofres de senha).
- Prefetch: limpar deixa o Windows mais lento e é marcador de ferramenta suspeita para antivírus.

## Boas práticas

- Deixe 10 a 15% do SSD livre: abaixo disso o desempenho de escrita e a vida útil sofrem, e atualizações grandes falham.
- Cache de shader: limpar resolve corrupção rara, mas recompila tudo e causa travadas nas primeiras partidas. Não é rotina.
- Discos virtuais grandes (WSL `ext4.vhdx`, Docker, máquinas virtuais) não encolhem sozinhos; a compactação é feita pela própria ferramenta.
- Bibliotecas de jogos: mova pelo launcher (Steam: Configurações › Armazenamento) para outro disco, nunca copiando a pasta.
- Mostre tamanhos com a fonte do número (varredura do disco, prévia da limpeza) e nunca prometa espaço que não foi medido.
