//! Embedding providers for semantic search (PRD §12.1, v0.4). Pluggable with a fallback
//! chain: a local provider (Ollama / LM Studio / any OpenAI-compatible server) is
//! preferred; a bundled model (fastembed) is the offline fallback (added separately);
//! and when no embedder is available the product degrades to FTS-only — the hard rule.
//!
//! Local-first: providers are only ever localhost endpoints the user already runs.

use std::time::Duration;

use anyhow::{anyhow, Result};
use serde_json::{json, Value};

/// Serialize a vector as the little-endian f32 blob `sqlite-vec` stores.
pub fn to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// A source of text embeddings.
pub trait Embedder {
    /// Embed a batch of texts. All returned vectors share one dimension.
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
    /// Human label, e.g. `ollama`.
    fn provider(&self) -> &str;
    /// Model identifier.
    fn model(&self) -> &str;
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .build()
        .into()
}

/// Ollama embeddings (`POST /api/embed`).
pub struct Ollama {
    base: String,
    model: String,
}

impl Embedder for Ollama {
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut resp = agent()
            .post(format!("{}/api/embed", self.base))
            .send_json(json!({ "model": self.model, "input": texts }))
            .map_err(|e| anyhow!("ollama embed request: {e}"))?;
        let v: Value = resp.body_mut().read_json().map_err(|e| anyhow!("ollama embed body: {e}"))?;
        let arr = v.get("embeddings").and_then(|x| x.as_array())
            .ok_or_else(|| anyhow!("ollama: no embeddings in response ({})", truncate(&v)))?;
        Ok(arr.iter().map(parse_vec).collect())
    }
    fn provider(&self) -> &str { "ollama" }
    fn model(&self) -> &str { &self.model }
}

/// OpenAI-compatible embeddings (`POST /v1/embeddings`) — LM Studio, llama.cpp, etc.
pub struct OpenAiCompatible {
    base: String,
    model: String,
    label: String,
}

impl Embedder for OpenAiCompatible {
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut resp = agent()
            .post(format!("{}/v1/embeddings", self.base))
            .send_json(json!({ "model": self.model, "input": texts }))
            .map_err(|e| anyhow!("{} embed request: {e}", self.label))?;
        let v: Value = resp.body_mut().read_json().map_err(|e| anyhow!("{} embed body: {e}", self.label))?;
        let data = v.get("data").and_then(|x| x.as_array())
            .ok_or_else(|| anyhow!("{}: no data in response ({})", self.label, truncate(&v)))?;
        Ok(data.iter().map(|d| parse_vec(d.get("embedding").unwrap_or(&Value::Null))).collect())
    }
    fn provider(&self) -> &str { &self.label }
    fn model(&self) -> &str { &self.model }
}

/// Bundled offline embedder (fastembed, ONNX/CPU). Downloads a small model
/// (all-MiniLM-L6-v2, 384-dim) to a cache on first use; works with no external server.
pub struct FastEmbed {
    model: std::sync::Mutex<fastembed::TextEmbedding>,
}

impl FastEmbed {
    pub fn new() -> Result<Self> {
        let model = fastembed::TextEmbedding::try_new(
            fastembed::InitOptions::new(fastembed::EmbeddingModel::AllMiniLML6V2)
                .with_show_download_progress(true),
        )
        .map_err(|e| anyhow!("fastembed init: {e}"))?;
        Ok(FastEmbed { model: std::sync::Mutex::new(model) })
    }
}

impl Embedder for FastEmbed {
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let docs: Vec<&str> = texts.iter().map(String::as_str).collect();
        self.model
            .lock()
            .unwrap()
            .embed(docs, None)
            .map_err(|e| anyhow!("fastembed embed: {e}"))
    }
    fn provider(&self) -> &str { "fastembed" }
    fn model(&self) -> &str { "all-MiniLM-L6-v2" }
}

fn parse_vec(v: &Value) -> Vec<f32> {
    v.as_array()
        .map(|a| a.iter().filter_map(|x| x.as_f64().map(|f| f as f32)).collect())
        .unwrap_or_default()
}

fn truncate(v: &Value) -> String {
    v.to_string().chars().take(160).collect()
}

const OLLAMA: &str = "http://localhost:11434";
const LMSTUDIO: &str = "http://localhost:1234";

/// Choose an embedder for `recanta embed` (the fallback chain: a local provider if one
/// has an embedding model, else the bundled fastembed model). `model` forces a specific
/// provider model. Almost always succeeds (fastembed downloads on demand); errors only if
/// the bundled model can't be initialized (e.g. no network on first run).
pub fn for_embed(model: Option<&str>) -> Result<Box<dyn Embedder>> {
    if let Some(m) = model {
        let oll = Ollama { base: OLLAMA.into(), model: m.into() };
        if probe(&oll) {
            return Ok(Box::new(oll));
        }
        let oai = OpenAiCompatible { base: LMSTUDIO.into(), model: m.into(), label: "lmstudio".into() };
        if probe(&oai) {
            return Ok(Box::new(oai));
        }
    } else {
        if let Some(m) = ollama_embed_model(OLLAMA) {
            let p = Ollama { base: OLLAMA.into(), model: m };
            if probe(&p) {
                return Ok(Box::new(p));
            }
        }
        if let Some(m) = openai_embed_model(LMSTUDIO) {
            let p = OpenAiCompatible { base: LMSTUDIO.into(), model: m, label: "lmstudio".into() };
            if probe(&p) {
                return Ok(Box::new(p));
            }
        }
    }
    eprintln!("No embedding provider found; using the bundled model (fastembed). First run downloads it…");
    Ok(Box::new(FastEmbed::new()?))
}

/// Rebuild the embedder recorded at embed time, so query embeddings land in the same
/// vector space. Returns `None` when the recorded provider is currently unreachable (the
/// caller then ranks FTS-only).
pub fn for_search(provider: &str, model: &str) -> Option<Box<dyn Embedder>> {
    match provider {
        "fastembed" => FastEmbed::new().ok().map(|f| Box::new(f) as Box<dyn Embedder>),
        "ollama" => {
            let p = Ollama { base: OLLAMA.into(), model: model.into() };
            probe(&p).then(|| Box::new(p) as Box<dyn Embedder>)
        }
        "lmstudio" => {
            let p = OpenAiCompatible { base: LMSTUDIO.into(), model: model.into(), label: "lmstudio".into() };
            probe(&p).then(|| Box::new(p) as Box<dyn Embedder>)
        }
        _ => None,
    }
}

/// A model name that looks like a text-embedding model.
fn looks_like_embedding(name: &str) -> bool {
    let n = name.to_lowercase();
    ["embed", "bge", "minilm", "gte", "e5", "nomic", "mxbai"].iter().any(|k| n.contains(k))
}

fn ollama_embed_model(base: &str) -> Option<String> {
    let mut resp = quick(base, "/api/tags")?;
    let v: Value = resp.body_mut().read_json().ok()?;
    v.get("models")?.as_array()?.iter()
        .filter_map(|m| m.get("name").and_then(|x| x.as_str()))
        .find(|n| looks_like_embedding(n))
        .map(str::to_string)
}

fn openai_embed_model(base: &str) -> Option<String> {
    let mut resp = quick(base, "/v1/models")?;
    let v: Value = resp.body_mut().read_json().ok()?;
    v.get("data")?.as_array()?.iter()
        .filter_map(|m| m.get("id").and_then(|x| x.as_str()))
        .find(|n| looks_like_embedding(n))
        .map(str::to_string)
}

fn quick(base: &str, path: &str) -> Option<ureq::http::Response<ureq::Body>> {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(2)))
        .build()
        .new_agent()
        .get(format!("{base}{path}"))
        .call()
        .ok()
}

/// Confirm a provider actually returns a non-empty embedding.
fn probe(e: &dyn Embedder) -> bool {
    matches!(e.embed(&["recanta".to_string()]), Ok(v) if v.first().is_some_and(|x| !x.is_empty()))
}

#[cfg(test)]
mod fastembed_smoke {
    use super::Embedder;

    #[test]
    #[ignore = "downloads a model on first run"]
    fn fastembed_embeds_384() {
        let fe = super::FastEmbed::new().unwrap();
        let v = fe.embed(&["hello world".to_string(), "another text".to_string()]).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].len(), 384, "all-MiniLM-L6-v2 is 384-dim");
    }
}
