/// HTTP MCP server — handles JSON-RPC over POST /mcp.
///
/// Cross-platform: uses axum (tokio-based).

use crate::AppState;
use axum::{
    Router,
    extract::State,
    http::{HeaderMap, Method, StatusCode},
    response::IntoResponse,
    routing::post,
};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

pub async fn serve(state: Arc<AppState>) {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::POST, Method::OPTIONS])
        .allow_headers(Any);

    let app = Router::new()
        .route("/mcp", post(handle_mcp))
        .layer(cors)
        .with_state(state.clone());

    let addr = format!("127.0.0.1:{}", state.config.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind MCP port");

    tracing::info!("MCP server listening on {addr}");
    axum::serve(listener, app).await.unwrap();
}

async fn handle_mcp(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    body: String,
) -> impl IntoResponse {
    let auth_ok = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.to_lowercase()
                .contains(&format!("bearer {}", state.config.token.to_lowercase()))
        });

    if !auth_ok {
        return (StatusCode::UNAUTHORIZED, r#"{"error":"unauthorized"}"#.to_string());
    }

    match super::router::handle_request(&state, &body).await {
        Some(response_json) => (StatusCode::OK, response_json),
        None => (StatusCode::NO_CONTENT, String::new()),
    }
}
