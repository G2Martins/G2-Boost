# Pronto para jogar com anti-cheat

**Última revisão:** 2026-11-20 (rodada 11, bloco A3) · Código: `crates/core/src/anticheat.rs`, `firmware.rs` (DeviceGuard, GPT, item `iommu` do guia), `advisor.rs` (achados), `context.rs` (pacote da IA); shell `src-tauri/src/anticheat_cmds.rs`; MCP `get_anticheat_readiness`; UI `src/components/games/anticheat-panel.tsx` (Jogos, Meu PC), aviso do `hvci_off` em `src/pages/boost.tsx`.

## O que é

Painel que diz, por jogo instalado, se o anti-cheat dele vai abrir neste PC e o que falta, com o caminho no menu da BIOS da placa-mãe do usuário. Sem jogo desses instalado, mostra a tabela geral (o que cada anti-cheat pede e como este PC está). Mudar BIOS pelo Windows **não é possível** em placa consumer (Secure Boot não é alterável pelo SO por design): o app detecta, guia, reinicia direto na BIOS (UEFI, admin, confirmação) e confere de novo no próximo start.

## Requisitos (pesquisa 2026-11)

| Jogo | Anti-cheat | Exige | Usa se tiver | Modo sob demanda |
|---|---|---|---|---|
| Valorant, League of Legends | Riot Vanguard | UEFI, GPT, Secure Boot, TPM 2.0 (VAN9001/9003 no Win11) | — | Win11 25H2+, Secure Boot, TPM 2.0, VBS, HVCI, IOMMU |
| FACEIT (CS2) | FACEIT AC | UEFI, GPT, Secure Boot, TPM 2.0 (desde 25/11/2025), Windows 11 (desde out/2026) | VBS, IOMMU | — |
| Battlefield 6 | EA Javelin | UEFI, GPT, Secure Boot, TPM 2.0 | — | — |
| Call of Duty BO7 / Warzone | Ricochet | UEFI, GPT, Secure Boot, TPM 2.0 | — | — |

UEFI e GPT entram como pré-requisito do Secure Boot (o UEFI só inicia o Windows de disco GPT). EAC/BattlEye em geral não pedem firmware: fora da tabela. A matriz é dado (`anticheat::TITLES`); mudou o requisito de um anti-cheat, muda ali e no teste.

## Checagens (`anticheat::Check`, tudo sem admin)

| Check | Fonte | Ok quando |
|---|---|---|
| `tpm2` | `Tbsi_GetDeviceInfo` (perfil) + `Win32_Tpm` `IsEnabled/IsActivated_InitialValue` (só elevado) | versão 2.0 e não "desligado/não ativado" |
| `secure_boot` | `HKLM\…\SecureBoot\State\UEFISecureBootEnabled` | 1 (sem a chave em modo legado = falta) |
| `uefi` | `GetFirmwareType` | UEFI |
| `gpt` | `IOCTL_DISK_GET_PARTITION_INFO_EX` em `\\.\C:` (`PartitionStyle` 1) | GPT (UEFI sem resposta = GPT) |
| `vbs`, `hvci`, `iommu` | WMI `Win32_DeviceGuard` (Microsoft Learn, "Enable memory integrity"): `VirtualizationBasedSecurityStatus` = 2; `SecurityServicesRunning` contém 2; `AvailableSecurityProperties` contém 3 (proteção de DMA = IOMMU ligado) | idem |
| `win11`, `win25h2` | `CurrentBuildNumber` | ≥ 22000 / ≥ 26200 |
| `blocklist` | `CI\Config\VulnerableDriverBlocklistEnable` | 1; ausente no 22H2+ (padrão ligado); HVCI ligado sempre aplica |

Estado = `ok` / `missing` / `unknown` (não deu para ler: nunca chute). Cada check aponta a correção: item do guia da BIOS (`csm`, `secure_boot`, `tpm`, `virtualization`, `iommu`, com caminho por fabricante) ou passo do Windows (`mbr2gpt` antes do CSM, `core_isolation`, `update`, `blocklist`). VBS sem VT-x ligado aponta também a virtualização na BIOS.

## Detecção dos jogos

Só manifestos da Steam (`steam::installs`) e entradas de desinstalação (`apps::desktop_list`): nome + publisher, sem varrer pastas, sem spawn. Regras (nome normalizado para letras/dígitos): `valorant`/`leagueoflegends*`/`teamfighttactics*` com publisher Riot (o Teamfight Tactics tem entrada própria e roda sob o mesmo Vanguard do League); `faceit*` com publisher FACEIT; Battlefield 6 pela appid 2807960 ou nome `battlefield6` (EA); Call of Duty pela appid 1938090 (HQ) ou nome `callofduty`/`blackops7`/`warzone` (Activision/Blizzard). CoD antigos ("Black Ops III") e homônimos de outro publisher não contam. Nomes são dado hostil: sem caracteres de controle nem de direção (bidi), até 80 caracteres.

## Re-checagem depois do boot

No setup do app (thread, 20 s depois de abrir) `anticheat::recheck` compara com `anticheat.json` no data dir e chama `notify` (rota `games`, até 6 linhas): por check que virou (`anticheat.notify.<check>.on|off`, ex.: "Secure Boot ligado desde o último boot", "O TPM 2.0 sumiu…") e por jogo que ficou pronto ou bloqueado (`anticheat.notify.ready|blocked` com `{game}`). Primeira execução só grava a base. Leitura `unknown` mantém o último valor conhecido (abrir sem admin não vira "mudou"). O arquivo é entrada hostil: > 64 KB, JSON inválido ou check desconhecido = sem snapshot (sem aviso); ids de jogo desconhecidos descartados. A tarefa de logon abre o app logo após o boot, então o aviso chega sem o usuário abrir nada.

## Integridade de memória (decisão do dono, 2026-11)

`hvci_off` com jogo que usa VBS/HVCI/IOMMU instalado (`Readiness.vbs_games`): **só avisa**. Antes de ligar o ajuste a UI abre um modal citando o jogo ("Manter ligada" é o botão principal; "Desligar mesmo assim" segue). Não bloqueia. O conselheiro troca `hvci_on` ("desligar é opção") por `hvci_needed` ("o jogo X usa: mantenha").

## Achados do conselheiro

`anticheat_blocked` (Aviso, por jogo, `{game}` + `{count}` faltando), `iommu_off` (Info, só com jogo que usa IOMMU), `vuln_driver_blocklist_off` (Aviso, sempre), `hvci_needed` (Info). Os antigos `secure_boot_off`, `tpm_missing`, `legacy_boot` continuam.

## IA e MCP

Pacote de contexto: `anticheat` = estados dos checks + jogos instalados (nome, anti-cheat, pronto, faltando, sob demanda), nada mais. MCP `get_anticheat_readiness` (só leitura) devolve a prontidão completa com o guia. Base da Luli: seção em `knowledge/04-hardware-e-upgrades.md`.

## Testes

`anticheat`: matriz jogo × requisito com fatos sintéticos (PC pronto, Secure Boot off, 24H2 sem VBS/VT-d, Windows 10 com FACEIT, PC legado completo), estados isolados (TPM 1.2, TPM não ativado, GPT inferido do UEFI, blocklist), detecção pela Steam e pelo Uninstall (incluindo falsos), nomes hostis (bidi, controle, 500 caracteres), diff depois do boot (Secure Boot ligado, TPM sumido, leitura desconhecida), arquivo de snapshot hostil, leitura ao vivo. `firmware`: `Win32_DeviceGuard` sintético (PC do dono: VBS + HVCI, sem DMA), DeviceGuard/GPT ao vivo, item `iommu` em todo fabricante. `advisor`: `hvci_needed` no lugar de `hvci_on` com Valorant, nada com só Battlefield 6, `anticheat_blocked` por jogo. e2e `e2e/anticheat.spec.ts`: Jogos e Meu PC fora do Tauri mostram o estado honesto (sem veredito, sem botão de reiniciar).

**Não verificado:** reinício na BIOS e re-checagem com mudança real de firmware (exige o dono na BIOS); `Win32_Tpm` elevado em TPM desligado; caminhos de menu do IOMMU por fabricante (melhor esforço, "os nomes mudam").

Na máquina do dono (2026-11-20): UEFI, GPT, Secure Boot, TPM 2.0, VBS + HVCI, 25H2, blocklist ligados; **IOMMU desligado** (VT-d off, `AvailableSecurityProperties` sem 3); League of Legends instalado = pronto, modo sob demanda ainda não (falta o IOMMU).
