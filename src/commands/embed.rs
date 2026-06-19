//! `recanta embed` — compute embeddings for stored memories and enable semantic search
//! (PRD §12.1, v0.4). Uses a local provider (Ollama / LM Studio); if none is available it
//! explains how to set one up, and search stays FTS-only until then.

use std::path::Path;

use anyhow::{bail, Result};
use clap::Args;

use crate::embed;
use crate::memory;
use crate::project::{Config, EmbedSettings, Paths};
use crate::db;

#[derive(Debug, Args)]
pub struct EmbedArgs {
    /// Embedding model to use (otherwise auto-detect an embedding model on Ollama/LM Studio).
    #[arg(long)]
    pub model: Option<String>,

    /// Use the bundled offline model (fastembed), skipping local providers.
    #[arg(long)]
    pub bundled: bool,
}

pub fn run(args: EmbedArgs, project_override: Option<&Path>) -> Result<()> {
    let paths = Paths::discover(project_override)?;
    let mut cfg = Config::load(&paths.config)?;
    let conn = db::open_existing(&paths.db)?;

    // Reuse the prior provider's model unless overridden, so re-embeds stay in one space.
    let model = args.model.or_else(|| {
        if cfg.embeddings.provider == "fastembed" || cfg.embeddings.model.is_empty() {
            None
        } else {
            Some(cfg.embeddings.model.clone())
        }
    });
    let embedder: Box<dyn embed::Embedder> = if args.bundled {
        match embed::bundled() {
            Some(r) => r?,
            None => bail!("this build excludes the bundled model (fastembed feature is off)"),
        }
    } else {
        embed::for_embed(model.as_deref())
            .map_err(|e| anyhow::anyhow!("could not initialize any embedder (provider + bundled both failed): {e}"))?
    };

    // Collect memories to embed.
    let memories: Vec<(i64, String, String)> = {
        let mut stmt = conn.prepare(
            "SELECT id, title, content FROM memory_items WHERE status = 'active'",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    if memories.is_empty() {
        println!("No memories to embed yet (try `recanta remember …` or import sessions first).");
        return Ok(());
    }

    println!(
        "Embedding {} memories via {}/{} …",
        memories.len(),
        embedder.provider(),
        embedder.model()
    );

    let mut vectors: Vec<(i64, Vec<f32>)> = Vec::new();
    let mut dim = 0usize;
    for batch in memories.chunks(32) {
        let texts: Vec<String> = batch
            .iter()
            .map(|(_, t, c)| format!("{t}\n\n{}", truncate(c, 2000)))
            .collect();
        let embs = embedder.embed(&texts)?;
        for ((id, _, _), v) in batch.iter().zip(embs) {
            if !v.is_empty() {
                dim = v.len();
                vectors.push((*id, v));
            }
        }
    }
    if dim == 0 {
        bail!("provider returned empty embeddings");
    }

    // Store vectors in the same SQLite file (sqlite-vec).
    memory::ensure_vec_table(&conn, dim as u32)?;
    let tx = conn.unchecked_transaction()?;
    for (id, v) in &vectors {
        memory::store_vector(&tx, *id, v)?;
    }
    tx.commit()?;

    cfg.embeddings = EmbedSettings {
        enabled: true,
        provider: embedder.provider().to_string(),
        model: embedder.model().to_string(),
        dim: dim as u32,
    };
    cfg.save(&paths.config)?;

    println!(
        "Embedded {} memories (dim {}). Semantic search is on; `search` now ranks hybrid \
         (vector + FTS).",
        vectors.len(),
        dim
    );
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max).collect()
    }
}
