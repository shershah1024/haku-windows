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
    #[cfg(feature = "embedding")]
    pub embedding: Option<embedding::EmbeddingEngine>,
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
        let model_path = config::Config::config_dir()
            .join("models")
            .join("embeddinggemma-300m-qat-Q8_0.gguf");
        embedding::EmbeddingEngine::load(&model_path)
    };

    let state = Arc::new(AppState {
        config: config.clone(),
        session_manager: RwLock::new(session::SessionManager::new()),
        browser_sessions: RwLock::new(session::BrowserSessionManager::new()),
        flow_store: Mutex::new(flow_store),
        platform,
        license,
        #[cfg(feature = "embedding")]
        embedding,
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
