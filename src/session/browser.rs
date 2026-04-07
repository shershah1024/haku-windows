/// Browser session management — tracks WebSocket-connected browser tabs
/// and DOM-scanned web tools.
///
/// Cross-platform.

use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct WebTool {
    pub name: String,
    pub description: String,
    pub selector: String,
    pub action_type: String,
    pub url_pattern: Option<String>,
}

#[derive(Debug, Clone)]
struct BrowserTool {
    name: String,
    description: String,
    input_schema: Value,
    session_id: String,
}

struct BrowserSession {
    session_id: String,
    tools: HashMap<String, BrowserTool>,
    is_ready: bool,
}

pub struct BrowserSessionManager {
    sessions: HashMap<String, BrowserSession>,
    web_tools: HashMap<String, WebTool>,
    pending_results: HashMap<String, tokio::sync::oneshot::Sender<Value>>,
}

impl BrowserSessionManager {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            web_tools: HashMap::new(),
            pending_results: HashMap::new(),
        }
    }

    pub fn create_session(&mut self, session_id: &str) {
        self.sessions.insert(
            session_id.to_string(),
            BrowserSession {
                session_id: session_id.to_string(),
                tools: HashMap::new(),
                is_ready: false,
            },
        );
    }

    pub fn remove_session(&mut self, session_id: &str) {
        self.sessions.remove(session_id);
    }

    pub fn mark_ready(&mut self, session_id: &str) {
        if let Some(s) = self.sessions.get_mut(session_id) {
            s.is_ready = true;
        }
    }

    pub fn register_browser_tool(
        &mut self,
        session_id: &str,
        name: &str,
        description: &str,
        input_schema: &Value,
    ) {
        if let Some(session) = self.sessions.get_mut(session_id) {
            session.tools.insert(
                name.to_string(),
                BrowserTool {
                    name: name.to_string(),
                    description: description.to_string(),
                    input_schema: input_schema.clone(),
                    session_id: session_id.to_string(),
                },
            );
        }
    }

    pub fn unregister_browser_tool(&mut self, session_id: &str, name: &str) {
        if let Some(session) = self.sessions.get_mut(session_id) {
            session.tools.remove(name);
        }
    }

    pub fn all_browser_tools(&self) -> Vec<Value> {
        let mut tools = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for session in self.sessions.values().filter(|s| s.is_ready) {
            for (name, tool) in &session.tools {
                if seen.insert(name.clone()) {
                    tools.push(json!({
                        "name": name,
                        "description": tool.description,
                        "inputSchema": tool.input_schema,
                        "defer_loading": true,
                    }));
                }
            }
        }
        tools
    }

    pub fn has_browser_tool(&self, name: &str) -> bool {
        self.sessions.values().any(|s| s.tools.contains_key(name))
    }

    pub fn register_web_tool(
        &mut self,
        name: &str,
        description: &str,
        selector: &str,
        action_type: &str,
        url_pattern: Option<&str>,
    ) {
        self.web_tools.insert(
            name.to_string(),
            WebTool {
                name: name.to_string(),
                description: description.to_string(),
                selector: selector.to_string(),
                action_type: action_type.to_string(),
                url_pattern: url_pattern.map(|s| s.to_string()),
            },
        );
    }

    pub fn get_web_tool(&self, name: &str) -> Option<&WebTool> {
        self.web_tools.get(name)
    }

    pub fn clear_web_tools(&mut self) {
        self.web_tools.clear();
    }

    pub fn all_web_tools(&self) -> Vec<Value> {
        self.web_tools
            .values()
            .map(|t| {
                let mut schema = json!({"type": "object", "properties": {}});
                match t.action_type.as_str() {
                    "fill" => {
                        schema["properties"] =
                            json!({"value": {"type": "string", "description": "Text to fill in"}});
                        schema["required"] = json!(["value"]);
                    }
                    "select" => {
                        schema["properties"] =
                            json!({"value": {"type": "string", "description": "Value to select"}});
                        schema["required"] = json!(["value"]);
                    }
                    _ => {}
                }
                json!({
                    "name": t.name,
                    "description": t.description,
                    "inputSchema": schema,
                    "defer_loading": true,
                })
            })
            .collect()
    }

    pub fn handle_tool_result(&mut self, call_id: &str, result: &Value) {
        if let Some(sender) = self.pending_results.remove(call_id) {
            let _ = sender.send(result.clone());
        }
    }

    pub fn handle_tool_error(&mut self, call_id: &str, error: &str) {
        if let Some(sender) = self.pending_results.remove(call_id) {
            let _ = sender.send(json!({"error": error}));
        }
    }
}
