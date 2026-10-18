//! Console G2: a command line for the app's own native diagnostics. Not a shell: a fixed list
//! of commands, each mapped to a core function (ICMP, routes, DNS, processes, disks, power,
//! apps, Event Log, hardware, sensors, startup). Nothing here changes the PC; navigation and
//! clearing are UI effects. The line is hostile input: bounded, tokenized here, every argument
//! validated by the command that reads it.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::Serialize;

use crate::cleanup::TokenStore;
use crate::{apps, devices, disk, dns, hardware, network, power, procs, sensors, settings, startup, system, winevents, winget};

const MAX_LINE: usize = 512;

/// One command: canonical name, aliases (pt-BR first, then en), usage shown in help.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Command {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub usage: &'static str,
}

pub const COMMANDS: &[Command] = &[
    Command { name: "ajuda", aliases: &["help", "?"], usage: "ajuda" },
    Command { name: "ping", aliases: &[], usage: "ping <host> [vezes]" },
    Command { name: "rota", aliases: &["tracert", "traceroute"], usage: "rota <host>" },
    Command { name: "tcping", aliases: &[], usage: "tcping <host>[:porta]" },
    Command { name: "dns", aliases: &["nslookup", "resolver"], usage: "dns <nome>" },
    Command { name: "rede", aliases: &["ip", "ipconfig", "network"], usage: "rede" },
    Command { name: "dispositivos", aliases: &["devices", "lan"], usage: "dispositivos" },
    Command { name: "processos", aliases: &["top", "ps", "processes"], usage: "processos [quantos]" },
    Command { name: "disco", aliases: &["df", "drives"], usage: "disco" },
    Command { name: "pasta", aliases: &["du", "folder"], usage: "pasta <caminho> [segundos]" },
    Command { name: "energia", aliases: &["power", "plans"], usage: "energia" },
    Command { name: "apps", aliases: &["programas"], usage: "apps [filtro]" },
    Command { name: "erros", aliases: &["eventos", "errors", "events"], usage: "erros [dias]" },
    Command { name: "pc", aliases: &["hardware", "specs"], usage: "pc" },
    Command { name: "sensores", aliases: &["temps", "sensors"], usage: "sensores" },
    Command { name: "inicializacao", aliases: &["inicialização", "startup"], usage: "inicializacao" },
    Command {
        name: "winget",
        aliases: &["pacotes"],
        usage: "winget buscar <termo> | atualizacoes | instalar <id> | atualizar <id>",
    },
    Command { name: "abrir", aliases: &["open", "ir"], usage: "abrir <tela>" },
    Command { name: "limpar", aliases: &["clear", "cls"], usage: "limpar" },
];

/// Screens `abrir` can open: (accepted words, route).
const SCREENS: &[(&[&str], &str)] = &[
    (&["painel", "dashboard", "inicio", "início"], "dashboard"),
    (&["disco", "disk", "analisador"], "disk"),
    (&["limpeza", "cleanup"], "cleanup"),
    (&["apps", "desinstalar", "uninstall"], "apps"),
    (&["rede", "network"], "network"),
    (&["boost", "pcboost", "ajustes"], "boost"),
    (&["luli", "ia", "ai"], "ai"),
    (&["configuracoes", "configurações", "settings"], "settings"),
];

/// A message the UI translates (`console.<key>` with params).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Msg {
    pub key: &'static str,
    pub params: BTreeMap<&'static str, String>,
}

fn msg(key: &'static str, params: &[(&'static str, String)]) -> Msg {
    Msg { key, params: params.iter().cloned().collect() }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Block {
    /// Column ids are i18n keys (`console.col.<id>`); cells are data.
    Table { columns: Vec<&'static str>, rows: Vec<Vec<String>> },
    Note(Msg),
    /// A change waiting for the user's click: `console_confirm(token, action, id)` runs it.
    Confirm { action: &'static str, id: String, name: String, version: String, token: u64 },
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Effect {
    Help,
    Clear,
    Navigate { route: &'static str },
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Reply {
    pub ok: bool,
    /// The canonical command that ran (for the UI's history and Luli).
    pub command: Option<&'static str>,
    pub blocks: Vec<Block>,
    pub effect: Option<Effect>,
    pub error: Option<Msg>,
}

impl Reply {
    fn ok(command: &'static str, blocks: Vec<Block>) -> Self {
        Reply { ok: true, command: Some(command), blocks, effect: None, error: None }
    }
    fn effect(command: &'static str, e: Effect) -> Self {
        Reply { ok: true, command: Some(command), blocks: Vec::new(), effect: Some(e), error: None }
    }
    fn err(command: Option<&'static str>, m: Msg) -> Self {
        Reply { ok: false, command, blocks: Vec::new(), effect: None, error: Some(m) }
    }
}

/// Split on whitespace; double quotes group (for paths with spaces). No escapes, no expansion.
pub fn tokenize(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for c in line.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

pub fn find(word: &str) -> Option<&'static Command> {
    let w = word.to_lowercase();
    COMMANDS.iter().find(|c| c.name == w || c.aliases.contains(&w.as_str()))
}

/// Host names and IP literals only (letters, digits, dots, hyphens, IPv6 colons).
pub fn valid_host(h: &str) -> bool {
    (1..=253).contains(&h.len()) && h.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':'))
}

fn int_arg(v: Option<&String>, default: u64, min: u64, max: u64) -> Result<u64, ()> {
    match v {
        None => Ok(default),
        Some(s) => s.parse::<u64>().map(|n| n.clamp(min, max)).map_err(|_| ()),
    }
}

fn ms(v: f64) -> String {
    format!("{v:.1}")
}

fn mb(bytes: u64) -> String {
    format!("{:.0}", bytes as f64 / 1_048_576.0)
}

fn gb(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / 1_073_741_824.0)
}

fn yes(b: bool) -> String {
    if b { "✓".into() } else { "·".into() }
}

fn date(unix: u64) -> String {
    // Days since epoch → civil date (no time zone math needed for a log listing).
    let days = (unix / 86_400) as i64;
    let (z, era) = { let z = days + 719_468; (z, z.div_euclid(146_097)) };
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let secs = unix % 86_400;
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}", secs / 3600, (secs % 3600) / 60)
}

/// Run one line.
pub fn run(line: &str) -> Reply {
    if line.chars().count() > MAX_LINE || line.chars().any(|c| c.is_control() && c != '\t') {
        return Reply::err(None, msg("err.line", &[]));
    }
    let toks = tokenize(line);
    let Some(first) = toks.first() else {
        return Reply::err(None, msg("err.empty", &[]));
    };
    let Some(cmd) = find(first) else {
        return Reply::err(None, msg("err.unknown", &[("cmd", first.chars().take(40).collect())]));
    };
    let args = &toks[1..];
    let usage = || Reply::err(Some(cmd.name), msg("err.usage", &[("usage", cmd.usage.to_string())]));
    match cmd.name {
        "ajuda" => Reply::effect(cmd.name, Effect::Help),
        "limpar" => Reply::effect(cmd.name, Effect::Clear),
        "winget" => winget_cmd(cmd.name, args),
        "abrir" => {
            let Some(w) = args.first().map(|s| s.to_lowercase()) else {
                return usage();
            };
            match SCREENS.iter().find(|(words, _)| words.contains(&w.as_str())) {
                Some((_, route)) => Reply::effect(cmd.name, Effect::Navigate { route }),
                None => Reply::err(Some(cmd.name), msg("err.screen", &[("screen", w.chars().take(40).collect())])),
            }
        }
        "ping" => {
            let (Some(host), Ok(n)) = (args.first(), int_arg(args.get(1), 4, 1, 20)) else {
                return usage();
            };
            ping(cmd.name, host, n as usize)
        }
        "rota" => match args.first() {
            Some(host) => route(cmd.name, host),
            None => usage(),
        },
        "tcping" => match args.first() {
            Some(target) => tcping(cmd.name, target),
            None => usage(),
        },
        "dns" => match args.first() {
            Some(name) => resolve(cmd.name, name),
            None => usage(),
        },
        "rede" => adapters(cmd.name),
        "dispositivos" => lan(cmd.name),
        "processos" => match int_arg(args.first(), 15, 1, 50) {
            Ok(n) => processes(cmd.name, n as usize),
            Err(()) => usage(),
        },
        "disco" => drives(cmd.name),
        "pasta" => {
            let (Some(path), Ok(secs)) = (args.first(), int_arg(args.get(1), 20, 5, 110)) else {
                return usage();
            };
            folder(cmd.name, path, secs)
        }
        "energia" => plans(cmd.name),
        "apps" => installed(cmd.name, &args.join(" ")),
        "erros" => match int_arg(args.first(), 7, 1, 30) {
            Ok(d) => errors(cmd.name, d as u32),
            Err(()) => usage(),
        },
        "pc" => pc(cmd.name),
        "sensores" => sensors_cmd(cmd.name),
        "inicializacao" => startup_cmd(cmd.name),
        _ => usage(),
    }
}

fn resolve_host(command: &'static str, host: &str) -> Result<std::net::Ipv4Addr, Reply> {
    if !valid_host(host) {
        return Err(Reply::err(Some(command), msg("err.host", &[("host", host.chars().take(60).collect())])));
    }
    network::resolve_v4(host).map_err(|_| Reply::err(Some(command), msg("err.resolve", &[("host", host.to_string())])))
}

fn ping(command: &'static str, host: &str, n: usize) -> Reply {
    let ip = match resolve_host(command, host) {
        Ok(ip) => ip,
        Err(r) => return r,
    };
    let mut rows = Vec::new();
    let mut samples = Vec::new();
    for i in 0..n {
        if i > 0 {
            std::thread::sleep(Duration::from_millis(250));
        }
        let (from, rtt) = match network::ping(ip, 1000) {
            Ok(network::Probe::Reply { from, rtt_ms }) => (from.to_string(), Some(f64::from(rtt_ms))),
            _ => ("*".into(), None),
        };
        samples.push(rtt);
        rows.push(vec![(i + 1).to_string(), from, rtt.map_or_else(|| "timeout".into(), ms)]);
    }
    let s = network::latency_stats(&samples);
    Reply::ok(
        command,
        vec![
            Block::Table { columns: vec!["seq", "from", "ms"], rows },
            Block::Note(msg(
                "note.latency",
                &[
                    ("ip", ip.to_string()),
                    ("avg", ms(s.avg_ms)),
                    ("min", ms(s.min_ms)),
                    ("max", ms(s.max_ms)),
                    ("jitter", ms(s.jitter_ms)),
                    ("loss", format!("{:.0}", s.loss_pct)),
                ],
            )),
        ],
    )
}

fn route(command: &'static str, host: &str) -> Reply {
    let ip = match resolve_host(command, host) {
        Ok(ip) => ip,
        Err(r) => return r,
    };
    match network::trace_route(ip, 30, 1, 1000) {
        Ok(hops) => {
            let rows = hops
                .iter()
                .map(|h| {
                    let rtt = h.rtts_ms.first().copied().flatten();
                    vec![h.ttl.to_string(), h.addr.map_or_else(|| "*".into(), |a| a.to_string()), rtt.map_or_else(|| "*".into(), |r| r.to_string())]
                })
                .collect();
            Reply::ok(command, vec![Block::Table { columns: vec!["hop", "address", "ms"], rows }])
        }
        Err(_) => Reply::err(Some(command), msg("err.network", &[])),
    }
}

fn tcping(command: &'static str, target: &str) -> Reply {
    let (host, port) = match target.rsplit_once(':') {
        Some((h, p)) if !h.contains(':') => match p.parse::<u16>() {
            Ok(p) if p > 0 => (h, p),
            _ => return Reply::err(Some(command), msg("err.port", &[])),
        },
        _ => (target, 443),
    };
    let ip = match resolve_host(command, host) {
        Ok(ip) => ip,
        Err(r) => return r,
    };
    let mut rows = Vec::new();
    let mut samples = Vec::new();
    for i in 0..4 {
        let rtt = network::tcping(ip, port, 1500);
        samples.push(rtt);
        rows.push(vec![(i + 1).to_string(), format!("{ip}:{port}"), rtt.map_or_else(|| "timeout".into(), ms)]);
    }
    let s = network::latency_stats(&samples);
    Reply::ok(
        command,
        vec![
            Block::Table { columns: vec!["seq", "to", "ms"], rows },
            Block::Note(msg(
                "note.latency",
                &[
                    ("ip", format!("{ip}:{port}")),
                    ("avg", ms(s.avg_ms)),
                    ("min", ms(s.min_ms)),
                    ("max", ms(s.max_ms)),
                    ("jitter", ms(s.jitter_ms)),
                    ("loss", format!("{:.0}", s.loss_pct)),
                ],
            )),
        ],
    )
}

fn resolve(command: &'static str, name: &str) -> Reply {
    use std::net::ToSocketAddrs;
    if !valid_host(name) || name.contains(':') {
        return Reply::err(Some(command), msg("err.host", &[("host", name.chars().take(60).collect())]));
    }
    let t0 = std::time::Instant::now();
    match (name, 0u16).to_socket_addrs() {
        Ok(addrs) => {
            let mut seen = Vec::new();
            for a in addrs {
                if !seen.contains(&a.ip()) {
                    seen.push(a.ip());
                }
            }
            let rows = seen.iter().map(|ip| vec![ip.to_string(), if ip.is_ipv4() { "IPv4" } else { "IPv6" }.into()]).collect();
            Reply::ok(
                command,
                vec![
                    Block::Table { columns: vec!["address", "family"], rows },
                    Block::Note(msg("note.resolved", &[("ms", ms(t0.elapsed().as_secs_f64() * 1000.0))])),
                ],
            )
        }
        Err(_) => Reply::err(Some(command), msg("err.resolve", &[("host", name.to_string())])),
    }
}

fn adapters(command: &'static str) -> Reply {
    let rows = dns::adapters()
        .iter()
        .map(|a| {
            vec![
                a.name.clone(),
                yes(a.primary),
                a.gateway.map(|g| g.to_string()).unwrap_or_default(),
                a.dns.iter().map(|d| d.to_string()).collect::<Vec<_>>().join(", "),
                a.provider.unwrap_or("").to_string(),
                if a.link_mbps > 0 { a.link_mbps.to_string() } else { String::new() },
            ]
        })
        .collect();
    Reply::ok(command, vec![Block::Table { columns: vec!["adapter", "primary", "gateway", "dns", "provider", "mbps"], rows }])
}

/// Install/upgrade confirmations: single-use, 2 minutes, bound to the action and the exact id.
fn tokens() -> &'static TokenStore {
    static T: std::sync::OnceLock<TokenStore> = std::sync::OnceLock::new();
    T.get_or_init(TokenStore::default)
}

fn winget_error(command: &'static str, e: &winget::WingetError) -> Reply {
    let key = match e {
        winget::WingetError::Unavailable => "err.winget_missing",
        winget::WingetError::InvalidId => "err.winget_id",
        winget::WingetError::InvalidQuery => "err.filter",
        winget::WingetError::NotFound => "err.winget_not_found",
        winget::WingetError::Timeout => "err.winget_timeout",
        winget::WingetError::Io(_) => "err.network",
    };
    Reply::err(Some(command), msg(key, &[]))
}

fn winget_cmd(command: &'static str, args: &[String]) -> Reply {
    let usage = || Reply::err(Some(command), msg("err.usage", &[("usage", COMMANDS.iter().find(|c| c.name == command).map_or("", |c| c.usage).to_string())]));
    let sub = args.first().map(|s| s.to_lowercase()).unwrap_or_default();
    match sub.as_str() {
        "buscar" | "search" | "procurar" => {
            let q = args[1..].join(" ");
            match winget::search(&q) {
                Ok(list) => Reply::ok(
                    command,
                    vec![Block::Table {
                        columns: vec!["app", "id", "version"],
                        rows: list.into_iter().map(|p| vec![p.name, p.id, p.version]).collect(),
                    }],
                ),
                Err(e) => winget_error(command, &e),
            }
        }
        "atualizacoes" | "atualizações" | "upgrades" | "updates" => match winget::upgrades() {
            Ok(list) => {
                let n = list.len();
                Reply::ok(
                    command,
                    vec![
                        Block::Table {
                            columns: vec!["app", "id", "version", "available"],
                            rows: list.into_iter().map(|p| vec![p.name, p.id, p.version, p.available.unwrap_or_default()]).collect(),
                        },
                        Block::Note(msg("note.winget_upgrades", &[("n", n.to_string())])),
                    ],
                )
            }
            Err(e) => winget_error(command, &e),
        },
        "instalar" | "install" | "atualizar" | "upgrade" => {
            let Some(id) = args.get(1) else {
                return usage();
            };
            let action = if matches!(sub.as_str(), "instalar" | "install") { "install" } else { "upgrade" };
            match winget::exact(id) {
                Ok(p) => {
                    let token = tokens().issue(vec![format!("winget:{action}:{}", p.id)]).token;
                    Reply::ok(command, vec![Block::Confirm { action, id: p.id, name: p.name, version: p.version, token }])
                }
                Err(e) => winget_error(command, &e),
            }
        }
        _ => usage(),
    }
}

/// The user clicked "Install"/"Update" on a confirmation block: run it once.
pub fn confirm(token: u64, action: &str, id: &str) -> Reply {
    const C: &str = "winget";
    let action = match action {
        "install" => "install",
        "upgrade" => "upgrade",
        _ => return Reply::err(Some(C), msg("err.confirm", &[])),
    };
    if !winget::valid_id(id) || !tokens().consume(token, &[format!("winget:{action}:{id}")]) {
        return Reply::err(Some(C), msg("err.confirm", &[]));
    }
    match winget::install(id, action == "upgrade") {
        Ok(o) => {
            let key = if o.ok { "note.winget_done" } else { "note.winget_failed" };
            Reply {
                ok: o.ok,
                command: Some(C),
                blocks: vec![Block::Note(msg(key, &[("id", id.to_string()), ("code", format!("{:#x}", o.code)), ("message", o.message)]))],
                effect: None,
                error: None,
            }
        }
        Err(e) => winget_error(C, &e),
    }
}

fn lan(command: &'static str) -> Reply {
    match devices::scan() {
        Err(_) => Reply::err(Some(command), msg("err.network", &[])),
        Ok(s) => {
            let rows = s
                .devices
                .iter()
                .map(|d| {
                    vec![
                        serde_json::to_value(d.kind).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default(),
                        d.ip.clone(),
                        d.name.clone().or_else(|| d.hostname.clone()).unwrap_or_default(),
                        d.vendor.clone().unwrap_or_default(),
                        d.mac.clone(),
                        if d.new { "✓".into() } else { String::new() },
                    ]
                })
                .collect();
            Reply::ok(
                command,
                vec![
                    Block::Table { columns: vec!["device_kind", "ip", "name", "vendor", "mac", "new"], rows },
                    Block::Note(msg(
                        "note.devices",
                        &[("count", s.devices.len().to_string()), ("subnet", s.subnet.clone()), ("new", s.new_count.to_string())],
                    )),
                ],
            )
        }
    }
}

fn processes(command: &'static str, n: usize) -> Reply {
    let rows = procs::top(Duration::from_millis(1000), n)
        .iter()
        .map(|p| vec![p.name.clone(), p.count.to_string(), mb(p.ram_bytes), format!("{:.1}", p.cpu_pct), yes(p.system)])
        .collect();
    Reply::ok(command, vec![Block::Table { columns: vec!["process", "count", "ram_mb", "cpu_pct", "system"], rows }])
}

fn drives(command: &'static str) -> Reply {
    let rows = system::list_drives()
        .iter()
        .map(|d| {
            let used = if d.total_bytes > 0 { 100.0 * (d.total_bytes - d.free_bytes) as f64 / d.total_bytes as f64 } else { 0.0 };
            vec![d.letter.clone(), format!("{:?}", d.kind).to_lowercase(), gb(d.free_bytes), gb(d.total_bytes), format!("{used:.0}")]
        })
        .collect();
    Reply::ok(command, vec![Block::Table { columns: vec!["drive", "kind", "free_gb", "total_gb", "used_pct"], rows }])
}

fn folder(command: &'static str, path: &str, secs: u64) -> Reply {
    match disk::usage(Some(path), secs) {
        Err(_) => Reply::err(Some(command), msg("err.path", &[])),
        Ok(u) => {
            let rows = u
                .tree
                .as_ref()
                .map(|t| {
                    t.children
                        .iter()
                        .map(|c| vec![c.name.clone(), if c.is_dir { "dir" } else { "file" }.into(), gb(c.size), c.files.to_string()])
                        .collect()
                })
                .unwrap_or_default();
            let key = if u.summary.cancelled { "note.folder_partial" } else { "note.folder" };
            Reply::ok(
                command,
                vec![
                    Block::Table { columns: vec!["name", "type", "size_gb", "files"], rows },
                    Block::Note(msg(key, &[("size", gb(u.summary.bytes)), ("files", u.summary.files.to_string()), ("secs", secs.to_string())])),
                ],
            )
        }
    }
}

fn plans(command: &'static str) -> Reply {
    let p = power::plans();
    let rows = p.plans.iter().map(|x| vec![x.name.clone(), yes(x.active), yes(x.builtin)]).collect();
    Reply::ok(command, vec![Block::Table { columns: vec!["plan", "active", "builtin"], rows }])
}

fn installed(command: &'static str, filter: &str) -> Reply {
    let f = filter.trim().to_lowercase();
    if f.chars().count() > 64 {
        return Reply::err(Some(command), msg("err.filter", &[]));
    }
    let s = settings::load();
    let mut list = apps::list(&s.protected_apps);
    list.retain(|a| f.is_empty() || a.name.to_lowercase().contains(&f) || a.publisher.to_lowercase().contains(&f));
    list.sort_by_key(|a| std::cmp::Reverse(a.size_bytes.unwrap_or(0)));
    let total = list.len();
    let rows = list
        .iter()
        .take(60)
        .map(|a| vec![a.name.clone(), a.publisher.clone(), a.version.clone(), a.size_bytes.map(mb).unwrap_or_default(), a.install_date.clone().unwrap_or_default()])
        .collect();
    Reply::ok(
        command,
        vec![
            Block::Table { columns: vec!["app", "publisher", "version", "size_mb", "installed"], rows },
            Block::Note(msg("note.apps", &[("shown", total.min(60).to_string()), ("total", total.to_string())])),
        ],
    )
}

fn errors(command: &'static str, days: u32) -> Reply {
    let s = winevents::summary(days);
    let rows = s
        .groups
        .iter()
        .map(|g| {
            vec![
                g.count.to_string(),
                g.level.to_string(),
                g.source.clone(),
                g.event_id.to_string(),
                g.hint.unwrap_or("").to_string(),
                date(g.last_unix),
                g.message.clone().unwrap_or_default(),
            ]
        })
        .collect();
    Reply::ok(
        command,
        vec![
            Block::Table { columns: vec!["count", "level", "source", "id", "hint", "last", "message"], rows },
            Block::Note(msg("note.errors", &[("days", days.to_string()), ("scanned", s.scanned.to_string())])),
        ],
    )
}

/// Hardware summary. The `part` column holds ids the UI translates (`console.part.<id>`); details
/// are language-neutral data.
fn pc(command: &'static str) -> Reply {
    let p = hardware::profile();
    let mut rows = vec![
        vec!["os".to_string(), p.os.clone()],
        vec!["cpu".into(), format!("{} · {}C/{}T", p.cpu, p.cores_physical, p.cores_logical)],
        vec![
            "ram".into(),
            format!(
                "{} GB · {}{}",
                gb(p.ram_bytes),
                p.memory.len(),
                p.memory.first().map(|m| format!(" × {} · {} MT/s", m.kind, m.configured_mts)).unwrap_or_default()
            ),
        ],
    ];
    for g in &p.gpus {
        rows.push(vec!["gpu".into(), format!("{} · {} GB{}", g.name, gb(g.vram_bytes), g.driver.as_deref().map(|d| format!(" · {d}")).unwrap_or_default())]);
    }
    for m in &p.monitors {
        rows.push(vec!["monitor".into(), format!("{} · {}x{} · {:.0}/{} Hz", m.name, m.width, m.height, m.refresh_hz, m.max_refresh_hz)]);
    }
    for d in &p.disks {
        let kind = match d.ssd {
            Some(true) => "SSD",
            Some(false) => "HDD",
            None => "?",
        };
        let part = if d.system { "disk_system" } else { "disk" };
        rows.push(vec![part.into(), format!("{} · {} · {} · {} GB", d.model, kind, d.bus, gb(d.size_bytes))]);
    }
    if let Some(b) = &p.board {
        rows.push(vec!["board".into(), b.clone()]);
    }
    Reply::ok(command, vec![Block::Table { columns: vec!["part", "detail"], rows }])
}

fn sensors_cmd(command: &'static str) -> Reply {
    match sensors::read() {
        None => Reply::ok(command, vec![Block::Note(msg("note.no_sensors", &[]))]),
        Some(x) => {
            let rows = x
                .readings
                .iter()
                .take(80)
                .map(|r| vec![r.sensor.clone(), r.label.clone(), format!("{:.1} {}", r.value, r.unit)])
                .collect();
            Reply::ok(command, vec![Block::Table { columns: vec!["sensor", "reading", "value"], rows }])
        }
    }
}

fn startup_cmd(command: &'static str) -> Reply {
    let rows = startup::list()
        .iter()
        .map(|i| vec![i.name.clone(), yes(i.enabled), format!("{:?}", i.location), yes(i.protected)])
        .collect();
    Reply::ok(command, vec![Block::Table { columns: vec!["name", "enabled", "location", "protected"], rows }])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizer_and_lookup() {
        assert_eq!(tokenize(r#"pasta "C:\Program Files" 30"#), vec!["pasta", r"C:\Program Files", "30"]);
        assert_eq!(tokenize("  ping   1.1.1.1  "), vec!["ping", "1.1.1.1"]);
        assert_eq!(find("TRACERT").map(|c| c.name), Some("rota"));
        assert_eq!(find("help").map(|c| c.name), Some("ajuda"));
        assert!(find("cmd").is_none() && find("powershell").is_none());
        // Names are unique across names and aliases.
        let mut all: Vec<&str> = COMMANDS.iter().flat_map(|c| std::iter::once(c.name).chain(c.aliases.iter().copied())).collect();
        all.sort_unstable();
        let n = all.len();
        all.dedup();
        assert_eq!(all.len(), n);
    }

    #[test]
    fn hostile_lines_are_refused() {
        assert_eq!(run("").error.map(|m| m.key), Some("err.empty"));
        assert_eq!(run(&"x".repeat(600)).error.map(|m| m.key), Some("err.line"));
        assert_eq!(run("ping a\u{0}b").error.map(|m| m.key), Some("err.line"));
        assert_eq!(run("del /s /q C:\\").error.map(|m| m.key), Some("err.unknown"));
        assert_eq!(run("ping").error.map(|m| m.key), Some("err.usage"));
        assert_eq!(run("ping a;calc").error.map(|m| m.key), Some("err.host"));
        assert_eq!(run("ping 1.1.1.1 muitas").error.map(|m| m.key), Some("err.usage"));
        assert_eq!(run("tcping 1.1.1.1:99999").error.map(|m| m.key), Some("err.port"));
        assert_eq!(run("abrir regedit").error.map(|m| m.key), Some("err.screen"));
        assert_eq!(run("abrir Limpeza").effect, Some(Effect::Navigate { route: "cleanup" }));
        assert_eq!(run("cls").effect, Some(Effect::Clear));
        assert_eq!(run("winget").error.map(|m| m.key), Some("err.usage"));
        assert_eq!(run("winget instalar a;b").error.map(|m| m.key), Some("err.winget_id"));
        assert_eq!(run("winget buscar --force").error.map(|m| m.key), Some("err.filter"));
        // A token is bound to the action and the id, used once.
        let t = tokens().issue(vec!["winget:install:Valve.Steam".into()]).token;
        assert_eq!(confirm(t, "upgrade", "Valve.Steam").error.map(|m| m.key), Some("err.confirm"), "other action");
        assert_eq!(confirm(t, "install", "a;b").error.map(|m| m.key), Some("err.confirm"), "bad id");
        assert_eq!(confirm(t + 2, "install", "Valve.Steam").error.map(|m| m.key), Some("err.confirm"), "unknown token");
        assert!(!valid_host("a b") && !valid_host("x/y") && valid_host("speed.cloudflare.com"));
    }

    #[test]
    fn local_read_only_commands_answer() {
        for line in ["disco", "energia", "inicializacao", "rede", "erros 1", "pc"] {
            let r = run(line);
            assert!(r.ok, "{line}: {:?}", r.error);
            assert!(!r.blocks.is_empty(), "{line}");
        }
        let r = run(&format!(r#"pasta "{}" 5"#, std::env::temp_dir().display()));
        assert!(r.ok, "{:?}", r.error);
        assert_eq!(date(0), "1970-01-01 00:00");
        assert_eq!(date(1_760_745_600), "2025-10-18 00:00");
    }
}
