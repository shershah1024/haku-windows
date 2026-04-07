/// Embedding engine — loads EmbeddingGemma-300M GGUF via llama.cpp C FFI.
///
/// Computes 256-dim embeddings for tool filtering by semantic similarity.
/// Caches embeddings in SQLite to avoid recomputation.

mod ffi;

use rusqlite::Connection;
use std::collections::HashMap;
use std::ffi::CString;
use std::path::Path;
use std::sync::Mutex;

const DIMS: usize = 256;

pub struct EmbeddingEngine {
    ctx: Mutex<*mut std::ffi::c_void>,
    dims: usize,
    tool_embeddings: std::sync::RwLock<HashMap<String, Vec<f32>>>,
    label_cache: Mutex<HashMap<String, Vec<f32>>>,
    label_db: Mutex<Option<Connection>>,
}

// SAFETY: The llama context is protected by Mutex, only accessed single-threaded.
unsafe impl Send for EmbeddingEngine {}
unsafe impl Sync for EmbeddingEngine {}

impl EmbeddingEngine {
    /// Load the GGUF model. Returns None if the model file doesn't exist.
    pub fn load(model_path: &Path) -> Option<Self> {
        if !model_path.exists() {
            tracing::warn!("Embedding model not found at {}", model_path.display());
            return None;
        }

        let path_cstr = CString::new(model_path.to_str()?).ok()?;
        let n_threads = (num_cpus() / 2).max(2) as i32;

        let ctx = unsafe { ffi::llama_embed_init(path_cstr.as_ptr(), n_threads) };
        if ctx.is_null() {
            tracing::error!("Failed to initialize embedding model");
            return None;
        }

        // Open label cache database
        let db_path = model_path.parent()?.join("labels.db");
        let db = Connection::open(&db_path).ok();
        if let Some(ref db) = db {
            let _ = db.execute_batch(
                "CREATE TABLE IF NOT EXISTS labels (label TEXT PRIMARY KEY, vec BLOB NOT NULL)"
            );
        }

        // Load cached labels into memory
        let mut label_cache = HashMap::new();
        if let Some(ref db) = db {
            if let Ok(mut stmt) = db.prepare("SELECT label, vec FROM labels") {
                let _ = stmt.query_map([], |row| {
                    let label: String = row.get(0)?;
                    let blob: Vec<u8> = row.get(1)?;
                    let floats: Vec<f32> = blob
                        .chunks_exact(4)
                        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                        .collect();
                    Ok((label, floats))
                }).map(|rows| {
                    for row in rows.flatten() {
                        label_cache.insert(row.0, row.1);
                    }
                });
            }
        }

        tracing::info!(
            cached_labels = label_cache.len(),
            "Embedding engine loaded"
        );

        Some(Self {
            ctx: Mutex::new(ctx),
            dims: DIMS,
            tool_embeddings: std::sync::RwLock::new(HashMap::new()),
            label_cache: Mutex::new(label_cache),
            label_db: Mutex::new(db),
        })
    }

    /// Compute embedding for a text string.
    pub fn embed(&self, text: &str) -> Option<Vec<f32>> {
        let ctx = self.ctx.lock().unwrap();
        let cstr = CString::new(text).ok()?;
        let mut output = vec![0.0f32; self.dims];

        let dim = unsafe {
            ffi::llama_embed_compute(*ctx, cstr.as_ptr(), output.as_mut_ptr(), self.dims as i32)
        };

        if dim > 0 {
            output.truncate(dim as usize);
            Some(output)
        } else {
            None
        }
    }

    /// Pre-cache embeddings for a set of tools.
    pub fn cache_tool_embeddings(&self, tools: &[(String, String)]) {
        let mut cache = self.label_cache.lock().unwrap();
        let mut tool_embs = self.tool_embeddings.write().unwrap();
        let mut new_labels: Vec<(String, Vec<f32>)> = Vec::new();

        for (name, label) in tools {
            if let Some(emb) = cache.get(label) {
                tool_embs.insert(name.clone(), emb.clone());
            } else if let Some(emb) = self.embed(label) {
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

    /// Rank tools by semantic similarity to a query. Returns top-K (name, score).
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

    pub fn shutdown(&self) {
        let ctx = self.ctx.lock().unwrap();
        if !ctx.is_null() {
            unsafe { ffi::llama_embed_free(*ctx) };
        }
    }
}

impl Drop for EmbeddingEngine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn dot_product(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

fn num_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}
