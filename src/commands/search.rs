//! `recanta search` — hybrid retrieval over memory (PRD §8.2, §13). v0.1 is FTS5/BM25
//! only. Searches the project store and the global (`user`) store, merges, ranks
//! deterministically by `(rank, id)`, and emits budget-shaped output.

use std::path::Path;

use anyhow::Result;
use clap::Args;

use crate::memory::{self, Hit, Scope};
use crate::output::{self, Format};
use crate::project::{self, Paths};
use crate::db;

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

    let mut hits: Vec<Hit> = Vec::new();

    // Project store (unless restricted to user scope).
    if args.scope != Some(Scope::User) {
        if let Ok(paths) = Paths::discover(project_override) {
            let conn = db::open_existing(&paths.db)?;
            let filter = args.scope.filter(|s| *s != Scope::User);
            hits.extend(memory::search(&conn, &match_expr, filter, FETCH_LIMIT)?);
        }
    }

    // Global user store (unless restricted away from user scope).
    if args.scope.map_or(true, |s| s == Scope::User) {
        let path = project::global_db_path()?;
        if path.is_file() {
            let conn = db::open_existing(&path)?;
            hits.extend(memory::search(&conn, &match_expr, Some(Scope::User), FETCH_LIMIT)?);
        }
    }

    // Merge two corpora: order by BM25 then id for a deterministic, stable result.
    hits.sort_by(|a, b| {
        a.rank
            .partial_cmp(&b.rank)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.row.id.cmp(&b.row.id))
    });

    if hits.is_empty() {
        println!("no matches for {:?}", args.query);
        return Ok(());
    }

    match args.format {
        Format::Json => print!("{}", render_json(&args.query, &hits)?),
        Format::IdsOnly => {
            for h in &hits {
                println!("{}", h.row.id);
            }
        }
        Format::Compact => {
            let blocks: Vec<String> = hits.iter().map(|h| render_block(h, args.with_evidence)).collect();
            println!("{}", output::pack(blocks, args.budget));
        }
    }
    Ok(())
}

fn render_block(h: &Hit, with_evidence: bool) -> String {
    let r = &h.row;
    let body = if with_evidence {
        r.content.clone()
    } else {
        snippet(&r.content, 140)
    };
    format!(
        "#{} [{}/{}] {} ({})\n   {}",
        r.id, r.scope, r.mem_type, r.title, r.importance, body
    )
}

fn render_json(query: &str, hits: &[Hit]) -> Result<String> {
    use serde_json::json;
    let results: Vec<_> = hits
        .iter()
        .map(|h| {
            json!({
                "id": h.row.id,
                "scope": h.row.scope,
                "type": h.row.mem_type,
                "importance": h.row.importance,
                "title": h.row.title,
                "content": h.row.content,
                "rank": h.rank,
                "updated_at": h.row.updated_at,
            })
        })
        .collect();
    let doc = json!({
        "schema_version": 1,
        "query": query,
        "results": results,
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
