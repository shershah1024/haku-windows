use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub token: String,
    pub port: u16,
    #[serde(rename = "wsPort")]
    pub ws_port: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_launch: Option<String>,
}

impl Config {
    pub fn ws_tls_port(&self) -> u16 {
        self.ws_port - 1
    }

    pub fn load_or_create() -> Self {
        let path = Self::config_path();
        if let Ok(data) = fs::read_to_string(&path) {
            if let Ok(cfg) = serde_json::from_str::<Config>(&data) {
                return cfg;
            }
        }

        let now = chrono::Utc::now().to_rfc3339();
        let cfg = Config {
            token: uuid::Uuid::new_v4().to_string().replace('-', ""),
            port: 19820,
            ws_port: 19822,
            license_key: None,
            instance_id: None,
            first_launch: Some(now),
        };
        cfg.write_internal_config();
        cfg
    }

    pub fn write_internal_config(&self) {
        let dir = Self::config_dir();
        let _ = fs::create_dir_all(&dir);

        let path = dir.join("config.json");
        if let Ok(data) = serde_json::to_string_pretty(&self) {
            let _ = fs::write(&path, data);
            tracing::info!("Wrote config to {}", path.display());
        }
    }

    pub fn config_dir() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".haku")
    }

    pub fn config_path() -> PathBuf {
        Self::config_dir().join("config.json")
    }
}
