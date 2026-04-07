/// MCP JSON-RPC tool router.
///
/// Dispatches tools/list and tools/call — cross-platform.

use crate::AppState;
use serde_json::{json, Value};
use std::sync::Arc;

pub async fn handle_request(state: &Arc<AppState>, request: &str) -> Option<String> {
    let req: Value = serde_json::from_str(request).ok()?;
    let id = req.get("id").cloned();
    let method = req["method"].as_str().unwrap_or("");

    match method {
        "initialize" => Some(json_rpc(
            id,
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {"tools": {"listChanged": true}},
                "serverInfo": {"name": "haku", "version": "0.1.0"},
            }),
        )),

        "notifications/initialized" => None,

        "tools/list" => {
            let tools = all_tools(state).await;
            Some(json_rpc(id, json!({"tools": tools})))
        }

        "tools/call" => {
            let params = &req["params"];
            let tool_name = params["name"].as_str().unwrap_or("");
            let args = params["arguments"].as_object().cloned().unwrap_or_default();
            let result = call_tool(state, tool_name, &Value::Object(args)).await;
            Some(tool_response(id, &result))
        }

        _ => Some(error_response(id, -32601, "Method not found")),
    }
}

async fn all_tools(state: &Arc<AppState>) -> Vec<Value> {
    let mut tools = static_tools();

    // Dynamic tools from last native scan
    let session = state.session_manager.read().await;
    if let Some(ref scan) = session.last_scan {
        for elem in &scan.elements {
            let mut schema = json!({"type": "object", "properties": {}});
            match elem.tool_prefix.as_str() {
                "fill" => {
                    schema["properties"] = json!({"value": {"type": "string", "description": "Text to fill in"}});
                    schema["required"] = json!(["value"]);
                }
                "set" => {
                    schema["properties"] = json!({"value": {"type": "number", "description": "Value to set"}});
                    schema["required"] = json!(["value"]);
                }
                _ => {}
            }
            tools.push(json!({
                "name": elem.tool_name,
                "description": elem.description,
                "inputSchema": schema,
            }));
        }
    }

    // Browser tools from WebSocket extension sessions
    let browser = state.browser_sessions.read().await;
    tools.extend(browser.all_browser_tools());
    tools.extend(browser.all_web_tools());

    tools
}

fn static_tools() -> Vec<Value> {
    vec![
        tool("session_start",
             "Start controlling an app. Activates it, scans its UI, and returns all interactive elements as available tools.",
             json!({"app_name": {"type": "string", "description": "App name (e.g., 'Notepad', 'Calculator')"},
                    "bundle_id": {"type": "string", "description": "Executable path or identifier. More precise."}}),
             &[]),
        tool("session_end",
             "Stop controlling the current target app. If save_as is provided, saves the recorded flow for replay later.",
             json!({"save_as": {"type": "string", "description": "Name to save this flow as (optional)."}}),
             &[]),
        tool("get_page_info",
             "Re-scan the target app's UI. Returns window title, focused element, all interactive elements.",
             json!({}), &[]),
        tool("list_apps", "List all running GUI applications with name and PID.", json!({}), &[]),
        tool("screenshot", "Capture the target app's frontmost window as a PNG image.", json!({}), &[]),
        tool("type_text",
             "Type text character-by-character into the focused field.",
             json!({"text": {"type": "string", "description": "Text to type"}}),
             &["text"]),
        tool("press_key",
             "Send a keyboard shortcut (e.g., 'ctrl+s', 'return', 'tab', 'escape'). Use 'ctrl' not 'cmd' on Windows.",
             json!({"key": {"type": "string", "description": "Key combo: 'ctrl+s', 'ctrl+shift+n', 'return', 'tab', 'escape'"}}),
             &["key"]),
        tool("activate_app", "Bring the target app to the foreground.", json!({}), &[]),
        tool("search_flows",
             "Search for previously recorded flows.",
             json!({"query": {"type": "string", "description": "Search query"}}),
             &["query"]),
        tool("list_flows", "List all saved flows.", json!({}), &[]),
        tool("load_flow",
             "Load a saved flow by name. Returns the steps so you can replay them.",
             json!({"name": {"type": "string", "description": "Flow name"}}),
             &["name"]),
        tool("delete_flow",
             "Delete a saved flow by name.",
             json!({"name": {"type": "string", "description": "Flow name"}}),
             &["name"]),
        tool("search_available_tools",
             "Find the most relevant tools for a given intent. Use when there are many tools (e.g., 100+ from a web page scan) and you need to surface the ones matching what you want to do. Returns top-K tool names ranked by semantic similarity.",
             json!({"query": {"type": "string", "description": "What you want to do (e.g., 'submit the form', 'enter email')"},
                    "top_k": {"type": "integer", "description": "Max results to return (default 7)"}}),
             &["query"]),
    ]
}

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    let mut schema = json!({"type": "object", "properties": properties});
    if !required.is_empty() {
        schema["required"] = json!(required);
    }
    json!({"name": name, "description": description, "inputSchema": schema})
}

async fn call_tool(state: &Arc<AppState>, name: &str, args: &Value) -> Value {
    match name {
        "session_start" => {
            let app_name = args["app_name"].as_str();
            let bundle_id = args["bundle_id"].as_str();
            let mut session = state.session_manager.write().await;
            session.start_session(&state.platform, app_name, bundle_id, &state.flow_store).await
        }

        "session_end" => {
            let save_name = args["save_as"].as_str();
            let mut session = state.session_manager.write().await;
            session.end_session(save_name, &state.flow_store).await
        }

        "get_page_info" => {
            let mut session = state.session_manager.write().await;
            session.get_state(&state.platform)
        }

        "list_apps" => {
            let apps = state.platform.running_apps();
            json!({"apps": apps})
        }

        "screenshot" => {
            let pid = {
                let s = state.session_manager.read().await;
                match s.target_pid { Some(p) => p, None => return json!({"error": "no active session"}) }
            };
            match state.platform.screenshot(pid) {
                Ok(data) => json!({"__image__": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &data)}),
                Err(e) => json!({"error": e}),
            }
        }

        "type_text" => {
            let text = match args["text"].as_str() {
                Some(t) => t,
                None => return json!({"error": "missing 'text'"}),
            };
            let pid = {
                let s = state.session_manager.read().await;
                match s.target_pid { Some(p) => p, None => return json!({"error": "no active session"}) }
            };
            if let Err(e) = state.platform.type_text(text, pid) {
                return json!({"error": e});
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            state.flow_store.lock().await.record_step("type_text", args, None, None, None, None);
            state.session_manager.write().await.get_state_diff(&state.platform)
        }

        "press_key" => {
            let key = match args["key"].as_str() {
                Some(k) => k,
                None => return json!({"error": "missing 'key'"}),
            };
            let pid = {
                let s = state.session_manager.read().await;
                match s.target_pid { Some(p) => p, None => return json!({"error": "no active session"}) }
            };
            if let Err(e) = state.platform.press_key(key, pid) {
                return json!({"error": e});
            }
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            state.flow_store.lock().await.record_step("press_key", args, None, None, None, None);
            state.session_manager.write().await.get_state_diff(&state.platform)
        }

        "activate_app" => {
            let pid = {
                let s = state.session_manager.read().await;
                match s.target_pid { Some(p) => p, None => return json!({"error": "no active session"}) }
            };
            if let Err(e) = state.platform.activate_app(pid) {
                return json!({"error": e});
            }
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            state.session_manager.write().await.get_state_diff(&state.platform)
        }

        "search_flows" => {
            let query = match args["query"].as_str() {
                Some(q) => q,
                None => return json!({"error": "missing 'query'"}),
            };
            let flow = state.flow_store.lock().await;
            let results = flow.search_flows(query);
            if results.is_empty() {
                json!({"matches": 0, "message": format!("No saved flows match '{query}'.")})
            } else {
                json!({"matches": results.len(), "flows": results})
            }
        }

        "list_flows" => {
            let flow = state.flow_store.lock().await;
            json!({"flows": flow.list_flows()})
        }

        "load_flow" => {
            let name = match args["name"].as_str() {
                Some(n) => n,
                None => return json!({"error": "missing 'name'"}),
            };
            let mut flow = state.flow_store.lock().await;
            flow.load_flow(name)
        }

        "delete_flow" => {
            let name = match args["name"].as_str() {
                Some(n) => n,
                None => return json!({"error": "missing 'name'"}),
            };
            let mut flow = state.flow_store.lock().await;
            flow.delete_flow(name)
        }

        "search_available_tools" => {
            let query = match args["query"].as_str() {
                Some(q) => q,
                None => return json!({"error": "missing 'query'"}),
            };
            let top_k = args["top_k"].as_u64().unwrap_or(7) as usize;
            search_tools(state, query, top_k).await
        }

        _ => {
            // Dynamic tool — check browser tools first, then native
            let has_browser = {
                let browser = state.browser_sessions.read().await;
                browser.has_browser_tool(name)
            };

            if has_browser {
                return dispatch_browser_tool(state, name, args).await;
            }

            let mut session = state.session_manager.write().await;
            session.perform_action(&state.platform, name, args, &state.flow_store).await
        }
    }
}

// ── Browser tool dispatch ──

const BROWSER_TOOL_TIMEOUT_SECS: u64 = 30;

/// Send a tool call to the Chrome extension via WebSocket and wait for the result.
async fn dispatch_browser_tool(state: &Arc<AppState>, name: &str, args: &Value) -> Value {
    let (call_id, receiver) = {
        let mut mgr = state.browser_sessions.write().await;
        match mgr.dispatch_tool_call(name, args) {
            Ok(pair) => pair,
            Err(e) => return json!({"error": e}),
        }
    };

    match tokio::time::timeout(
        std::time::Duration::from_secs(BROWSER_TOOL_TIMEOUT_SECS),
        receiver,
    )
    .await
    {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => {
            // oneshot sender dropped (session disconnected)
            state.browser_sessions.write().await.cancel_pending(&call_id);
            json!({"error": format!("extension disconnected before responding to '{name}'")})
        }
        Err(_) => {
            state.browser_sessions.write().await.cancel_pending(&call_id);
            json!({"error": format!("timeout waiting for extension to respond to '{name}'")})
        }
    }
}

// ── Tool search ──

/// Find the most relevant tools for a query.
///
/// With `embedding` feature: semantic ranking via EmbeddingGemma.
/// Without: substring match fallback (still useful for small tool sets).
async fn search_tools(state: &Arc<AppState>, query: &str, top_k: usize) -> Value {
    // Collect all available tools (static + dynamic + browser-registered)
    let all = all_tools(state).await;
    let mut candidates: Vec<(String, String)> = Vec::new();
    for t in &all {
        if let (Some(name), Some(desc)) = (t["name"].as_str(), t["description"].as_str()) {
            candidates.push((name.to_string(), desc.to_string()));
        }
    }

    #[cfg(feature = "embedding")]
    {
        if let Some(ref engine) = state.embedding {
            // Ensure all candidates have embeddings cached
            engine.cache_tool_embeddings(&candidates);
            let ranked = engine.rank_tools(query, top_k);
            if !ranked.is_empty() {
                let results: Vec<Value> = ranked
                    .into_iter()
                    .filter_map(|(name, score)| {
                        let desc = candidates
                            .iter()
                            .find(|(n, _)| n == &name)
                            .map(|(_, d)| d.clone())
                            .unwrap_or_default();
                        Some(json!({
                            "name": name,
                            "description": desc,
                            "score": score,
                        }))
                    })
                    .collect();
                return json!({
                    "query": query,
                    "method": "semantic",
                    "matches": results,
                });
            }
        }
    }

    // Substring fallback
    let q_lower = query.to_lowercase();
    let mut scored: Vec<(String, String, f32)> = candidates
        .into_iter()
        .map(|(name, desc)| {
            let name_score = if name.to_lowercase().contains(&q_lower) { 2.0 } else { 0.0 };
            let desc_score = if desc.to_lowercase().contains(&q_lower) { 1.0 } else { 0.0 };
            // Token overlap bonus
            let overlap: f32 = q_lower
                .split_whitespace()
                .filter(|word| {
                    name.to_lowercase().contains(word) || desc.to_lowercase().contains(word)
                })
                .count() as f32
                * 0.5;
            (name, desc, name_score + desc_score + overlap)
        })
        .collect();
    scored.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(top_k);

    let results: Vec<Value> = scored
        .into_iter()
        .filter(|(_, _, score)| *score > 0.0)
        .map(|(name, desc, score)| json!({"name": name, "description": desc, "score": score}))
        .collect();

    json!({
        "query": query,
        "method": "substring",
        "matches": results,
    })
}

// ── JSON-RPC helpers ──

fn json_rpc(id: Option<Value>, result: Value) -> String {
    json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string()
}

fn tool_response(id: Option<Value>, result: &Value) -> String {
    if let Some(image_data) = result["__image__"].as_str() {
        return json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "content": [{"type": "image", "data": image_data, "mimeType": "image/png"}]
            }
        })
        .to_string();
    }

    if let Some(error) = result["error"].as_str() {
        return json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "content": [{"type": "text", "text": format!("Error: {error}")}],
                "isError": true,
            }
        })
        .to_string();
    }

    // If the result already has MCP-shaped content (e.g., from extension tool_result),
    // pass it through instead of re-wrapping. This avoids nested content escapes.
    if result.get("content").and_then(|c| c.as_array()).is_some() {
        return json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        })
        .to_string();
    }

    let text = result.to_string();
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "content": [{"type": "text", "text": text}]
        }
    })
    .to_string()
}

fn error_response(id: Option<Value>, code: i32, message: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {"code": code, "message": message},
    })
    .to_string()
}
