/// License manager — validates license via Dodo Payments.
///
/// States: Licensed, Trial(days_left), Expired.
/// License key stored in ~/.haku/config.json alongside server config.

use crate::config::Config;
use chrono::{DateTime, Utc};
use std::fs;

#[derive(Debug, Clone)]
pub enum LicenseState {
    Licensed,
    Trial { days_left: i64 },
    Expired,
}

const TRIAL_DAYS: i64 = 14;
const ACTIVATE_URL: &str = "https://live.dodopayments.com/licenses/activate";

pub struct LicenseManager {
    config_path: std::path::PathBuf,
    license_key: Option<String>,
    first_launch: Option<DateTime<Utc>>,
}

impl LicenseManager {
    pub fn new(config: &Config) -> Self {
        let first_launch = config
            .first_launch
            .as_ref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));

        Self {
            config_path: Config::config_path(),
            license_key: config.license_key.clone(),
            first_launch,
        }
    }

    pub fn check_state(&self) -> LicenseState {
        if self.license_key.as_ref().is_some_and(|k| !k.is_empty()) {
            return LicenseState::Licensed;
        }

        let first = match self.first_launch {
            Some(dt) => dt,
            None => return LicenseState::Trial { days_left: TRIAL_DAYS },
        };

        let elapsed = Utc::now().signed_duration_since(first).num_days();
        let days_left = (TRIAL_DAYS - elapsed).max(0);

        if days_left > 0 {
            LicenseState::Trial { days_left }
        } else {
            LicenseState::Expired
        }
    }

    /// Activate a license key against Dodo Payments.
    /// Returns Ok(()) on success, Err(message) on failure.
    pub fn activate(&mut self, key: &str) -> Result<(), String> {
        let device_id = get_device_id();
        let device_name = hostname();

        let body = serde_json::json!({
            "license_key": key,
            "name": format!("{} ({})", device_name, &device_id[..8.min(device_id.len())])
        });

        let resp = ureq::post(ACTIVATE_URL)
            .header("Content-Type", "application/json")
            .send(body.to_string().as_bytes())
            .map_err(|e| format!("Network error: {e}"))?;

        let status = resp.status();
        if status == 200 || status == 201 {
            let resp_text = resp.into_body().read_to_string().unwrap_or_default();
            let resp_body: serde_json::Value = serde_json::from_str(&resp_text)
                .unwrap_or(serde_json::json!({}));
            let instance_id = resp_body["id"].as_str().unwrap_or("").to_string();

            self.license_key = Some(key.to_string());
            self.save_license(key, &instance_id);
            Ok(())
        } else {
            let text = resp.into_body().read_to_string().unwrap_or_default();
            Err(format!("Activation failed ({}): {}", status, text))
        }
    }

    /// JSON payload for sending license state over WebSocket to extension.
    pub fn to_ws_message(&self) -> serde_json::Value {
        match self.check_state() {
            LicenseState::Licensed => serde_json::json!({
                "type": "license_state",
                "state": "licensed"
            }),
            LicenseState::Trial { days_left } => serde_json::json!({
                "type": "license_state",
                "state": "trial",
                "daysLeft": days_left
            }),
            LicenseState::Expired => serde_json::json!({
                "type": "license_state",
                "state": "expired"
            }),
        }
    }

    fn save_license(&self, key: &str, instance_id: &str) {
        if let Ok(data) = fs::read_to_string(&self.config_path) {
            if let Ok(mut cfg) = serde_json::from_str::<serde_json::Value>(&data) {
                cfg["license_key"] = serde_json::json!(key);
                cfg["instance_id"] = serde_json::json!(instance_id);
                if let Ok(json) = serde_json::to_string_pretty(&cfg) {
                    let _ = fs::write(&self.config_path, json);
                }
            }
        }
    }
}

fn get_device_id() -> String {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("wmic")
            .args(["csproduct", "get", "uuid"])
            .output()
            .ok()
            .and_then(|o| {
                String::from_utf8(o.stdout).ok().map(|s| {
                    s.lines()
                        .nth(1)
                        .unwrap_or("")
                        .trim()
                        .to_string()
                })
            })
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("ioreg")
            .args(["-d2", "-c", "IOPlatformExpertDevice"])
            .output()
            .ok()
            .and_then(|o| {
                String::from_utf8(o.stdout).ok().and_then(|s| {
                    s.lines()
                        .find(|l| l.contains("IOPlatformUUID"))
                        .and_then(|l| l.split('"').nth(3))
                        .map(|s| s.to_string())
                })
            })
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
    }
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/etc/machine-id")
            .unwrap_or_else(|_| uuid::Uuid::new_v4().to_string())
            .trim()
            .to_string()
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        uuid::Uuid::new_v4().to_string()
    }
}

fn hostname() -> String {
    #[cfg(target_os = "windows")]
    {
        std::env::var("COMPUTERNAME").unwrap_or_else(|_| "Windows PC".into())
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new("hostname")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "Unknown".into())
    }
}
