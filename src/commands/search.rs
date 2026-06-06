//! `recanta search` — hybrid retrieval over memory (PRD §8.2, §13). v0.1 is FTS5/BM25
//! only. Searches the project store and the global (`user`) store, merges, ranks
//! deterministically by `(rank, id)`, and emits budget-shaped output.

use std::path::Path;

use anyhow::Result;
use clap::Args;

use crate::documents::{self, DocHit};
use crate::memory::{self, Hit, Scope};
use crate::output::{self, Format};
use crate::project::{self, Paths};
use crate::db;

/// A unified search result across the memory and document corpora.
enum Match {
    Memory(Hit),
    Document(DocHit),
}

impl Match {
    fn rank(&self) -> f64 {
        match self {
            Match::Memory(h) => h.rank,
            Match::Document(d) => d.rank,
        }
    }

    /// Tie-break key: keep memory and document id-spaces from colliding.
    fn order_key(&self) -> (u8, i64) {
        match self {
            Match::Memory(h) => (0, h.row.id),
            Match::Document(d) => (1, d.id),
        }
    }
}

/// Raw rows fetched per store before budget trimming.
const FETCH_LIMIT: usize = 50;

#[derive(Debug, Args)]
pub struct SearchArgs {
    /// The search query (free text).
    pub query: String,

    /// Restrict to a single scope. Omit to search project + user memory.
    #[arg(long, value_enum)]
    pub scope: Option<Scope>,

    /// Output budget in characters (text formats only).
    #[arg(long, default_value_t = output::SEARCH_DEFAULT)]
    pub budget: usize,

    /// Show full memory content instead of a snippet.
    #[arg(long)]
    pub with_evidence: bool,

    /// Output format.
    #[arg(long, value_enum, default_value = "compact")]
    pub format: Format,
}

pub fn run(args: SearchArgs, project_override: Option<&Path>) -> Result<()> {
    let Some(match_expr) = memory::fts_query(&args.query) else {
        println!("no searchable terms in query");
        return Ok(());
    };

    let mut results: Vec<Match> = Vec::new();

    // Project store + ingested documents (unless restricted to user scope).
    if args.scope != Some(Scope::User) {
        if let Ok(paths) = Paths::discover(project_override) {
            let conn = db::open_existing(&paths.db)?;
            let filter = args.scope.filter(|s| *s != Scope::User);
            results.extend(memory::search(&conn, &match_expr, filter, FETCH_LIMIT)?.into_iter().map(Match::Memory));
            // Documents have no scope; include them only on an unrestricted search.
            if args.scope.is_none() {
                results.extend(documents::search(&conn, &match_expr, FETCH_LIMIT)?.into_iter().map(Match::Document));
            }
        }
    }

    // Global user store (unless restricted away from user scope).
    if args.scope.map_or(true, |s| s == Scope::User) {
        let path = project::global_db_path()?;
        if path.is_file() {
            let conn = db::open_existing(&path)?;
            results.extend(
                memory::search(&conn, &match_expr, Some(Scope::User), FETCH_LIMIT)?
                    .into_iter()
                    .map(Match::Memory),
            );
        }
    }

    // Merge the corpora: order by BM25 then a namespaced id for deterministic output.
    results.sort_by(|a, b| {
        a.rank()
            .partial_cmp(&b.rank())
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.order_key().cmp(&b.order_key()))
    });

    if results.is_empty() {
        println!("no matches for {:?}", args.query);
        return Ok(());
    }

    match args.format {
        Format::Json => print!("{}", render_json(&args.query, &results)?),
        Format::IdsOnly => {
            for m in &results {
                match m {
                    Match::Memory(h) => println!("{}", h.row.id),
                    Match::Document(d) => println!("doc:{}", d.id),
                }
            }
        }
        Format::Compact => {
            let blocks: Vec<String> =
                results.iter().map(|m| render_block(m, args.with_evidence)).collect();
            println!("{}", output::pack(blocks, args.budget));
        }
    }
    Ok(())
}

fn render_block(m: &Match, with_evidence: bool) -> String {
    match m {
        Match::Memory(h) => {
            let r = &h.row;
            let body = if with_evidence { r.content.clone() } else { snippet(&r.content, 140) };
            format!("#{} [{}/{}] {} ({})\n   {}", r.id, r.scope, r.mem_type, r.title, r.importance, body)
        }
        Match::Document(d) => {
            let snip = d.snippet.split_whitespace().collect::<Vec<_>>().join(" ");
            format!("doc:{} [document] {} ({})\n   {}", d.id, d.title, d.path, snip)
        }
    }
}

fn render_json(query: &str, results: &[Match]) -> Result<String> {
    use serde_json::json;
    let items: Vec<_> = results
        .iter()
        .map(|m| match m {
            Match::Memory(h) => json!({
                "kind": "memory",
                "id": h.row.id,
                "scope": h.row.scope,
                "type": h.row.mem_type,
                "importance": h.row.importance,
                "title": h.row.title,
                "content": h.row.content,
                "rank": h.rank,
                "updated_at": h.row.updated_at,
            }),
            Match::Document(d) => json!({
                "kind": "document",
                "id": d.id,
                "title": d.title,
                "path": d.path,
                "snippet": d.snippet,
                "rank": d.rank,
            }),
        })
        .collect();
    let doc = json!({
        "schema_version": 1,
        "query": query,
        "results": items,
    });
    Ok(serde_json::to_string_pretty(&doc)? + "\n")
}

/// Single-line snippet of `text`, truncated to `max` characters.
fn snippet(text: &str, max: usize) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        flat
    } else {
        let mut s: String = flat.chars().take(max).collect();
        s.push('…');
        s
    }
}
