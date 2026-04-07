/// Embedding engine — loads EmbeddingGemma-300M GGUF via llama.cpp.
///
/// Computes 256-dim embeddings for tool filtering by semantic similarity.
/// Caches embeddings in SQLite to avoid recomputation across sessions.
///
/// Critical for web automation: a single page scan can produce 100+ tools.
/// We use semantic ranking to surface only the top-K most relevant to the
/// agent's intent.

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel, Special};
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};

const DIMS: usize = 256;
const N_CTX: u32 = 512;

pub struct EmbeddingEngine {
    backend: Arc<LlamaBackend>,
    model: Arc<LlamaModel>,
    /// Single context guarded by Mutex — llama contexts are not Send/Sync friendly
    /// for concurrent use. All embedding calls serialize through this.
    ctx_lock: Mutex<()>,
    dims: usize,
    /// Per-tool embeddings: tool name → vector
    tool_embeddings: RwLock<HashMap<String, Vec<f32>>>,
    /// In-memory cache: label → vector (avoids re-embedding identical labels)
    label_cache: Mutex<HashMap<String, Vec<f32>>>,
    /// SQLite-backed persistent label cache
    label_db: Mutex<Option<Connection>>,
}

impl EmbeddingEngine {
    /// Load the GGUF model from disk. Returns None if the file doesn't exist
    /// or fails to load — the server continues running, just without embedding-based filtering.
    pub fn load(model_path: &Path) -> Option<Self> {
        if !model_path.exists() {
            tracing::warn!("Embedding model not found at {}", model_path.display());
            return None;
        }

        let backend = match LlamaBackend::init() {
            Ok(b) => Arc::new(b),
            Err(e) => {
                tracing::error!("Failed to init llama backend: {e}");
                return None;
            }
        };

        let model_params = LlamaModelParams::default();
        let model = match LlamaModel::load_from_file(&backend, model_path, &model_params) {
            Ok(m) => Arc::new(m),
            Err(e) => {
                tracing::error!("Failed to load embedding model: {e}");
                return None;
            }
        };

        // Open SQLite cache next to the model
        let db_path = model_path
            .parent()
            .map(|p| p.join("labels.db"))
            .unwrap_or_else(|| std::path::PathBuf::from("labels.db"));
        let db = Connection::open(&db_path).ok();
        if let Some(ref db) = db {
            let _ = db.execute_batch(
                "CREATE TABLE IF NOT EXISTS labels (label TEXT PRIMARY KEY, vec BLOB NOT NULL)",
            );
        }

        // Load existing cached embeddings into memory
        let mut label_cache: HashMap<String, Vec<f32>> = HashMap::new();
        if let Some(ref db) = db {
            if let Ok(mut stmt) = db.prepare("SELECT label, vec FROM labels") {
                if let Ok(rows) = stmt.query_map([], |row| {
                    let label: String = row.get(0)?;
                    let blob: Vec<u8> = row.get(1)?;
                    let floats: Vec<f32> = blob
                        .chunks_exact(4)
                        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                        .collect();
                    Ok((label, floats))
                }) {
                    for r in rows.flatten() {
                        label_cache.insert(r.0, r.1);
                    }
                }
            }
        }

        tracing::info!(
            cached_labels = label_cache.len(),
            model = %model_path.display(),
            "Embedding engine loaded"
        );

        Some(Self {
            backend,
            model,
            ctx_lock: Mutex::new(()),
            dims: DIMS,
            tool_embeddings: RwLock::new(HashMap::new()),
            label_cache: Mutex::new(label_cache),
            label_db: Mutex::new(db),
        })
    }

    /// Compute a normalized embedding for a single text string.
    pub fn embed(&self, text: &str) -> Option<Vec<f32>> {
        let _guard = self.ctx_lock.lock().unwrap();

        // Each call creates a fresh context with embeddings=true.
        // This is slower than reusing a context but avoids state-leak bugs
        // between calls (and embedding inference is cheap on a 300M model).
        let n_threads = (num_cpus() / 2).max(2) as i32;
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(std::num::NonZeroU32::new(N_CTX))
            .with_n_threads(n_threads)
            .with_n_threads_batch(n_threads)
            .with_embeddings(true);

        let mut ctx = match self.model.new_context(&self.backend, ctx_params) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Failed to create embedding context: {e}");
                return None;
            }
        };

        // Tokenize
        let tokens = match self.model.str_to_token(text, AddBos::Always) {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("Tokenize failed: {e}");
                return None;
            }
        };

        if tokens.is_empty() {
            return None;
        }

        // Build a batch with logits=true on the last token (CLS pooling)
        let mut batch = LlamaBatch::new(tokens.len(), 1);
        for (i, token) in tokens.iter().enumerate() {
            let is_last = i == tokens.len() - 1;
            if let Err(e) = batch.add(*token, i as i32, &[0], is_last) {
                tracing::warn!("Batch add failed: {e}");
                return None;
            }
        }

        if let Err(e) = ctx.decode(&mut batch) {
            tracing::warn!("Decode failed: {e}");
            return None;
        }

        // Extract sequence embedding (pooled)
        let raw = match ctx.embeddings_seq_ith(0) {
            Ok(e) => e.to_vec(),
            Err(_) => {
                // Fallback: per-token embedding for last token
                match ctx.embeddings_ith(tokens.len() as i32 - 1) {
                    Ok(e) => e.to_vec(),
                    Err(e) => {
                        tracing::warn!("No embedding output: {e}");
                        return None;
                    }
                }
            }
        };

        // Truncate or pad to DIMS
        let dim = raw.len().min(self.dims);
        let mut emb = raw[..dim].to_vec();

        // L2 normalize
        let norm: f32 = emb.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 1e-9 {
            for x in emb.iter_mut() {
                *x /= norm;
            }
        }

        Some(emb)
    }

    /// Pre-cache embeddings for a set of (tool_name, label) pairs.
    /// Checks the SQLite cache first, computes only what's missing, then persists.
    pub fn cache_tool_embeddings(&self, tools: &[(String, String)]) {
        let mut cache = self.label_cache.lock().unwrap();
        let mut tool_embs = self.tool_embeddings.write().unwrap();
        let mut new_labels: Vec<(String, Vec<f32>)> = Vec::new();

        for (name, label) in tools {
            if let Some(emb) = cache.get(label) {
                tool_embs.insert(name.clone(), emb.clone());
                continue;
            }

            // Drop locks before calling embed (which takes its own lock)
            drop(cache);
            drop(tool_embs);

            let computed = self.embed(label);

            cache = self.label_cache.lock().unwrap();
            tool_embs = self.tool_embeddings.write().unwrap();

            if let Some(emb) = computed {
                cache.insert(label.clone(), emb.clone());
                tool_embs.insert(name.clone(), emb.clone());
                new_labels.push((label.clone(), emb));
            }
        }

        // Persist new labels to SQLite
        if !new_labels.is_empty() {
            if let Some(ref db) = *self.label_db.lock().unwrap() {
                for (label, emb) in &new_labels {
                    let blob: Vec<u8> = emb.iter().flat_map(|f| f.to_le_bytes()).collect();
                    let _ = db.execute(
                        "INSERT OR IGNORE INTO labels (label, vec) VALUES (?1, ?2)",
                        rusqlite::params![label, blob],
                    );
                }
            }
        }
    }

    /// Clear all tool embeddings (call when starting a new session/page).
    pub fn clear_tools(&self) {
        self.tool_embeddings.write().unwrap().clear();
    }

    /// Rank cached tools by cosine similarity to the query.
    /// Returns top-K (tool_name, similarity_score), highest first.
    pub fn rank_tools(&self, query: &str, top_k: usize) -> Vec<(String, f32)> {
        let query_emb = match self.embed(query) {
            Some(e) => e,
            None => return Vec::new(),
        };

        let tool_embs = self.tool_embeddings.read().unwrap();
        let mut scores: Vec<(String, f32)> = tool_embs
            .iter()
            .map(|(name, emb)| {
                let score = dot_product(&query_emb, emb);
                (name.clone(), score)
            })
            .collect();

        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scores.truncate(top_k);
        scores
    }
}

fn dot_product(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    a[..n].iter().zip(b[..n].iter()).map(|(x, y)| x * y).sum()
}

fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}
