#ifndef LLAMA_BRIDGE_H
#define LLAMA_BRIDGE_H

#include <stdbool.h>
#include <stdint.h>

typedef struct llama_embed_ctx llama_embed_ctx;

// Initialize: load GGUF model, return context (NULL on failure)
llama_embed_ctx* llama_embed_init(const char* model_path, int n_threads);

// Compute normalized embedding for text.
// Output written to out_embedding (must have space for max_dims floats).
// Returns actual dimension count, or 0 on error.
int llama_embed_compute(llama_embed_ctx* ctx, const char* text,
                        float* out_embedding, int max_dims);

// Free context and model
void llama_embed_free(llama_embed_ctx* ctx);

#endif
