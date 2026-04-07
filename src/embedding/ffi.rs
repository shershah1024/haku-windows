/// FFI bindings to llama_bridge.c — thin C wrapper around llama.cpp.
///
/// The C bridge handles tokenization, batching, L2 normalization.
/// We just call 3 functions.

use std::ffi::c_char;
use std::ffi::c_int;
use std::ffi::c_void;

extern "C" {
    /// Initialize: load GGUF model, return context (NULL on failure).
    pub fn llama_embed_init(model_path: *const c_char, n_threads: c_int) -> *mut c_void;

    /// Compute normalized embedding for text.
    /// Output written to out_embedding (must have space for max_dims floats).
    /// Returns actual dimension count, or 0 on error.
    pub fn llama_embed_compute(
        ctx: *mut c_void,
        text: *const c_char,
        out_embedding: *mut f32,
        max_dims: c_int,
    ) -> c_int;

    /// Free context and model.
    pub fn llama_embed_free(ctx: *mut c_void);
}
