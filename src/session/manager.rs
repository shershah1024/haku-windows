/// Session manager — tracks the target app and scanning state.
///
/// Cross-platform: uses the Platform trait for all OS calls.

use crate::flow::FlowStore;
use crate::platform::{Platform, ScanResult};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use tokio::sync::Mutex;

pub struct SessionManager {
    pub target_pid: Option<u32>,
    pub target_app_name: Option<String>,
    pub target_bundle_id: Option<String>,
    pub last_scan: Option<ScanResult>,
    previous_tools: HashMap<String, Option<String>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            target_pid: None,
            target_app_name: None,
            target_bundle_id: None,
            last_scan: None,
            previous_tools: HashMap::new(),
        }
    }

    pub async fn start_session(
        &mut self,
        platform: &Box<dyn Platform>,
        app_name: Option<&str>,
        bundle_id: Option<&str>,
        flow_store: &Mutex<FlowStore>,
    ) -> Value {
        let apps = platform.running_apps();

        let matched = if let Some(bid) = bundle_id.filter(|s| !s.is_empty()) {
            apps.iter().find(|a| a.bundle_id == bid)
        } else if let Some(name) = app_name.filter(|s| !s.is_empty()) {
            let lower = name.to_lowercase();
            apps.iter()
                .find(|a| a.name.to_lowercase() == lower)
                .or_else(|| apps.iter().find(|a| a.name.to_lowercase().contains(&lower)))
        } else {
            return json!({"error": "provide app_name or bundle_id"});
        };

        let app = match matched {
            Some(a) => a.clone(),
            None => {
                let available: Vec<&str> = apps.iter().map(|a| a.name.as_str()).collect();
                return json!({"error": format!("app not found. Running apps: {}", available.join(", "))});
            }
        };

        self.target_pid = Some(app.pid);
        self.target_app_name = Some(app.name.clone());
        self.target_bundle_id = Some(app.bundle_id.clone());

        let _ = platform.activate_app(app.pid);
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        let scan = match platform.scan(app.pid) {
            Ok(s) => s,
            Err(e) => return json!({"error": format!("scan failed: {e}")}),
        };

        tracing::info!(
            app = app.name,
            pid = app.pid,
            elements = scan.elements.len(),
            "Session started"
        );

        {
            let mut flow = flow_store.lock().await;
            flow.begin_recording(&app.name, Some(&app.bundle_id), None);
        }

        let response = self.build_state_response(&scan, &app.name, &app.bundle_id);
        self.last_scan = Some(scan);
        response
    }

    pub async fn end_session(
        &mut self,
        save_name: Option<&str>,
        flow_store: &Mutex<FlowStore>,
    ) -> Value {
        let name = self.target_app_name.clone().unwrap_or("none".into());
        let flow_result = {
            let mut flow = flow_store.lock().await;
            flow.end_recording(save_name)
        };

        self.target_pid = None;
        self.target_app_name = None;
        self.target_bundle_id = None;
        self.last_scan = None;
        self.previous_tools.clear();

        json!({"status": "ended", "app": name, "flow": flow_result})
    }

    pub fn get_state(&mut self, platform: &Box<dyn Platform>) -> Value {
        let pid = match self.target_pid {
            Some(p) => p,
            None => return json!({"error": "no active session. Call session_start first."}),
        };
        let scan = match platform.scan(pid) {
            Ok(s) => s,
            Err(e) => return json!({"error": format!("scan failed: {e}")}),
        };
        let name = self.target_app_name.clone().unwrap_or_default();
        let bid = self.target_bundle_id.clone().unwrap_or_default();
        let response = self.build_state_response(&scan, &name, &bid);
        self.last_scan = Some(scan);
        response
    }

    pub fn get_state_diff(&mut self, platform: &Box<dyn Platform>) -> Value {
        let pid = match self.target_pid {
            Some(p) => p,
            None => return json!({"error": "no active session"}),
        };
        let scan = match platform.scan(pid) {
            Ok(s) => s,
            Err(e) => return json!({"error": format!("scan failed: {e}")}),
        };
        let response = self.build_diff_response(&scan);
        self.last_scan = Some(scan);
        response
    }

    pub async fn perform_action(
        &mut self,
        platform: &Box<dyn Platform>,
        tool_name: &str,
        args: &Value,
        flow_store: &Mutex<FlowStore>,
    ) -> Value {
        let pid = match self.target_pid {
            Some(p) => p,
            None => return json!({"error": "no active session"}),
        };

        let scan = match &self.last_scan {
            Some(s) => s,
            None => return json!({"error": "no scan available. Call get_page_info first."}),
        };

        let element = match scan.elements.iter().find(|e| e.tool_name == tool_name) {
            Some(e) => e,
            None => return json!({"error": format!("element '{tool_name}' not found. Call get_page_info to refresh.")}),
        };

        let result = match element.tool_prefix.as_str() {
            "click" | "select" | "toggle" | "open" => {
                platform.perform_action(pid, &element.handle, "press")
            }
            "fill" => {
                let value = match args["value"].as_str() {
                    Some(v) => v,
                    None => return json!({"error": "missing 'value' argument"}),
                };
                platform.set_value(pid, &element.handle, value)
            }
            "set" => {
                let value = args["value"].to_string();
                platform.set_value(pid, &element.handle, &value)
            }
            other => Err(format!("unknown action prefix: {other}")),
        };

        if let Err(e) = result {
            return json!({"error": e});
        }

        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        let new_scan = match platform.scan(pid) {
            Ok(s) => s,
            Err(e) => return json!({"error": format!("re-scan failed: {e}")}),
        };

        {
            let mut flow = flow_store.lock().await;
            flow.record_step(
                tool_name,
                args,
                Some(&new_scan.window_title),
                Some(new_scan.elements.len() as i32),
                None,
                None,
            );
        }

        let response = self.build_diff_response(&new_scan);
        self.last_scan = Some(new_scan);
        response
    }

    fn build_state_response(&mut self, scan: &ScanResult, app_name: &str, bundle_id: &str) -> Value {
        self.previous_tools.clear();
        for e in &scan.elements {
            self.previous_tools.insert(e.tool_name.clone(), e.value.clone());
        }

        let elements: Vec<Value> = scan
            .elements
            .iter()
            .map(|e| {
                let mut d = json!({
                    "tool": e.tool_name,
                    "role": e.role.to_lowercase(),
                    "title": e.title,
                    "enabled": e.enabled,
                });
                if let Some(ref v) = e.value {
                    d["value"] = json!(v);
                }
                d
            })
            .collect();

        json!({
            "app": app_name,
            "bundle_id": bundle_id,
            "window_title": scan.window_title,
            "focused_element": scan.focused_element,
            "element_count": scan.elements.len(),
            "interactive_elements": elements,
            "available_tools": scan.elements.iter().map(|e| &e.tool_name).collect::<Vec<_>>(),
            "hierarchy_summary": scan.hierarchy_summary,
        })
    }

    fn build_diff_response(&mut self, scan: &ScanResult) -> Value {
        let current: HashMap<String, Option<String>> = scan
            .elements
            .iter()
            .map(|e| (e.tool_name.clone(), e.value.clone()))
            .collect();

        let old_keys: HashSet<String> = self.previous_tools.keys().cloned().collect();
        let new_keys: HashSet<String> = current.keys().cloned().collect();

        let added_names: HashSet<&String> = new_keys.difference(&old_keys).collect();
        let removed: Vec<&String> = old_keys.difference(&new_keys).collect();

        let added: Vec<Value> = scan
            .elements
            .iter()
            .filter(|e| added_names.contains(&e.tool_name))
            .map(|e| {
                let mut d = json!({
                    "tool": e.tool_name,
                    "role": e.role.to_lowercase(),
                    "title": e.title,
                    "enabled": e.enabled,
                });
                if let Some(ref v) = e.value {
                    d["value"] = json!(v);
                }
                d
            })
            .collect();

        let mut changed: Vec<Value> = Vec::new();
        for name in old_keys.intersection(&new_keys) {
            let old_val = self.previous_tools.get(name).and_then(|v| v.as_deref());
            let new_val = current.get(name).and_then(|v| v.as_deref());
            if old_val != new_val {
                changed.push(json!({
                    "tool": name,
                    "old_value": old_val.unwrap_or(""),
                    "new_value": new_val.unwrap_or(""),
                }));
            }
        }

        let nothing_changed = added.is_empty() && removed.is_empty() && changed.is_empty();
        self.previous_tools = current;

        let mut result = json!({
            "window_title": scan.window_title,
            "focused_element": scan.focused_element,
            "element_count": scan.elements.len(),
            "available_tools": scan.elements.iter().map(|e| &e.tool_name).collect::<Vec<_>>(),
        });

        if nothing_changed {
            result["diff"] = json!("no changes");
        } else {
            if !added.is_empty() { result["added"] = json!(added); }
            if !removed.is_empty() { result["removed"] = json!(removed); }
            if !changed.is_empty() { result["changed"] = json!(changed); }
        }

        result
    }
}
