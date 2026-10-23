# GPU: configurações 3D do driver (NVIDIA, AMD beta)

**Última revisão:** 2026-10-22 · Código: `crates/core/src/nvapi.rs` (NVIDIA), `adlx.rs` (AMD, beta), `gpu.rs` (seção por vendor), `hardware::gpus` (DXGI); shell `src-tauri/src/lib.rs` (`gpu_*`, `gpu_target`, `--gpu-helper` no topo de `run()`); MCP `get_gpu_settings` (`crates/mcp/src/lib.rs`, só leitura); UI `src/pages/gpu.tsx` (rota `gpu`, depois de Jogos na barra lateral); testes `nvapi::tests`, `adlx::tests`, `gpu::tests`, `e2e/gpu.spec.ts`.

## Qual seção aparece

Pelo vendor id do DXGI (`hardware::gpus`, adaptador de software fora): `0x10DE` NVIDIA, `0x1002`/`0x1022` AMD, `0x8086` Intel. Ordem de preferência NVIDIA → AMD → Intel (`gpu::section`): notebook híbrido mostra a placa dedicada (Intel ou APU AMD + GeForce = NVIDIA; Intel + Radeon = AMD). Só Intel = "Em breve" com os dados da placa. Nenhuma = estado vazio. O cabeçalho mostra nome, driver (versão + ramo pela NVAPI na NVIDIA), VRAM, PCIe e as métricas ao vivo do monitor (`monitor:tick`: uso, temperatura, clock, potência, VRAM em uso, linha de 60 s).

## NVIDIA: NVAPI DRS (`nvapi.rs`)

- `nvapi64.dll` carregada só de System32 (`LoadLibraryExW` + `LOAD_LIBRARY_SEARCH_SYSTEM32`); funções por id via `nvapi_QueryInterface`. Cada operação abre `NvAPI_Initialize` → `DRS_CreateSession` → `LoadSettings` e fecha `DestroySession` → `NvAPI_Unload` (RAII), uma sessão por vez (mutex).
- **Fonte dos ids e layouts:** SDK público da NVIDIA, `github.com/NVIDIA/nvapi` (MIT), commit `70d337db9186e968eab622f7e786de7e437faf3d` (2026-09-18): `nvapi_interface.h` (ids das funções), `nvapi.h` (`NVDRS_SETTING_V1` 12320 bytes / versão `0x13020`, `NVDRS_PROFILE_V1` `0x11014`, `NVDRS_APPLICATION_V4` `0x4500C`, conferidos por teste), `nvapi_lite_common.h` (status), `NvApiDriverSettings.h` (ids e valores).
- **Alvo:** perfil global (`GetCurrentGlobalProfile`; aqui "Base Profile") ou o perfil do executável do jogo (`FindApplicationByName`). Jogo sem perfil: a leitura mostra os valores globais; o primeiro ajuste cria `G2 Boost <exe>` (`CreateProfile` + `CreateApplication`). O jogo vem da lista `games_list` (a UI manda o id; o shell resolve o exe principal), nunca um nome livre do webview; `nvapi::valid_exe` exige nome de arquivo `.exe` sem caminho.

| Ajuste (`id`) | DRS id | Opções → valor | Recomendado | Também conta como bom |
|---|---|---|---|---|
| Modo de baixa latência (`low_latency`) | `PRERENDERLIMIT 0x007BA09E` + `0x0005F543` (CPL state) + `0x10835000` (ULL enabled) | off `0,0,0` · on `1,1,0` · ultra `1,2,1` | on | ultra |
| Gerenciamento de energia (`power`) | `PREFERRED_PSTATE 0x1057EB71` | normal `5` · adaptive `0` · max `1` | max | |
| Taxa máxima de quadros (`fps_limit`) | `FRL_FPS 0x10835002` | 0 (desligado) ou 20..1000 | nenhum (depende do monitor) | |
| Sincronização vertical (`vsync`) | `VSYNCMODE 0x00A879CF` | app `0x60925292` · off `0x08416747` · on `0x47814940` · fast `0x18888888` | nenhum (depende do monitor) | |
| Cache de shaders (`shader_cache`) | `PS_SHADERDISKCACHE_MAX_SIZE 0x00AC8497` (MB) | default `0x4000` · 5gb `5120` · 10gb `10240` · 100gb `102400` · unlimited `0xFFFFFFFF` | 10gb | default, 100gb, unlimited |
| Filtragem de textura (`texture_quality`) | `QUALITY_ENHANCEMENTS 0x00CE2691` | high_quality `0xFFFFFFF6` · quality `0` · performance `0xA` · high_performance `0x14` | high_performance | performance |
| Otimização com threads (`threaded`) | `OGL_THREAD_CONTROL 0x20C1221E` | auto `0` · on `1` · off `2` | auto | on |

- **Baixa latência Ultra = mais de um id.** Os ids `0x0005F543` ("Ultra Low Latency - CPL State": 0/1/2) e `0x10835000` ("Ultra Low Latency - Enabled": 0/1) **não estão no header público**; o driver também não devolve nome para eles (`GetSettingNameFromId` = `SETTING_NOT_FOUND`). Fonte: tabela pública de nomes do NVIDIA Profile Inspector (`nvidiaProfileInspector/CustomSettingNames.xml`, MIT, commit `2994194`), que também traz a recomendação de usar pré-renderizados = 1 com o Ultra. A leitura usa o interruptor mais forte (`detect_low_latency`: ULL enabled = 1 → Ultra; pré-renderizados = 1 → Ligado; tudo 0 → Desligado), então um "Ligado" gravado só como pré-renderizados = 1 (o caso desta máquina) lê certo.
- **Allowlist:** só ids e opções da tabela chegam a `SetSetting`; valor de cache em MB confirmado na máquina do dono (100 GB = `0x19000`) e na mesma tabela pública.
- **Backup e desfazer:** antes da primeira mudança de cada DRS id por alvo, `gpu_settings_backup.json` (data dir) guarda o valor do usuário naquele perfil, ou `null` se não havia (herdado ou predefinido da NVIDIA). Mudanças seguintes não sobrescrevem o original. Desfazer grava o valor guardado ou apaga o valor do usuário (`DeleteProfileSetting`: volta ao padrão do driver ou ao predefinido do jogo). Sem backup, desfazer apaga. Perfil criado pelo G2 Boost é removido (`DeleteProfile`) quando o último ajuste dele é desfeito. O arquivo é hostil: entradas com id fora da tabela, alvo que não é `global`/exe válido ou valor fora da faixa do header são descartadas (essas voltam ao padrão em vez de um valor arbitrário). Backup gravado antes do `SaveSettings`; se o driver recusar, o arquivo volta ao estado anterior.
- **"Aplicar recomendados"** (com confirmação na UI listando atual → recomendado) muda só o que não está bom (nunca rebaixa Ultra, 100 GB etc.); jogo sem perfil só ganha perfil se algo mudar.
- **Admin:** não precisa. A pasta `C:\ProgramData\NVIDIA Corporation\Drs` dá controle total a Todos; `SaveSettings` funcionou com o app sem elevação. Se um driver devolver `INVALID_USER_PRIVILEGE`/`ACCESS_DENIED`, o core responde `needs_admin` e a UI mostra Elevar (rota `gpu` está na allowlist do `--route`).
- Os jogos leem o perfil ao abrir: a UI avisa para reabrir o jogo.

### Verificado nesta máquina (2026-10-22, usuário comum)

Driver 617.14, ramo `r617_07`, perfil global "Base Profile". Leitura ao vivo (`live_read_global_profile`): low_latency **on** (`0x1 0x0 0x0`, definido pelo usuário) · power **max** (`0x1`, usuário) · fps_limit **0** (padrão) · vsync **app** (`0x60925292`, padrão) · shader_cache **100gb** (`0x19000`, usuário) · texture_quality **high_performance** (`0x14`, usuário) · threaded **auto** (`0x0`, padrão). Nomes e listas de valores do próprio driver (`live_probe_names_and_values`) batem com o header para os 7 ids públicos. Ida e volta de escrita no global sem admin (`live_round_trip_fps_limit`, rodado à mão com `G2BOOST_LIVE_WRITE=1`): 0 → 1000 (`0x3E8`, ajustado, com backup) → desfazer → 0, sem valor do usuário e sem backup sobrando. **Não conferido:** comparação visual com o painel de controle da NVIDIA (feita só por leitura dos valores); perfil por jogo ao vivo (criar/remover perfil) não foi exercitado nesta máquina.

## AMD: ADLX (`adlx.rs`), **beta**

- **Não testado em placa AMD** (a máquina de desenvolvimento é NVIDIA). Escrito contra o SDK oficial: `github.com/GPUOpen-LibrariesAndSDKs/ADLX` (MIT), commit `32b5a740d42295c5dfe9026b9f52683da0f3af91` (2026-09-21), headers `ADLX.h`, `ADLXDefines.h`, `ISystem.h`, `I3DSettings.h`, `ICollections.h`, `ADLXStructures.h`, `ADLXVersion.h` (2.0.0.125). As vtables C são structs `#[repr(C)]` campo a campo (slots não usados como `usize`), com o tamanho conferido em `const` (ex.: `GetFrameRateTargetControl` = slot 9 de `IADLX3DSettingsServices`).
- Fluxo: `amdadlx64.dll` de System32 → `ADLXQueryFullVersion` → `ADLXInitialize` (ou `ADLXInitialize2`) com a menor versão entre driver e SDK → `IADLXSystem::GetGPUs` + `Get3DSettingsServices` → por GPU: Anti-Lag, Chill, Boost, Image Sharpening, Enhanced Sync, Frame Rate Target Control (supported, enabled, valor e faixa) → `Release` de tudo → `ADLXTerminate`.
- **Isolamento de travamento:** toda chamada ADLX roda num processo filho, o próprio `G2 Boost.exe --gpu-helper <json>` (sem shell, flag fixa, `CREATE_NO_WINDOW`, stdin fechado, saída limitada a 64 KB, prazo de 15 s e kill). O pedido é validado nos dois lados (`Request::parse`: `deny_unknown_fields`, ≤ 1 KB, `cmd` read/set, GPU < 8, até 6 mudanças sem repetir recurso, valor só para nitidez 0..100 e FRTC 20..1000, e ainda conferido contra a faixa do driver). A resposta também é hostil (`parse_answer`: uma linha JSON, ≤ 8 GPUs, nome sem controle e cortado). Filho que trava, sai com erro ou responde lixo = `helper_failed` → a UI mostra "Não foi possível ler a placa AMD." O binário de dev do MCP (`g2boost-mcp.exe`) também atende a flag.
- Recomendado: Anti-Lag ligado; Chill, Boost, Enhanced Sync e limite de FPS desligados; nitidez = gosto. Sem backup próprio: cada recurso é um interruptor visível e desfazer é ligar/desligar de novo (dito na confirmação).
- Verificado aqui só o caminho sem AMD: o helper responde `no_amd` em vez de cair (`helper_reports_missing_driver_or_bad_input`).

## Intel

Só informações da placa e métricas ao vivo ("Em breve"). Caminho futuro: IGCL (Intel Graphics Control Library), não iniciado.

## MCP

`get_gpu_settings` (só leitura, último da lista): visão geral (GPUs, seção, driver NVIDIA), perfil global ou de um `exe` (`nvapi::Target::game` valida o nome; leitura nunca cria perfil) e, em AMD, o estado pelo helper com `beta: true`. Mudanças só pela tela GPU do app. `open_in_g2boost` aceita a rota `gpu`.

## Pendências

- Conferir no painel de controle da NVIDIA que Ligado/Ultra gravados pelo G2 Boost aparecem iguais (e o inverso), e exercitar um perfil por jogo (criar → desfazer → perfil removido).
- Testar a seção AMD numa placa Radeon (leitura, cada interruptor, FRTC e nitidez, helper com driver antigo).
- Intel via IGCL.
