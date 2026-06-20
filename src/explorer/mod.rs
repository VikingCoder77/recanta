//! Recanta Explorer (PRD §15): a local, read-only web UI to see and search memory and
//! the code graph. Served by `recanta serve` from the single binary — no Node/Nuxt build
//! and no external CDN, so it preserves the zero-runtime, local-first guarantee. Binds
//! `127.0.0.1` only, no auth, no telemetry, no write endpoints.

mod api;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::Result;
use rusqlite::Connection;
use tiny_http::{Header, Response, Server};

use crate::project::{Config, Paths};
use crate::{db, repo};

/// The single-file UI, embedded in the binary.
const UI_HTML: &str = include_str!("ui.html");
/// Vendored Cytoscape.js (embedded so the graph works fully offline — no CDN).
const CYTOSCAPE_JS: &str = include_str!("vendor/cytoscape.min.js");

/// One opened project store the Explorer reads from. In workspace mode several are open at
/// once; `key` namespaces this project's node ids so they never collide across stores.
pub struct ProjectStore {
    pub key: String,
    pub name: String,
    pub conn: Connection,
    pub cfg: Config,
    pub root: PathBuf,
    pub repo_id: Option<i64>,
}

impl ProjectStore {
    /// Open the store at `root` (must be an initialized project). `key` is the stable
    /// per-run namespace (e.g. `p0`).
    pub fn open(root: &Path, key: String) -> Result<ProjectStore> {
        let paths = Paths::for_root(root);
        let conn = db::open_existing(&paths.db)?;
        let cfg = Config::load(&paths.config)?;
        let project_id = repo::current_project_id(&conn).ok();
        let repo_id = project_id
            .as_deref()
            .and_then(|pid| repo::ensure_repository(&conn, pid, &paths.root, None).ok());
        Ok(ProjectStore { key, name: cfg.name.clone(), conn, cfg, root: paths.root, repo_id })
    }
}

/// Start the Explorer server over one or more project stores (blocks until interrupted).
/// `version` is bumped by the optional document watcher (`serve --watch`); the UI polls
/// `/api/version` and refreshes when it changes. Pass a fresh `AtomicU64(0)` when not
/// watching — it simply never changes.
pub fn run(
    stores: Vec<ProjectStore>,
    host: &str,
    port: u16,
    open: bool,
    version: Arc<AtomicU64>,
) -> Result<()> {
    let addr = format!("{host}:{port}");
    let server = Server::http(&addr).map_err(|e| anyhow::anyhow!("binding {addr}: {e}"))?;
    let url = format!("http://{addr}");
    if stores.len() > 1 {
        println!(
            "Recanta Explorer → {url}  ({} projects; read-only; Ctrl-C to stop)",
            stores.len()
        );
    } else {
        println!("Recanta Explorer → {url}  (read-only; Ctrl-C to stop)");
    }
    if open {
        open_browser(&url);
    }

    for request in server.incoming_requests() {
        let (body, ctype, code) = route(&stores, &version, request.url());
        let header = Header::from_bytes(b"Content-Type".as_slice(), ctype.as_bytes()).unwrap();
        let response = Response::from_string(body).with_status_code(code).with_header(header);
        let _ = request.respond(response);
    }
    Ok(())
}

/// Pick the store a per-project request targets: the `project=<key>` query param, or the
/// first store when none is given (single-project mode).
fn resolve<'a>(stores: &'a [ProjectStore], url: &str) -> Option<&'a ProjectStore> {
    match query_param(url, "project") {
        Some(k) => stores.iter().find(|s| s.key == k),
        None => stores.first(),
    }
}

/// Route a request to a body + content-type + status code. All read-only.
fn route(stores: &[ProjectStore], version: &AtomicU64, url: &str) -> (String, &'static str, u16) {
    let path = url.split('?').next().unwrap_or(url);
    let json = "application/json; charset=utf-8";
    let html = "text/html; charset=utf-8";

    // Per-project endpoints resolve a single store; aggregate endpoints take them all.
    let one = |f: &dyn Fn(&ProjectStore) -> Result<String>| -> Result<String> {
        match resolve(stores, url) {
            Some(s) => f(s),
            None => Ok("{\"error\":\"unknown project\"}".to_string()),
        }
    };

    let result: Result<String> = match path {
        "/" | "/index.html" => return (UI_HTML.to_string(), html, 200),
        "/vendor/cytoscape.min.js" => {
            return (CYTOSCAPE_JS.to_string(), "application/javascript; charset=utf-8", 200)
        }
        "/api/version" => Ok(format!("{{\"version\":{}}}", version.load(Ordering::SeqCst))),
        "/api/status" => api::status_all(stores),
        "/api/search" => api::search_all(stores, query_param(url, "q").as_deref().unwrap_or("")),
        "/api/graph/full" => api::full_graph_all(stores),
        "/api/graph" => one(&|s| api::graph(&s.conn, query_param(url, "focus").as_deref().unwrap_or(""))),
        "/api/symbol" => one(&|s| api::symbol(&s.conn, query_param(url, "id").as_deref().unwrap_or(""))),
        "/api/memory" => one(&|s| api::memory(&s.conn, query_param(url, "id").as_deref().unwrap_or(""))),
        _ => return ("{\"error\":\"not found\"}".to_string(), json, 404),
    };

    match result {
        Ok(body) => (body, json, 200),
        Err(e) => (format!("{{\"error\":{}}}", json_string(&e.to_string())), json, 500),
    }
}

/// JSON-encode a string (with surrounding quotes).
pub(crate) fn json_string(s: &str) -> String {
    serde_json::Value::String(s.to_string()).to_string()
}

fn query_param(url: &str, key: &str) -> Option<String> {
    let q = url.split_once('?')?.1;
    for pair in q.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        if k == key {
            return Some(url_decode(v));
        }
    }
    None
}

fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => match u8::from_str_radix(&s[i + 1..i + 3], 16) {
                Ok(b) => {
                    out.push(b);
                    i += 3;
                }
                Err(_) => {
                    out.push(bytes[i]);
                    i += 1;
                }
            },
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Best-effort: open `url` in the default browser. Never fails the server.
fn open_browser(url: &str) {
    let (cmd, args): (&str, &[&str]) = if cfg!(target_os = "macos") {
        ("open", &[])
    } else if cfg!(target_os = "windows") {
        ("cmd", &["/C", "start", ""])
    } else {
        ("xdg-open", &[])
    };
    let _ = std::process::Command::new(cmd)
        .args(args)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}
