mod config;
#[cfg(feature = "embedding")]
mod embedding;
mod flow;
mod license;
mod logging;
mod platform;
mod server;
mod session;
mod setup;

use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

/// Shared application state accessible from all handlers.
pub struct AppState {
    pub config: config::Config,
    pub session_manager: RwLock<session::SessionManager>,
    pub browser_sessions: RwLock<session::BrowserSessionManager>,
    pub flow_store: Mutex<flow::FlowStore>,
    pub platform: Box<dyn platform::Platform>,
    pub license: license::LicenseManager,
    /// Hot-swappable embedding engine: None until the model file is downloaded
    /// and loaded. Uses substring fallback in router until available.
    #[cfg(feature = "embedding")]
    pub embedding: RwLock<Option<embedding::EmbeddingEngine>>,
}

#[tokio::main]
async fn main() {
    // Handle CLI subcommands (--version, --setup, --download-model, --activate)
    if let Some(code) = setup::handle_cli() {
        std::process::exit(code);
    }

    logging::init();
    tracing::info!("Haku starting up");

    let config = config::Config::load_or_create();
    tracing::info!(port = config.port, ws_port = config.ws_port, "Config loaded");

    let platform = platform::create();
    let ax_ok = platform.check_accessibility_permission();
    tracing::info!(accessibility = ax_ok, "Permission check");

    let flow_store = flow::FlowStore::open().expect("Failed to open flow database");

    let license = license::LicenseManager::new(&config);
    let license_state = license.check_state();
    tracing::info!(?license_state, "License state");

    #[cfg(feature = "embedding")]
    let embedding = {
        let path = setup::model_path();
        if path.exists() {
            embedding::EmbeddingEngine::load(&path)
        } else {
            tracing::info!(
                "Embedding model not present at {} — will download in background after startup. Substring search active until ready.",
                path.display()
            );
            None
        }
    };

    let state = Arc::new(AppState {
        config: config.clone(),
        session_manager: RwLock::new(session::SessionManager::new()),
        browser_sessions: RwLock::new(session::BrowserSessionManager::new()),
        flow_store: Mutex::new(flow_store),
        platform,
        license,
        #[cfg(feature = "embedding")]
        embedding: tokio::sync::RwLock::new(embedding),
    });

    config.write_internal_config();

    let mcp_handle = {
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            server::mcp::serve(state).await;
        })
    };

    let ws_handle = {
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            server::websocket::serve(state).await;
        })
    };

    // Background model download — only if embedding feature on AND model missing.
    #[cfg(feature = "embedding")]
    {
        let state_dl = Arc::clone(&state);
        tokio::spawn(async move {
            if state_dl.embedding.read().await.is_some() {
                return; // already loaded
            }
            tracing::info!("Starting background model download from {}", setup::model_url());
            let result = tokio::task::spawn_blocking(|| {
                setup::download_model_with_progress(|written, total| {
                    if let Some(t) = total {
                        let pct = (written as f64 / t as f64 * 100.0) as u32;
                        if pct % 10 == 0 && written > 0 {
                            tracing::info!("Model download: {pct}% ({written}/{t} bytes)");
                        }
                    }
                })
            })
            .await;

            match result {
                Ok(Ok(path)) => {
                    tracing::info!("Model downloaded to {}, loading...", path.display());
                    let engine = tokio::task::spawn_blocking(move || {
                        embedding::EmbeddingEngine::load(&path)
                    })
                    .await
                    .ok()
                    .flatten();
                    if engine.is_some() {
                        *state_dl.embedding.write().await = engine;
                        tracing::info!("Embedding engine ready — semantic search active");
                    }
                }
                Ok(Err(e)) => tracing::error!("Model download failed: {e}"),
                Err(e) => tracing::error!("Model download task panicked: {e}"),
            }
        });
    }

    tracing::info!(
        "Haku running — MCP on 127.0.0.1:{}, WebSocket on 127.0.0.1:{}",
        config.port,
        config.ws_port,
    );

    tokio::select! {
        _ = mcp_handle => {},
        _ = ws_handle => {},
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Shutting down");
        },
    }
}
