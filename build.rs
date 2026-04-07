fn main() {
    // Windows: no extra framework linking needed — the `windows` crate handles it

    // macOS: link frameworks (for cross-platform dev builds)
    #[cfg(target_os = "macos")]
    {
        // No macOS-specific frameworks needed — this is a Windows-focused project.
        // macOS builds are for development/testing only (server + embedding, no native automation).
    }

    // Embedding engine: compile llama_bridge.c against vendored llama.cpp
    #[cfg(feature = "embedding")]
    {
        // TODO: Phase 2 — build llama.cpp via cmake, compile llama_bridge.c via cc crate
        // cc::Build::new()
        //     .file("src/embedding/llama_bridge.c")
        //     .include("vendor/llama.cpp/include")
        //     .include("vendor/llama.cpp/ggml/include")
        //     .compile("llama_bridge");
    }
}
