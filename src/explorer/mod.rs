//! Recanta Explorer (PRD §15): a local, read-only web UI to see and search memory and
//! the code graph. Served by `recanta serve` from the single binary — no Node/Nuxt build
//! and no external CDN, so it preserves the zero-runtime, local-first guarantee. Binds
//! `127.0.0.1` only, no auth, no telemetry, no write endpoints.

mod api;

use std::path::Path;

use anyhow::Result;
use tiny_http::{Header, Response, Server};

use crate::project::{Config, Paths};
use crate::{db, repo};

/// The single-file UI, embedded in the binary.
const UI_HTML: &str = include_str!("ui.html");
/// Vendored Cytoscape.js (embedded so the graph works fully offline — no CDN).
const CYTOSCAPE_JS: &str = include_str!("vendor/cytoscape.min.js");

/// Start the Explorer server (blocks until interrupted).
pub fn run(paths: &Paths, host: &str, port: u16, open: bool) -> Result<()> {
    // One connection reused across requests (the server loop is single-threaded).
    let conn = db::open_existing(&paths.db)?;
    let cfg = Config::load(&paths.config)?;
    let project_id = repo::current_project_id(&conn).ok();
    let repo_id = project_id
        .as_deref()
        .and_then(|pid| repo::ensure_repository(&conn, pid, &paths.root, None).ok());

    let addr = format!("{host}:{port}");
    let server = Server::http(&addr).map_err(|e| anyhow::anyhow!("binding {addr}: {e}"))?;
    let url = format!("http://{addr}");
    println!("Recanta Explorer → {url}  (read-only; Ctrl-C to stop)");
    if open {
        open_browser(&url);
    }

    for request in server.incoming_requests() {
        let (body, ctype, code) = route(&conn, &cfg, &paths.root, repo_id, request.url());
        let header = Header::from_bytes(b"Content-Type".as_slice(), ctype.as_bytes()).unwrap();
        let response = Response::from_string(body).with_status_code(code).with_header(header);
        let _ = request.respond(response);
    }
    Ok(())
}

/// Route a request to a body + content-type + status code. All read-only.
fn route(
    conn: &rusqlite::Connection,
    cfg: &Config,
    root: &Path,
    repo_id: Option<i64>,
    url: &str,
) -> (String, &'static str, u16) {
    let path = url.split('?').next().unwrap_or(url);
    let json = "application/json; charset=utf-8";
    let html = "text/html; charset=utf-8";

    let result: Result<String> = match path {
        "/" | "/index.html" => return (UI_HTML.to_string(), html, 200),
        "/vendor/cytoscape.min.js" => {
            return (CYTOSCAPE_JS.to_string(), "application/javascript; charset=utf-8", 200)
        }
        "/api/status" => api::status(conn, cfg, root, repo_id),
        "/api/search" => api::search(conn, root, repo_id, query_param(url, "q").as_deref().unwrap_or("")),
        "/api/graph/full" => api::full_graph(conn, repo_id),
        "/api/graph" => api::graph(conn, query_param(url, "focus").as_deref().unwrap_or("")),
        "/api/symbol" => api::symbol(conn, query_param(url, "id").as_deref().unwrap_or("")),
        "/api/memory" => api::memory(conn, query_param(url, "id").as_deref().unwrap_or("")),
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
