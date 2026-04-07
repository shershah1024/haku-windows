/// Flow store — SQLite storage for recorded UI flows.
///
/// Cross-platform: uses rusqlite.

use rusqlite::{Connection, params};
use serde_json::{json, Value};
use std::path::PathBuf;

pub struct FlowStore {
    db: Connection,
    current_flow_id: Option<i64>,
    step_counter: i32,
}

impl FlowStore {
    pub fn open() -> Result<Self, String> {
        let dir = Self::db_dir();
        std::fs::create_dir_all(&dir).map_err(|e| format!("failed to create dir: {e}"))?;
        let path = dir.join("flows.db");

        let db = Connection::open(&path).map_err(|e| format!("failed to open db: {e}"))?;

        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| format!("pragma failed: {e}"))?;

        let mut store = Self {
            db,
            current_flow_id: None,
            step_counter: 0,
        };
        store.create_tables();
        Ok(store)
    }

    fn db_dir() -> PathBuf {
        #[cfg(target_os = "macos")]
        {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("Library/Application Support/Haku")
        }
        #[cfg(target_os = "windows")]
        {
            dirs::data_local_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("Haku")
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("haku")
        }
    }

    fn create_tables(&mut self) {
        let sqls = [
            "CREATE TABLE IF NOT EXISTS flows (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                app_name TEXT NOT NULL,
                bundle_id TEXT,
                description TEXT,
                created_at TEXT DEFAULT (datetime('now')),
                updated_at TEXT DEFAULT (datetime('now')),
                run_count INTEGER DEFAULT 0,
                version INTEGER DEFAULT 1,
                target_url TEXT
            )",
            "CREATE TABLE IF NOT EXISTS flow_steps (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                flow_id INTEGER NOT NULL REFERENCES flows(id) ON DELETE CASCADE,
                step_number INTEGER NOT NULL,
                tool_name TEXT NOT NULL,
                args_json TEXT,
                window_title TEXT,
                element_count INTEGER,
                wait_ms INTEGER DEFAULT 300,
                narration TEXT,
                page_url TEXT,
                context TEXT,
                created_at TEXT DEFAULT (datetime('now'))
            )",
            "CREATE INDEX IF NOT EXISTS idx_flow_steps_flow ON flow_steps(flow_id, step_number)",
            "CREATE INDEX IF NOT EXISTS idx_flows_name ON flows(name)",
        ];

        for sql in &sqls {
            let _ = self.db.execute_batch(sql);
        }

        let migrations = [
            "ALTER TABLE flows ADD COLUMN version INTEGER DEFAULT 1",
            "ALTER TABLE flows ADD COLUMN target_url TEXT",
            "ALTER TABLE flow_steps ADD COLUMN page_url TEXT",
            "ALTER TABLE flow_steps ADD COLUMN context TEXT",
        ];
        for sql in &migrations {
            let _ = self.db.execute_batch(sql);
        }
    }

    // ── Recording ──

    pub fn begin_recording(&mut self, app_name: &str, bundle_id: Option<&str>, target_url: Option<&str>) {
        self.step_counter = 0;

        let result = self.db.execute(
            "INSERT INTO flows (name, app_name, bundle_id, description, target_url) VALUES (?1, ?2, ?3, ?4, ?5)",
            params!["__recording__", app_name, bundle_id, "Recording in progress", target_url],
        );

        if result.is_ok() {
            self.current_flow_id = Some(self.db.last_insert_rowid());
            tracing::info!(flow_id = self.current_flow_id, app = app_name, "Begin recording");
        }
    }

    pub fn record_step(
        &mut self,
        tool_name: &str,
        args: &Value,
        window_title: Option<&str>,
        element_count: Option<i32>,
        page_url: Option<&str>,
        context: Option<&str>,
    ) {
        let flow_id = match self.current_flow_id {
            Some(id) => id,
            None => return,
        };
        self.step_counter += 1;

        let args_json = if args.is_object() && !args.as_object().unwrap().is_empty() {
            Some(args.to_string())
        } else {
            None
        };

        let _ = self.db.execute(
            "INSERT INTO flow_steps (flow_id, step_number, tool_name, args_json, window_title, element_count, page_url, context)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![flow_id, self.step_counter, tool_name, args_json, window_title, element_count, page_url, context],
        );
    }

    pub fn end_recording(&mut self, save_name: Option<&str>) -> Value {
        let flow_id = match self.current_flow_id {
            Some(id) => id,
            None => return json!({"error": "not recording"}),
        };

        let steps = self.get_steps(flow_id);

        if let Some(name) = save_name.filter(|s| !s.is_empty()) {
            let version: i32 = self.db
                .query_row(
                    "SELECT COALESCE(MAX(version), 0) FROM flows WHERE name = ?1 AND name != '__recording__'",
                    params![name],
                    |row| row.get(0),
                )
                .unwrap_or(0)
                + 1;

            let _ = self.db.execute(
                "UPDATE flows SET name = ?1, description = ?2, version = ?3, updated_at = datetime('now') WHERE id = ?4",
                params![name, format!("Flow with {} steps (v{})", steps.len(), version), version, flow_id],
            );
            tracing::info!(name, version, steps = steps.len(), "Saved flow");
        } else {
            let _ = self.db.execute("DELETE FROM flows WHERE id = ?1", params![flow_id]);
        }

        self.current_flow_id = None;
        self.step_counter = 0;

        json!({
            "step_count": steps.len(),
            "steps": steps,
            "saved_as": save_name.unwrap_or("(not saved)"),
        })
    }

    // ── Queries ──

    pub fn list_flows(&self) -> Vec<Value> {
        let mut stmt = self.db.prepare(
            "SELECT id, name, app_name, bundle_id, description, created_at, run_count,
                    (SELECT COUNT(*) FROM flow_steps WHERE flow_id = flows.id) as step_count,
                    COALESCE(version, 1) as version, target_url
             FROM flows WHERE name != '__recording__' ORDER BY updated_at DESC"
        ).unwrap();

        stmt.query_map([], |row| {
            let id: i64 = row.get(0)?;
            let name: String = row.get(1)?;
            let app_name: String = row.get(2)?;
            let bundle_id: Option<String> = row.get(3)?;
            let description: Option<String> = row.get(4)?;
            let created_at: Option<String> = row.get(5)?;
            let run_count: i32 = row.get(6)?;
            let step_count: i32 = row.get(7)?;
            let version: i32 = row.get(8)?;

            let mut flow = json!({
                "id": id,
                "name": name,
                "app_name": app_name,
                "step_count": step_count,
                "run_count": run_count,
                "version": version,
            });
            if let Some(bid) = bundle_id { flow["bundle_id"] = json!(bid); }
            if let Some(desc) = description { flow["description"] = json!(desc); }
            if let Some(ca) = created_at { flow["created_at"] = json!(ca); }
            Ok(flow)
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    pub fn load_flow(&mut self, name: &str) -> Value {
        let row = self.db.query_row(
            "SELECT id, app_name, bundle_id, version, target_url FROM flows WHERE name = ?1 ORDER BY version DESC LIMIT 1",
            params![name],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, i32>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        );

        match row {
            Ok((flow_id, app_name, bundle_id, version, target_url)) => {
                let _ = self.db.execute(
                    "UPDATE flows SET run_count = run_count + 1, updated_at = datetime('now') WHERE id = ?1",
                    params![flow_id],
                );

                let steps = self.get_steps(flow_id);
                let mut result = json!({
                    "name": name,
                    "app_name": app_name,
                    "version": version,
                    "step_count": steps.len(),
                    "steps": steps,
                });
                if let Some(bid) = bundle_id { result["bundle_id"] = json!(bid); }
                if let Some(url) = target_url { result["target_url"] = json!(url); }
                result
            }
            Err(_) => json!({"error": format!("flow '{name}' not found")}),
        }
    }

    pub fn search_flows(&self, query: &str) -> Vec<Value> {
        let pattern = format!("%{query}%");
        let mut stmt = self.db.prepare(
            "SELECT DISTINCT f.id, f.name, f.app_name, f.bundle_id, f.description,
                    f.created_at, f.run_count,
                    (SELECT COUNT(*) FROM flow_steps WHERE flow_id = f.id) as step_count,
                    COALESCE(f.version, 1) as version, f.target_url
             FROM flows f
             LEFT JOIN flow_steps fs ON fs.flow_id = f.id
             WHERE f.name != '__recording__'
               AND (f.name LIKE ?1 OR f.app_name LIKE ?2 OR f.target_url LIKE ?3
                    OR fs.tool_name LIKE ?4 OR fs.context LIKE ?5)
             ORDER BY f.updated_at DESC LIMIT 10"
        ).unwrap();

        stmt.query_map(params![&pattern, &pattern, &pattern, &pattern, &pattern], |row| {
            let flow_id: i64 = row.get(0)?;
            let name: String = row.get(1)?;
            let app_name: String = row.get(2)?;
            let step_count: i32 = row.get(7)?;
            let run_count: i32 = row.get(6)?;
            let version: i32 = row.get(8)?;

            Ok((flow_id, json!({
                "name": name,
                "app_name": app_name,
                "step_count": step_count,
                "run_count": run_count,
                "version": version,
            })))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .map(|(flow_id, mut flow)| {
            flow["steps"] = json!(self.get_steps(flow_id));
            flow
        })
        .collect()
    }

    pub fn delete_flow(&mut self, name: &str) -> Value {
        let flow_id = self.db.query_row(
            "SELECT id FROM flows WHERE name = ?1",
            params![name],
            |row| row.get::<_, i64>(0),
        );

        match flow_id {
            Ok(id) => {
                let _ = self.db.execute("DELETE FROM flow_steps WHERE flow_id = ?1", params![id]);
                let _ = self.db.execute("DELETE FROM flows WHERE id = ?1", params![id]);
                json!({"status": "deleted", "name": name})
            }
            Err(_) => json!({"error": format!("flow '{name}' not found")}),
        }
    }

    fn get_steps(&self, flow_id: i64) -> Vec<Value> {
        let mut stmt = self.db.prepare(
            "SELECT step_number, tool_name, args_json, window_title, element_count, wait_ms, narration, page_url, context
             FROM flow_steps WHERE flow_id = ?1 ORDER BY step_number"
        ).unwrap();

        stmt.query_map(params![flow_id], |row| {
            let mut step = json!({
                "step": row.get::<_, i32>(0)?,
                "tool": row.get::<_, String>(1)?,
            });
            if let Ok(Some(args_str)) = row.get::<_, Option<String>>(2) {
                if let Ok(parsed) = serde_json::from_str::<Value>(&args_str) {
                    step["args"] = parsed;
                }
            }
            if let Ok(Some(wt)) = row.get::<_, Option<String>>(3) { step["window_title"] = json!(wt); }
            if let Ok(Some(ec)) = row.get::<_, Option<i32>>(4) { step["element_count"] = json!(ec); }
            if let Ok(Some(wm)) = row.get::<_, Option<i32>>(5) { step["wait_ms"] = json!(wm); }
            if let Ok(Some(n)) = row.get::<_, Option<String>>(6) { step["narration"] = json!(n); }
            if let Ok(Some(u)) = row.get::<_, Option<String>>(7) { step["page_url"] = json!(u); }
            if let Ok(Some(c)) = row.get::<_, Option<String>>(8) { step["context"] = json!(c); }
            Ok(step)
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }
}
