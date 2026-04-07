/// WebSocket server for browser extension bridge.
///
/// Cross-platform: uses axum's built-in WebSocket support.
/// Extension side panel connects here, registers tools, receives commands.

use crate::AppState;
use axum::{
    Router,
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    response::IntoResponse,
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::mpsc;

pub async fn serve(state: Arc<AppState>) {
    let app = Router::new()
        .route("/ws", get(ws_handler))
        .with_state(state.clone());

    let addr = format!("127.0.0.1:{}", state.config.ws_port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind WebSocket port");

    tracing::info!("WebSocket server listening on {addr}");
    axum::serve(listener, app).await.unwrap();
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut ws_sender, mut ws_receiver) = socket.split();

    let session_id = uuid::Uuid::new_v4().to_string();
    tracing::info!(session_id = %session_id, "Browser connected");

    // mpsc channel: anyone with the tx can push a message out to this WebSocket.
    // This is how the router dispatches tool_call messages back to the extension.
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    // Register the session with its writer channel
    {
        let mut mgr = state.browser_sessions.write().await;
        mgr.create_session(&session_id, tx.clone());
    }

    // Writer task: drains rx and forwards to the WebSocket sender.
    let writer = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if ws_sender.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    // Initial messages: session id + license state
    let _ = tx.send(json!({"type": "session_id", "sessionId": &session_id}).to_string());
    let _ = tx.send(state.license.to_ws_message().to_string());

    // Reader loop
    while let Some(Ok(msg)) = ws_receiver.next().await {
        match msg {
            Message::Text(text) => {
                let parsed: Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                let msg_type = parsed["type"].as_str().unwrap_or("");

                match msg_type {
                    "ping" => {
                        let _ = tx.send(r#"{"type":"pong"}"#.to_string());
                    }

                    "register" => {
                        let source = parsed["source"].as_str().unwrap_or("");
                        tracing::info!(source, session_id = %session_id, "Extension registered");
                    }

                    "page_info" => {
                        let url = parsed["url"].as_str().unwrap_or("");
                        let title = parsed["title"].as_str().unwrap_or("");
                        tracing::info!(title, url, "Page info");
                    }

                    "register_tool" => {
                        if let Some(tool_def) = parsed["tool"].as_object() {
                            let name = tool_def.get("name").and_then(|v| v.as_str()).unwrap_or("");
                            let desc = tool_def
                                .get("description")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            let schema = tool_def
                                .get("inputSchema")
                                .cloned()
                                .unwrap_or(json!({"type": "object", "properties": {}}));

                            let mut mgr = state.browser_sessions.write().await;
                            mgr.register_browser_tool(&session_id, name, desc, &schema);
                        }
                    }

                    "register_elements" => {
                        // Bulk registration from page-agent.js
                        if let Some(elements) = parsed["elements"].as_array() {
                            let mut mgr = state.browser_sessions.write().await;
                            for el in elements {
                                let label = el["label"].as_str().unwrap_or("");
                                let kind = el["kind"].as_str().unwrap_or("");
                                if label.is_empty() {
                                    continue;
                                }

                                let prefix = match kind {
                                    "button" => "click",
                                    "link" => "navigate",
                                    "input" | "editor" => "fill",
                                    "select" | "combobox" => "select",
                                    "toggle" | "switch" => "toggle",
                                    "slider" => "set",
                                    "tab_group" => "switch_tab",
                                    "radio_group" => "select",
                                    _ => continue,
                                };

                                let slug = label
                                    .to_lowercase()
                                    .chars()
                                    .map(|c| if c.is_alphanumeric() { c } else { '_' })
                                    .collect::<String>()
                                    .trim_matches('_')
                                    .chars()
                                    .take(50)
                                    .collect::<String>();

                                if slug.is_empty() {
                                    continue;
                                }

                                let tool_name = format!("{}_{}", prefix, slug);
                                let desc = format!("{} '{}'", prefix, label);

                                let mut schema = json!({"type": "object", "properties": {}});
                                if prefix == "fill" || prefix == "select" || prefix == "set" {
                                    schema["properties"] = json!({"value": {"type": "string", "description": "Value to set"}});
                                    schema["required"] = json!(["value"]);
                                }

                                mgr.register_browser_tool(&session_id, &tool_name, &desc, &schema);
                            }
                            mgr.mark_ready(&session_id);
                            tracing::info!(count = elements.len(), "Registered page elements");
                        }
                    }

                    "unregister_tool" => {
                        if let Some(name) = parsed["name"].as_str() {
                            let mut mgr = state.browser_sessions.write().await;
                            mgr.unregister_browser_tool(&session_id, name);
                        }
                    }

                    "tools_ready" => {
                        tracing::info!(session_id = %session_id, "Tools ready");
                        let mut mgr = state.browser_sessions.write().await;
                        mgr.mark_ready(&session_id);
                    }

                    "tool_result" => {
                        if let (Some(call_id), Some(result)) =
                            (parsed["callId"].as_str(), parsed["result"].as_object())
                        {
                            let mut mgr = state.browser_sessions.write().await;
                            mgr.handle_tool_result(call_id, &Value::Object(result.clone()));
                        }
                    }

                    "tool_error" => {
                        if let (Some(call_id), Some(error)) =
                            (parsed["callId"].as_str(), parsed["error"].as_str())
                        {
                            let mut mgr = state.browser_sessions.write().await;
                            mgr.handle_tool_error(call_id, error);
                        }
                    }

                    _ => {
                        tracing::debug!(msg_type, "Unknown WS message type");
                    }
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    tracing::info!(session_id = %session_id, "Browser disconnected");
    let mut mgr = state.browser_sessions.write().await;
    mgr.remove_session(&session_id);
    drop(mgr);
    drop(tx); // closes the writer task
    writer.abort();
}
