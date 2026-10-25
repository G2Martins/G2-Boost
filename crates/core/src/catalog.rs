//! The apps the **Instalar** tab offers, installed through winget (official `winget` source only).
//!
//! One table, three readers: the tab's grid, the batch installer's allowlist (`install::resolve`
//! refuses any id that is not here) and the post-format wizard's "essentials" preset.
//!
//! Rules for an entry (the tests below enforce what a test can):
//! - an exact winget id of the official source (every id was checked against its index on
//!   2026-10-25; `live_ids_exist_in_the_official_source` re-checks them with `winget search --exact`);
//! - no PC optimizer, cleaner or booster, no cracked or grey software, and none of the temperature
//!   tools the owner decided not to recommend (kernel drivers flagged as vulnerable, CLAUDE.md §6);
//! - `same_as` lists other ids of the same app (a locale build, an older line): installed under any of
//!   them counts as installed, and only the primary id is ever installed.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Browsers,
    Communication,
    Games,
    Media,
    Creative,
    Dev,
    Utilities,
    Office,
    Security,
    Hardware,
    Network,
    SelfHosted,
}

impl Category {
    /// The order the tab lists them in.
    pub const ALL: [Category; 12] = [
        Category::Browsers,
        Category::Communication,
        Category::Games,
        Category::Media,
        Category::Creative,
        Category::Dev,
        Category::Utilities,
        Category::Office,
        Category::Security,
        Category::Hardware,
        Category::Network,
        Category::SelfHosted,
    ];
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Entry {
    /// Exact winget id (`Publisher.App`): the only handle the UI sends back.
    pub id: &'static str,
    /// Display name (a brand, so not translated).
    pub name: &'static str,
    pub category: Category,
    /// Part of the post-format "essentials" preset.
    pub essential: bool,
    /// Other ids of the same app; the UI never needs them.
    #[serde(skip)]
    pub same_as: &'static [&'static str],
}

const fn app(id: &'static str, name: &'static str, category: Category) -> Entry {
    Entry { id, name, category, essential: false, same_as: &[] }
}

const fn ess(e: Entry) -> Entry {
    Entry { essential: true, ..e }
}

const fn also(e: Entry, same_as: &'static [&'static str]) -> Entry {
    Entry { same_as, ..e }
}

use Category::*;

/// Curated order: the apps most people look for come first inside each category.
pub const CATALOG: &[Entry] = &[
    // Navegadores
    ess(app("Google.Chrome", "Google Chrome", Browsers)),
    also(app("Mozilla.Firefox.pt-BR", "Mozilla Firefox", Browsers), &["Mozilla.Firefox"]),
    app("Microsoft.Edge", "Microsoft Edge", Browsers),
    app("Brave.Brave", "Brave", Browsers),
    app("Opera.Opera", "Opera", Browsers),
    app("Opera.OperaGX", "Opera GX", Browsers),
    app("Vivaldi.Vivaldi", "Vivaldi", Browsers),
    app("TorProject.TorBrowser", "Tor Browser", Browsers),
    app("LibreWolf.LibreWolf", "LibreWolf", Browsers),
    app("Zen-Team.Zen-Browser", "Zen Browser", Browsers),
    // Comunicação
    ess(app("Discord.Discord", "Discord", Communication)),
    app("Microsoft.Teams", "Microsoft Teams", Communication),
    app("SlackTechnologies.Slack", "Slack", Communication),
    app("Zoom.Zoom", "Zoom Workplace", Communication),
    app("Telegram.TelegramDesktop", "Telegram", Communication),
    app("OpenWhisperSystems.Signal", "Signal", Communication),
    also(app("Mozilla.Thunderbird.pt-BR", "Mozilla Thunderbird", Communication), &["Mozilla.Thunderbird"]),
    app("Element.Element", "Element", Communication),
    app("TeamSpeakSystems.TeamSpeakClient", "TeamSpeak", Communication),
    // Jogos e launchers
    ess(app("Valve.Steam", "Steam", Games)),
    app("EpicGames.EpicGamesLauncher", "Epic Games Launcher", Games),
    app("GOG.Galaxy", "GOG Galaxy", Games),
    app("ElectronicArts.EADesktop", "EA app", Games),
    app("Ubisoft.Connect", "Ubisoft Connect", Games),
    app("Blizzard.BattleNet", "Battle.net", Games),
    app("RockstarGames.Launcher", "Rockstar Games Launcher", Games),
    app("RiotGames.Valorant.BR", "VALORANT (Brasil)", Games),
    app("RiotGames.LeagueOfLegends.BR", "League of Legends (Brasil)", Games),
    app("Roblox.Roblox", "Roblox", Games),
    app("Mojang.MinecraftLauncher", "Minecraft Launcher", Games),
    app("PrismLauncher.PrismLauncher", "Prism Launcher", Games),
    app("Playnite.Playnite", "Playnite", Games),
    app("HeroicGamesLauncher.HeroicGamesLauncher", "Heroic Games Launcher", Games),
    app("Nvidia.GeForceNow", "GeForce NOW", Games),
    // Multimídia
    ess(app("VideoLAN.VLC", "VLC", Media)),
    app("Spotify.Spotify", "Spotify", Media),
    app("Apple.iTunes", "iTunes", Media),
    app("clsid2.mpc-hc", "MPC-HC", Media),
    app("Daum.PotPlayer", "PotPlayer", Media),
    app("PeterPawlowski.foobar2000", "foobar2000", Media),
    app("HandBrake.HandBrake", "HandBrake", Media),
    app("MoritzBunkus.MKVToolNix", "MKVToolNix", Media),
    app("Plex.Plex", "Plex", Media),
    app("XBMCFoundation.Kodi", "Kodi", Media),
    app("CodecGuide.K-LiteCodecPack.Standard", "K-Lite Codec Pack", Media),
    app("Gyan.FFmpeg", "FFmpeg", Media),
    // Criação
    app("OBSProject.OBSStudio", "OBS Studio", Creative),
    app("Streamlabs.Streamlabs", "Streamlabs Desktop", Creative),
    also(app("GIMP.GIMP.3", "GIMP", Creative), &["GIMP.GIMP", "GIMP.GIMP.2"]),
    app("Inkscape.Inkscape", "Inkscape", Creative),
    app("BlenderFoundation.Blender", "Blender", Creative),
    app("KDE.Krita", "Krita", Creative),
    app("Meltytech.Shotcut", "Shotcut", Creative),
    app("KDE.Kdenlive", "Kdenlive", Creative),
    app("OpenShot.OpenShot", "OpenShot", Creative),
    app("Figma.Figma", "Figma", Creative),
    app("dotPDN.PaintDotNet", "Paint.NET", Creative),
    app("Adobe.CreativeCloud", "Adobe Creative Cloud", Creative),
    app("Audacity.Audacity", "Audacity", Creative),
    app("GodotEngine.GodotEngine", "Godot Engine", Creative),
    // Desenvolvimento
    app("Microsoft.VisualStudioCode", "Visual Studio Code", Dev),
    app("Git.Git", "Git", Dev),
    app("GitHub.cli", "GitHub CLI", Dev),
    app("GitHub.GitHubDesktop", "GitHub Desktop", Dev),
    app("Python.Python.3.14", "Python 3.14", Dev),
    app("OpenJS.NodeJS.LTS", "Node.js LTS", Dev),
    app("Microsoft.PowerShell", "PowerShell 7", Dev),
    app("Microsoft.WindowsTerminal", "Windows Terminal", Dev),
    app("Microsoft.VisualStudio.Community", "Visual Studio 2026 Community", Dev),
    app("JetBrains.Toolbox", "JetBrains Toolbox", Dev),
    app("Docker.DockerDesktop", "Docker Desktop", Dev),
    app("Notepad++.Notepad++", "Notepad++", Dev),
    app("Postman.Postman", "Postman", Dev),
    app("Microsoft.DotNet.SDK.10", ".NET SDK 10", Dev),
    app("Anysphere.Cursor", "Cursor", Dev),
    app("Google.AndroidStudio", "Android Studio", Dev),
    app("Microsoft.WSL", "Windows Subsystem for Linux", Dev),
    app("Anthropic.ClaudeCode", "Claude Code", Dev),
    app("EclipseAdoptium.Temurin.21.JDK", "Java 21 JDK (Temurin)", Dev),
    // Utilitários
    ess(app("7zip.7zip", "7-Zip", Utilities)),
    app("RARLab.WinRAR", "WinRAR", Utilities),
    app("Microsoft.PowerToys", "PowerToys", Utilities),
    app("voidtools.Everything", "Everything", Utilities),
    app("ShareX.ShareX", "ShareX", Utilities),
    app("AutoHotkey.AutoHotkey", "AutoHotkey", Utilities),
    app("Rufus.Rufus", "Rufus", Utilities),
    app("Balena.Etcher", "balenaEtcher", Utilities),
    app("Ventoy.Ventoy", "Ventoy", Utilities),
    app("QL-Win.QuickLook", "QuickLook", Utilities),
    app("Microsoft.Sysinternals.ProcessExplorer", "Process Explorer", Utilities),
    app("LocalSend.LocalSend", "LocalSend", Utilities),
    app("Anthropic.Claude", "Claude", Utilities),
    app("OpenAI.ChatGPT", "ChatGPT", Utilities),
    ess(app("Microsoft.VCRedist.2015+.x64", "Visual C++ Redistributable (x64)", Utilities)),
    app("Microsoft.VCRedist.2015+.x86", "Visual C++ Redistributable (x86)", Utilities),
    ess(app("Microsoft.DotNet.DesktopRuntime.8", ".NET Desktop Runtime 8", Utilities)),
    app("Microsoft.DotNet.DesktopRuntime.10", ".NET Desktop Runtime 10", Utilities),
    ess(app("Microsoft.DirectX", "DirectX End-User Runtime", Utilities)),
    app("EclipseAdoptium.Temurin.21.JRE", "Java 21 (Temurin)", Utilities),
    // Escritório e documentos
    app("TheDocumentFoundation.LibreOffice", "LibreOffice", Office),
    app("Microsoft.Office", "Microsoft 365 (Office)", Office),
    also(app("Adobe.Acrobat.Reader.64-bit", "Adobe Acrobat Reader", Office), &["Adobe.Acrobat.Reader.32-bit"]),
    app("SumatraPDF.SumatraPDF", "SumatraPDF", Office),
    app("ONLYOFFICE.DesktopEditors", "ONLYOFFICE", Office),
    app("Obsidian.Obsidian", "Obsidian", Office),
    app("Notion.Notion", "Notion", Office),
    app("DigitalScholar.Zotero", "Zotero", Office),
    app("JGraph.Draw", "draw.io", Office),
    app("Google.GoogleDrive", "Google Drive", Office),
    app("Dropbox.Dropbox", "Dropbox", Office),
    app("Microsoft.OneDrive", "OneDrive", Office),
    // Segurança e privacidade
    app("Bitwarden.Bitwarden", "Bitwarden", Security),
    app("KeePassXCTeam.KeePassXC", "KeePassXC", Security),
    app("AgileBits.1Password", "1Password", Security),
    app("Proton.ProtonPass", "Proton Pass", Security),
    app("Malwarebytes.Malwarebytes", "Malwarebytes", Security),
    app("Proton.ProtonVPN", "Proton VPN", Security),
    app("MullvadVPN.MullvadVPN", "Mullvad VPN", Security),
    app("WireGuard.WireGuard", "WireGuard", Security),
    app("Cloudflare.Warp", "Cloudflare WARP", Security),
    app("IDRIX.VeraCrypt", "VeraCrypt", Security),
    app("Cryptomator.Cryptomator", "Cryptomator", Security),
    app("GnuPG.Gpg4win", "Gpg4win", Security),
    app("Henry++.simplewall", "simplewall", Security),
    // Hardware e drivers
    app("Intel.IntelDriverAndSupportAssistant", "Intel Driver & Support Assistant", Hardware),
    app("Dell.CommandUpdate", "Dell Command Update", Hardware),
    app("Lenovo.SystemUpdate", "Lenovo System Update", Hardware),
    app("Asus.ArmouryCrate", "ASUS Armoury Crate", Hardware),
    app("MSI.MSICenter", "MSI Center", Hardware),
    app("NZXT.CAM", "NZXT CAM", Hardware),
    app("Logitech.GHUB", "Logitech G HUB", Hardware),
    app("Logitech.OptionsPlus", "Logi Options+", Hardware),
    app("Corsair.iCUE.5", "Corsair iCUE", Hardware),
    app("SteelSeries.GG", "SteelSeries GG", Hardware),
    app("RazerInc.RazerInstaller.Synapse4", "Razer Synapse 4", Hardware),
    app("Elgato.StreamDeck", "Elgato Stream Deck", Hardware),
    app("REALiX.HWiNFO", "HWiNFO", Hardware),
    app("LibreHardwareMonitor.LibreHardwareMonitor", "Libre Hardware Monitor", Hardware),
    app("CrystalDewWorld.CrystalDiskInfo", "CrystalDiskInfo", Hardware),
    app("CrystalDewWorld.CrystalDiskMark", "CrystalDiskMark", Hardware),
    app("Wagnardsoft.DisplayDriverUninstaller", "Display Driver Uninstaller", Hardware),
    app("OpenRGB.OpenRGB", "OpenRGB", Hardware),
    // Rede e acesso remoto
    app("Tailscale.Tailscale", "Tailscale", Network),
    app("AnyDesk.AnyDesk", "AnyDesk", Network),
    app("TeamViewer.TeamViewer", "TeamViewer", Network),
    app("Microsoft.RemoteDesktopClient", "Remote Desktop (Microsoft)", Network),
    app("Parsec.Parsec", "Parsec", Network),
    app("MoonlightGameStreamingProject.Moonlight", "Moonlight", Network),
    app("LizardByte.Sunshine", "Sunshine", Network),
    app("WiresharkFoundation.Wireshark", "Wireshark", Network),
    app("WinSCP.WinSCP", "WinSCP", Network),
    app("PuTTY.PuTTY", "PuTTY", Network),
    app("angryziber.AngryIPScanner", "Angry IP Scanner", Network),
    app("Rclone.Rclone", "Rclone", Network),
    // Auto-hospedados e código aberto
    app("Ollama.Ollama", "Ollama", SelfHosted),
    app("ElementLabs.LMStudio", "LM Studio", SelfHosted),
    app("Jan.Jan", "Jan", SelfHosted),
    app("Syncthing.Syncthing", "Syncthing", SelfHosted),
    app("Nextcloud.NextcloudDesktop", "Nextcloud Desktop", SelfHosted),
    app("Jellyfin.Server", "Jellyfin Server", SelfHosted),
    app("Plex.PlexMediaServer", "Plex Media Server", SelfHosted),
    app("AdGuard.AdGuardHome", "AdGuard Home", SelfHosted),
    app("Oracle.VirtualBox", "VirtualBox", SelfHosted),
    app("RedHat.Podman-Desktop", "Podman Desktop", SelfHosted),
    app("SoftwareFreedomConservancy.QEMU", "QEMU", SelfHosted),
    app("Duplicati.Duplicati", "Duplicati", SelfHosted),
];

pub fn all() -> &'static [Entry] {
    CATALOG
}

/// The entry for `id`. winget ids are case-insensitive; anything else (a prefix, a padded or
/// decorated id, an alias from `same_as`) is `None`.
pub fn find(id: &str) -> Option<&'static Entry> {
    CATALOG.iter().find(|e| e.id.eq_ignore_ascii_case(id))
}

/// The default set for a freshly formatted PC: a browser, an archiver, a player, chat, the game
/// store and the runtimes games ask for. The post-format wizard starts from this list.
pub fn essentials() -> Vec<&'static Entry> {
    CATALOG.iter().filter(|e| e.essential).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn ids_are_unique_valid_and_shaped_like_winget_ids() {
        let mut seen = HashSet::new();
        for e in CATALOG {
            assert!(crate::winget::valid_id(e.id), "{} is not a valid winget id", e.id);
            assert!(e.id.contains('.'), "{}: winget ids are Publisher.App", e.id);
            assert!(seen.insert(e.id.to_ascii_lowercase()), "duplicate id {}", e.id);
            for s in e.same_as {
                assert!(crate::winget::valid_id(s), "{s}");
                assert!(!s.eq_ignore_ascii_case(e.id), "{} lists itself", e.id);
            }
        }
        // An alias never doubles as another entry's primary id (it would be installed twice).
        for e in CATALOG {
            for s in e.same_as {
                assert!(find(s).is_none(), "{s} is both an alias and an entry");
            }
        }
    }

    #[test]
    fn names_are_clean_ui_text() {
        for e in CATALOG {
            assert!((2..=40).contains(&e.name.chars().count()), "{}", e.name);
            assert_eq!(e.name, e.name.trim());
            assert!(!e.name.chars().any(char::is_control), "{}", e.name);
            // The UI never uses a dash as punctuation.
            for bad in [" - ", " \u{2013} ", " \u{2014} ", "--"] {
                assert!(!e.name.contains(bad), "{}", e.name);
            }
        }
    }

    #[test]
    fn every_category_is_present_and_the_size_is_about_right() {
        for c in Category::ALL {
            let n = CATALOG.iter().filter(|e| e.category == c).count();
            assert!(n >= 8, "{c:?} has only {n}");
        }
        assert!((140..=190).contains(&CATALOG.len()), "{}", CATALOG.len());
        // ALL lists each variant once.
        assert_eq!(Category::ALL.iter().collect::<HashSet<_>>().len(), 12);
    }

    #[test]
    fn nothing_the_owner_rules_out_is_offered() {
        // PC optimizers, cleaners, boosters, cracked or grey software, and temperature tools whose
        // drivers were flagged as vulnerable (CLAUDE.md §6).
        const OUT: &[&str] = &[
            "cleaner", "optimizer", "booster", "tweaker", "systemcare", "wise care", "glary", "iobit", "cortex",
            "bleachbit", "pc manager", "quickclean", "crack", "keygen", "activator", "kmspico", "patch",
            "afterburner", "coretemp", "core temp", "speccy", "nvcleanstall",
        ];
        for e in CATALOG {
            let hay = format!("{} {}", e.id, e.name).to_lowercase();
            for w in OUT {
                assert!(!hay.contains(w), "{} matches the exclusion '{w}'", e.id);
            }
        }
    }

    #[test]
    fn essentials_are_a_small_sensible_preset() {
        let ids: Vec<&str> = essentials().iter().map(|e| e.id).collect();
        for want in ["Google.Chrome", "7zip.7zip", "VideoLAN.VLC", "Discord.Discord", "Valve.Steam"] {
            assert!(ids.contains(&want), "{want} missing from {ids:?}");
        }
        assert!((5..=12).contains(&ids.len()), "{ids:?}");
        assert!(essentials().iter().all(|e| find(e.id).is_some()));
    }

    #[test]
    fn find_is_exact_but_not_case_sensitive() {
        assert_eq!(find("google.chrome").map(|e| e.id), Some("Google.Chrome"));
        assert_eq!(find("Mozilla.Firefox.pt-BR").map(|e| e.name), Some("Mozilla Firefox"));
        for bad in ["", "Google", "Google.Chrome ", " Google.Chrome", "Google.Chrom", "Google.Chrome\0", "Mozilla.Firefox", "--id"] {
            assert!(find(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn serializes_for_the_ui_without_the_aliases() {
        let v = serde_json::to_value(find("Oracle.VirtualBox").unwrap()).unwrap();
        assert_eq!(v["category"], "self_hosted");
        assert_eq!(v["essential"], false);
        assert!(v.get("same_as").is_none());
        assert_eq!(serde_json::to_value(Category::Dev).unwrap(), "dev");
    }

    /// Talks to the real winget (about 3 s per id): `cargo test -p g2boost-core -- --ignored live_ids`.
    #[test]
    #[ignore]
    fn live_ids_exist_in_the_official_source() {
        let mut missing = Vec::new();
        for e in CATALOG {
            match crate::winget::exact(e.id) {
                Ok(p) => println!("ok      {} -> {}", e.id, p.name),
                Err(err) => {
                    println!("MISSING {} ({err})", e.id);
                    missing.push(e.id);
                }
            }
        }
        assert!(missing.is_empty(), "{missing:?}");
    }
}
