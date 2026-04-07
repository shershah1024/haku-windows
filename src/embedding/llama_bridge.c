#include "llama_bridge.h"
#include "llama.h"
#include <stdlib.h>
#include <string.h>
#include <math.h>

struct llama_embed_ctx {
    struct llama_model  *model;
    struct llama_context *ctx;
    int n_embd;
};

llama_embed_ctx* llama_embed_init(const char* model_path, int n_threads) {
    llama_backend_init();

    struct llama_model_params mparams = llama_model_default_params();
    struct llama_model *model = llama_model_load_from_file(model_path, mparams);
    if (!model) return NULL;

    struct llama_context_params cparams = llama_context_default_params();
    cparams.n_ctx      = 512;
    cparams.n_threads   = n_threads;
    cparams.n_threads_batch = n_threads;
    cparams.embeddings  = true;

    struct llama_context *ctx = llama_init_from_model(model, cparams);
    if (!ctx) { llama_model_free(model); return NULL; }

    llama_embed_ctx *ectx = (llama_embed_ctx *)malloc(sizeof(llama_embed_ctx));
    ectx->model  = model;
    ectx->ctx    = ctx;
    ectx->n_embd = llama_model_n_embd(model);
    return ectx;
}

int llama_embed_compute(llama_embed_ctx* ctx, const char* text,
                        float* out_embedding, int max_dims) {
    if (!ctx || !text || !out_embedding) return 0;

    int text_len = (int)strlen(text);
    const struct llama_vocab *vocab = llama_model_get_vocab(ctx->model);

    // Tokenize — first call with NULL buffer returns negative required size
    int n_tokens = -llama_tokenize(vocab, text, text_len, NULL, 0, true, false);
    if (n_tokens <= 0) return 0;

    llama_token *tokens = (llama_token *)malloc(n_tokens * sizeof(llama_token));
    llama_tokenize(vocab, text, text_len, tokens, n_tokens, true, false);

    // Build batch
    struct llama_batch batch = llama_batch_init(n_tokens, 0, 1);
    for (int i = 0; i < n_tokens; i++) {
        batch.token[i]      = tokens[i];
        batch.pos[i]        = i;
        batch.n_seq_id[i]   = 1;
        batch.seq_id[i][0]  = 0;
        batch.logits[i]     = 0;
    }
    batch.logits[n_tokens - 1] = 1; // need output for last token
    batch.n_tokens = n_tokens;

    // Decode
    if (llama_decode(ctx->ctx, batch) != 0) {
        free(tokens);
        llama_batch_free(batch);
        return 0;
    }

    // Extract embedding
    float *embd = llama_get_embeddings_seq(ctx->ctx, 0);
    if (!embd) embd = llama_get_embeddings(ctx->ctx);
    if (!embd) {
        free(tokens);
        llama_batch_free(batch);
        return 0;
    }

    int dim = ctx->n_embd < max_dims ? ctx->n_embd : max_dims;

    // L2 normalize
    float norm = 0.0f;
    for (int i = 0; i < dim; i++) norm += embd[i] * embd[i];
    norm = sqrtf(norm);
    if (norm > 1e-9f) {
        for (int i = 0; i < dim; i++) out_embedding[i] = embd[i] / norm;
    } else {
        memcpy(out_embedding, embd, dim * sizeof(float));
    }

    free(tokens);
    llama_batch_free(batch);
    return dim;
}

void llama_embed_free(llama_embed_ctx* ctx) {
    if (!ctx) return;
    if (ctx->ctx)   llama_free(ctx->ctx);
    if (ctx->model) llama_model_free(ctx->model);
    free(ctx);
}
