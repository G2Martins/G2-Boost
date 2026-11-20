//! Map of the MCP servers configured on this PC, read-only: which AI apps (Claude Desktop, Claude
//! Code, VS Code, Cursor, Windsurf, LM Studio) start which servers. G2 Boost never starts, stops or
//! edits them; it only shows what is there, because an MCP server is a program another app runs at
//! every start (a persistence and supply-chain vector worth seeing).
//!
//! Those configs belong to other apps and are hostile input: symlinks refused, > 4 MB refused,
//! invalid JSON reported (JSON with comments, as VS Code writes, is accepted). What leaves this
//! module is deliberately thin: server name, app, transport, the executable's FILE NAME or the URL
//! HOST. Never arguments, environment, headers, query strings or user info: they often carry API
//! keys. Claude Code project servers come as name + how many projects, never the project paths.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Map, Value, json};

const MAX_BYTES: u64 = 4 << 20;
const MAX_SERVERS: usize = 100;

pub const CLIENTS: &[&str] = &["claude_desktop", "claude_code", "vscode", "cursor", "windsurf", "lmstudio"];

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Server {
    pub name: String,
    /// "user" (the app's own config) or "project" (Claude Code: declared by one or more projects).
    pub scope: &'static str,
    /// "stdio" | "http" | "unknown"; None for project rows (only name + count are read there).
    pub transport: Option<&'static str>,
    /// File name of the executable (`npx`, `uvx`, `node.exe`), lowercase, never a path.
    pub command: Option<String>,
    /// Host (and port) of an HTTP server's URL, never its path, query or user info.
    pub host: Option<String>,
    /// Fetches a package from the internet at every start (npx, uvx, bunx, pipx run, dlx…).
    pub downloads: bool,
    /// The executable is not on disk / not on PATH (the app would fail to start it).
    pub missing: bool,
    /// It is G2 Boost's own server.
    pub g2boost: bool,
    /// Project rows: how many projects declare it.
    pub projects: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClientMap {
    pub client: &'static str,
    /// At least one config file of this app exists.
    pub config: bool,
    /// A config file was refused: "symlink" | "too_large" | "invalid_json" | "unreadable".
    pub error: Option<&'static str>,
    /// The app's exe when found (for its icon in the UI only; never sent to an AI).
    pub exe: Option<String>,
    pub servers: Vec<Server>,
}

/// Where to look (tests point these at a sandbox).
pub struct Env {
    pub appdata: Option<PathBuf>,
    pub local: Option<PathBuf>,
    pub home: Option<PathBuf>,
    pub program_files: Option<PathBuf>,
    pub path: Option<OsString>,
}

impl Env {
    pub fn from_os() -> Self {
        let var = |k: &str| std::env::var_os(k).map(PathBuf::from);
        Env { appdata: var("APPDATA"), local: var("LOCALAPPDATA"), home: var("USERPROFILE"), program_files: var("ProgramFiles"), path: std::env::var_os("PATH") }
    }
}

/* ---------------- reading (hostile files) ---------------- */

/// Comments (`//`, `/* */`) and trailing commas out, strings untouched: JSONC → JSON.
fn strip_jsonc(s: &str) -> String {
    let b = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let (mut i, mut in_str) = (0, false);
    while i < b.len() {
        let c = b[i];
        if in_str {
            out.push(c);
            if c == b'\\' && i + 1 < b.len() {
                out.push(b[i + 1]);
                i += 1;
            } else if c == b'"' {
                in_str = false;
            }
        } else if c == b'"' {
            in_str = true;
            out.push(c);
        } else if c == b'/' && b.get(i + 1) == Some(&b'/') {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        } else if c == b'/' && b.get(i + 1) == Some(&b'*') {
            i += 2;
            while i < b.len() && !(b[i] == b'*' && b.get(i + 1) == Some(&b'/')) {
                i += 1;
            }
            i += 2;
            continue;
        } else if c == b',' {
            // Trailing comma: the next meaningful byte closes the object/array.
            let next = b[i + 1..].iter().find(|x| !x.is_ascii_whitespace());
            if !matches!(next, Some(b'}') | Some(b']')) {
                out.push(c);
            }
        } else {
            out.push(c);
        }
        i += 1;
    }
    // Only ASCII bytes were dropped, so the rest is still the input's valid UTF-8.
    String::from_utf8(out).unwrap_or_default()
}

/// The file as JSON. `Ok(None)` = not there.
fn read_json(path: &Path) -> Result<Option<Value>, &'static str> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("unreadable"),
    };
    if meta.file_type().is_symlink() {
        return Err("symlink");
    }
    if !meta.is_file() {
        return Err("unreadable");
    }
    if meta.len() > MAX_BYTES {
        return Err("too_large");
    }
    let text = std::fs::read_to_string(path).map_err(|_| "unreadable")?;
    let text = text.trim_start_matches('\u{feff}');
    if text.trim().is_empty() {
        return Ok(Some(Value::Object(Map::new())));
    }
    serde_json::from_str(text).or_else(|_| serde_json::from_str(&strip_jsonc(text))).map(Some).map_err(|_| "invalid_json")
}

/* ---------------- one server ---------------- */

/// Printable, one line, ≤ 64 chars.
fn clean_name(s: &str) -> Option<String> {
    let t: String = s.chars().filter(|c| !c.is_control()).take(64).collect();
    let t = t.trim().to_string();
    (!t.is_empty()).then_some(t)
}

/// `https://user:pw@api.example.com:8443/mcp?key=x` → `api.example.com:8443`.
pub(crate) fn host_of(url: &str) -> Option<String> {
    let (_, rest) = url.trim().split_once("://")?;
    // `\` ends the authority too, as browsers read http(s) URLs.
    let authority = rest.split(['/', '\\', '?', '#']).next()?;
    let host = authority.rsplit('@').next()?.to_ascii_lowercase();
    let ok = !host.is_empty() && host.len() <= 255 && host.chars().all(|c| c.is_ascii_alphanumeric() || ".-:[]".contains(c));
    ok.then_some(host)
}

/// The command cannot be found. Variables, relative paths and network paths are not checked (no
/// way to know, and a UNC path would touch the network): those count as found.
fn missing(command: &str, env: &Env) -> bool {
    let c = command.trim().trim_matches('"');
    if c.is_empty() || c.contains(['$', '%']) || c.starts_with("\\\\") || c.starts_with("//") {
        return false;
    }
    let p = Path::new(c);
    if p.is_absolute() {
        return !p.is_file();
    }
    if c.contains(['\\', '/']) {
        return false;
    }
    let name = c.split_whitespace().next().unwrap_or(c);
    let exts: &[&str] = if Path::new(name).extension().is_some() { &[""] } else { &["", ".exe", ".cmd", ".bat", ".com"] };
    let Some(path) = &env.path else { return false };
    !std::env::split_paths(path)
        .filter(|d| !d.as_os_str().is_empty() && !d.to_string_lossy().starts_with("\\\\"))
        .any(|d| exts.iter().any(|e| d.join(format!("{name}{e}")).is_file()))
}

/// Launchers that fetch a package from the internet each time the server starts.
fn downloads(stem: &str, a0: &str, a1: &str) -> bool {
    matches!(stem, "npx" | "uvx" | "bunx" | "pnpx")
        || (matches!(stem, "pnpm" | "yarn") && a0 == "dlx")
        || (stem == "pipx" && a0 == "run")
        || (stem == "bun" && a0 == "x")
        || (stem == "npm" && matches!(a0, "exec" | "x"))
        || (stem == "uv" && a0 == "tool" && a1 == "run")
}

fn server(name: &str, v: &Value, env: &Env) -> Option<Server> {
    let name = clean_name(name)?;
    let o = v.as_object()?;
    let s = |k: &str| o.get(k).and_then(Value::as_str);
    let url = s("url").or_else(|| s("serverUrl")).or_else(|| s("httpUrl"));
    let kind = s("type").or_else(|| s("transport")).unwrap_or("").to_ascii_lowercase();
    let command = s("command");
    let http = url.is_some() || matches!(kind.as_str(), "http" | "sse" | "streamable-http" | "streamablehttp");
    let base = command.map(crate::context::exe_of).filter(|b| !b.is_empty());
    let stem = base.as_deref().map(|b| b.trim_end_matches(".exe").trim_end_matches(".cmd").trim_end_matches(".bat").to_string()).unwrap_or_default();
    // Only the first two arguments are looked at, to recognize `pnpm dlx` and friends; never kept.
    let arg = |i: usize| o.get("args").and_then(|a| a.get(i)).and_then(Value::as_str).unwrap_or("").to_ascii_lowercase();
    let g2boost = name.eq_ignore_ascii_case("g2boost") || matches!(base.as_deref(), Some("g2 boost.exe" | "g2-boost.exe" | "g2boost-mcp.exe"));
    Some(Server {
        transport: Some(if http { "http" } else if command.is_some() { "stdio" } else { "unknown" }),
        host: if http { url.and_then(host_of) } else { None },
        downloads: !http && downloads(&stem, &arg(0), &arg(1)),
        missing: !http && command.is_some_and(|c| missing(c, env)),
        command: if http { None } else { base },
        g2boost,
        name,
        scope: "user",
        projects: 0,
    })
}

/* ---------------- per app ---------------- */

impl ClientMap {
    fn new(client: &'static str, exe: Option<PathBuf>) -> Self {
        ClientMap { client, config: false, error: None, exe: exe.map(|p| p.to_string_lossy().into_owned()), servers: Vec::new() }
    }

    /// Read one config; `pick` finds its servers object in the root.
    fn file(&mut self, path: &Path, pick: fn(&Value) -> Option<&Map<String, Value>>, env: &Env) -> Option<Value> {
        match read_json(path) {
            Ok(None) => None,
            Err(e) => {
                self.config = true;
                self.error.get_or_insert(e);
                None
            }
            Ok(Some(root)) => {
                self.config = true;
                if let Some(servers) = pick(&root) {
                    for (k, v) in servers {
                        self.push(server(k, v, env));
                    }
                }
                Some(root)
            }
        }
    }

    fn push(&mut self, s: Option<Server>) {
        if let Some(s) = s
            && self.servers.len() < MAX_SERVERS
            && !self.servers.iter().any(|x| x.name == s.name && x.scope == s.scope)
        {
            self.servers.push(s);
        }
    }
}

fn key<'a>(root: &'a Value, k: &str) -> Option<&'a Map<String, Value>> {
    root.get(k)?.as_object()
}

fn mcp_servers(root: &Value) -> Option<&Map<String, Value>> {
    key(root, "mcpServers")
}

fn vscode_servers(root: &Value) -> Option<&Map<String, Value>> {
    key(root, "servers")
}

fn first_file(candidates: impl IntoIterator<Item = Option<PathBuf>>) -> Option<PathBuf> {
    candidates.into_iter().flatten().find(|p| p.is_file())
}

pub fn scan() -> Vec<ClientMap> {
    scan_in(&Env::from_os())
}

pub fn scan_in(env: &Env) -> Vec<ClientMap> {
    let home = |rel: &str| env.home.as_ref().map(|h| h.join(rel));
    let local = |rel: &str| env.local.as_ref().map(|l| l.join(rel));
    let pf = |rel: &str| env.program_files.as_ref().map(|p| p.join(rel));
    let appdata = |rel: &str| env.appdata.as_ref().map(|a| a.join(rel));
    let mut out = Vec::new();

    let mut a = ClientMap::new("claude_desktop", first_file([local(r"AnthropicClaude\claude.exe")]));
    for p in crate::claude_connect::desktop_paths_in(env.appdata.clone(), env.local.clone()) {
        a.file(&p, mcp_servers, env);
    }
    out.push(a);

    let mut a = ClientMap::new("claude_code", first_file([home(r".local\bin\claude.exe")]));
    if let Some(root) = home(".claude.json").and_then(|p| a.file(&p, mcp_servers, env)) {
        // Project servers: names and how many projects declare each; the project paths (folder
        // names, user names) never leave this loop.
        let mut counts: Vec<(String, u32)> = Vec::new();
        for proj in key(&root, "projects").into_iter().flat_map(|m| m.values()) {
            for name in key(proj, "mcpServers").into_iter().flat_map(|m| m.keys()).filter_map(|n| clean_name(n)) {
                match counts.iter_mut().find(|(n, _)| *n == name) {
                    Some((_, c)) => *c += 1,
                    None => counts.push((name, 1)),
                }
            }
        }
        for (name, projects) in counts {
            a.push(Some(Server { name, scope: "project", transport: None, command: None, host: None, downloads: false, missing: false, g2boost: false, projects }));
        }
    }
    out.push(a);

    let mut a = ClientMap::new("vscode", first_file([local(r"Programs\Microsoft VS Code\Code.exe"), pf(r"Microsoft VS Code\Code.exe")]));
    if let Some(p) = appdata(r"Code\User\mcp.json") {
        a.file(&p, vscode_servers, env);
    }
    // Older VS Code kept servers in settings.json (`mcp.servers`). Every install has that file,
    // so it only counts when it declares servers, and its errors are not about MCP.
    if let Some(Ok(Some(root))) = appdata(r"Code\User\settings.json").map(|p| read_json(&p))
        && let Some(m) = root.get("mcp").and_then(vscode_servers)
    {
        a.config = true;
        for (k, v) in m {
            a.push(server(k, v, env));
        }
    }
    out.push(a);

    for (client, cfg, exe) in [
        ("cursor", r".cursor\mcp.json", local(r"Programs\cursor\Cursor.exe")),
        ("windsurf", r".codeium\windsurf\mcp_config.json", local(r"Programs\Windsurf\Windsurf.exe")),
        ("lmstudio", r".lmstudio\mcp.json", first_file([local(r"Programs\LM Studio\LM Studio.exe"), local(r"Programs\lm-studio\LM Studio.exe")])),
    ] {
        let mut a = ClientMap::new(client, first_file([exe]));
        if let Some(p) = home(cfg) {
            a.file(&p, mcp_servers, env);
        }
        out.push(a);
    }
    out
}

/* ---------------- what an AI sees ---------------- */

/// For the MCP tool: everything but the apps' exe paths.
pub fn for_ai(maps: &[ClientMap]) -> Value {
    Value::Array(
        maps.iter()
            .filter(|m| m.config || !m.servers.is_empty())
            .map(|m| json!({ "client": m.client, "error": m.error, "servers": m.servers }))
            .collect(),
    )
}

/// For Luli's context pack: name, app and flags only (≤ 40).
pub fn for_context(maps: &[ClientMap]) -> Value {
    let rows: Vec<Value> = maps
        .iter()
        .flat_map(|m| m.servers.iter().map(move |s| (m.client, s)))
        .take(40)
        .map(|(client, s)| {
            let flags: Vec<&str> = [(s.downloads, "downloads_at_start"), (s.missing, "exe_not_found"), (s.g2boost, "g2boost"), (s.scope == "project", "project")]
                .into_iter()
                .filter_map(|(on, f)| on.then_some(f))
                .collect();
            json!({ "name": s.name, "client": client, "flags": flags })
        })
        .collect();
    Value::Array(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox(tag: &str) -> (PathBuf, Env) {
        let d = std::env::temp_dir().join(format!("g2boost-mcpmap-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        for sub in [r"Roaming\Claude", r"Roaming\Code\User", "Local", r"home\.cursor", r"home\.codeium\windsurf", r"home\.lmstudio", "bin"] {
            std::fs::create_dir_all(d.join(sub)).unwrap();
        }
        std::fs::write(d.join(r"bin\uvx.cmd"), "").unwrap();
        let env = Env {
            appdata: Some(d.join("Roaming")),
            local: Some(d.join("Local")),
            home: Some(d.join("home")),
            program_files: None,
            path: Some(d.join("bin").into_os_string()),
        };
        (d, env)
    }

    fn find<'a>(maps: &'a [ClientMap], client: &str) -> &'a ClientMap {
        maps.iter().find(|m| m.client == client).unwrap()
    }

    #[test]
    fn maps_every_app_without_leaking_secrets() {
        let (d, env) = sandbox("map");
        let gone = d.join(r"nowhere\server.exe");
        std::fs::write(
            d.join(r"Roaming\Claude\claude_desktop_config.json"),
            json!({ "mcpServers": {
                "files": { "command": "npx", "args": ["-y", "@scope/pkg", "--token=SECRET1"], "env": { "API_KEY": "SECRET2" } },
                "remote": { "type": "http", "url": "https://user:SECRET3@api.example.com:8443/mcp?key=SECRET4", "headers": { "Authorization": "SECRET5" } },
                "gone": { "command": gone.to_string_lossy() },
                "py": { "command": "uvx", "args": ["thing"] },
                "nope": { "command": "definitely-not-a-real-tool-xyz" },
                "dlx": { "command": "pnpm", "args": ["dlx", "x"] },
                "g2boost": { "command": r"C:\Program Files\G2 Boost\G2 Boost.exe", "args": ["--mcp"] },
                "bad\u{7}name\u{1b}": { "command": "node" },
                "notobject": 5
            } })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            d.join(r"home\.claude.json"),
            json!({
                "mcpServers": { "mine": { "type": "stdio", "command": "node", "args": ["C:\\Users\\SECRETUSER\\x.js"] } },
                "projects": {
                    "C:\\Users\\SECRETUSER\\secret-proj": { "mcpServers": { "db": { "command": "x" }, "web": { "url": "https://h" } } },
                    "D:\\work\\other": { "mcpServers": { "db": { "command": "y" } } },
                    "E:\\none": {}
                }
            })
            .to_string(),
        )
        .unwrap();
        // VS Code writes JSON with comments and trailing commas.
        std::fs::write(
            d.join(r"Roaming\Code\User\mcp.json"),
            "// my servers\n{\n  \"servers\": {\n    /* remote */ \"gh\": { \"type\": \"http\", \"url\": \"https://api.githubcopilot.com/mcp/\" },\n  },\n  \"inputs\": [],\n}\n",
        )
        .unwrap();
        std::fs::write(d.join(r"Roaming\Code\User\settings.json"), "{ \"editor.fontSize\": 14, // c\n \"mcp\": { \"servers\": { \"legacy\": { \"command\": \"uvx\" } } } }").unwrap();
        std::fs::write(d.join(r"home\.cursor\mcp.json"), "{ not json").unwrap();
        std::fs::write(d.join(r"home\.codeium\windsurf\mcp_config.json"), json!({ "mcpServers": { "ws": { "serverUrl": "http://localhost:3000/sse" } } }).to_string()).unwrap();
        std::fs::write(d.join(r"home\.lmstudio\mcp.json"), vec![b' '; (MAX_BYTES + 1) as usize]).unwrap();

        let maps = scan_in(&env);
        assert_eq!(maps.iter().map(|m| m.client).collect::<Vec<_>>(), CLIENTS);

        let cd = find(&maps, "claude_desktop");
        let by = |n: &str| cd.servers.iter().find(|s| s.name == n).unwrap();
        assert_eq!((by("files").command.as_deref(), by("files").downloads, by("files").missing), (Some("npx"), true, true));
        assert_eq!((by("remote").transport, by("remote").host.as_deref(), by("remote").command.as_deref()), (Some("http"), Some("api.example.com:8443"), None));
        assert!(by("gone").missing && by("gone").command.as_deref() == Some("server.exe"));
        assert!(!by("py").missing && by("py").downloads, "uvx found on PATH through .cmd");
        assert!(by("nope").missing);
        assert!(by("dlx").downloads);
        assert!(by("g2boost").g2boost && !by("files").g2boost);
        assert!(cd.servers.iter().any(|s| s.name == "badname"), "control characters stripped");
        assert_eq!(cd.servers.len(), 8, "the non-object entry is skipped");

        let cc = find(&maps, "claude_code");
        assert_eq!(cc.servers.iter().filter(|s| s.scope == "user").count(), 1);
        let db = cc.servers.iter().find(|s| s.name == "db").unwrap();
        assert_eq!((db.scope, db.projects, db.transport, db.command.as_ref()), ("project", 2, None, None));
        assert_eq!(cc.servers.iter().find(|s| s.name == "web").unwrap().projects, 1);

        let vs = find(&maps, "vscode");
        assert_eq!(vs.error, None);
        assert!(vs.servers.iter().any(|s| s.name == "gh" && s.host.as_deref() == Some("api.githubcopilot.com")));
        assert!(vs.servers.iter().any(|s| s.name == "legacy"), "settings.json mcp.servers");

        assert_eq!(find(&maps, "cursor").error, Some("invalid_json"));
        assert!(find(&maps, "cursor").config);
        assert_eq!(find(&maps, "windsurf").servers[0].host.as_deref(), Some("localhost:3000"));
        assert_eq!(find(&maps, "lmstudio").error, Some("too_large"));

        for out in [for_ai(&maps).to_string(), for_context(&maps).to_string(), serde_json::to_string(&maps).unwrap()] {
            for secret in ["SECRET", "user:", "secret-proj", "--token", "-y", "@scope", "x.js", "D:\\\\work"] {
                assert!(!out.contains(secret), "{secret} leaked: {out}");
            }
        }
        assert!(!for_ai(&maps).to_string().contains("nowhere"), "no paths for an AI");
        let ctx = for_context(&maps);
        assert!(ctx.as_array().unwrap().iter().any(|r| r["name"] == "files" && r["flags"][0] == "downloads_at_start"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn symlinked_config_is_refused() {
        let (d, env) = sandbox("link");
        let real = d.join("real.json");
        std::fs::write(&real, json!({ "mcpServers": { "x": { "command": "node" } } }).to_string()).unwrap();
        // Creating a symlink needs Developer Mode or admin; without it there is nothing to test.
        if std::os::windows::fs::symlink_file(&real, d.join(r"home\.cursor\mcp.json")).is_ok() {
            let maps = scan_in(&env);
            assert_eq!(find(&maps, "cursor").error, Some("symlink"));
            assert!(find(&maps, "cursor").servers.is_empty());
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn helpers() {
        assert_eq!(host_of("https://a.b.com/x?y=1#z").as_deref(), Some("a.b.com"));
        assert_eq!(host_of("http://u:p@[::1]:80/").as_deref(), Some("[::1]:80"));
        assert_eq!(host_of("no-scheme.com/path"), None);
        assert_eq!(host_of("https://evil.com\\@x/").as_deref(), Some("evil.com"), "backslash ends the host");
        assert_eq!(host_of("https://bad host/"), None, "odd characters refused");
        assert_eq!(strip_jsonc("{\"a\": \"// not a comment\", /* c */ \"b\": [1,2,],}"), "{\"a\": \"// not a comment\",  \"b\": [1,2]}");
        assert_eq!(strip_jsonc("{\"s\": \"q\\\"//x\"}"), "{\"s\": \"q\\\"//x\"}");
        let env = Env { appdata: None, local: None, home: None, program_files: None, path: None };
        assert!(!missing("${workspaceFolder}/run.exe", &env) && !missing(r"\\server\share\x.exe", &env) && !missing("./rel/x", &env));
    }
}
