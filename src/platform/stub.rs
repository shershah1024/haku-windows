/// Stub platform for development builds on macOS/Linux.
/// All methods return errors — native automation only works on Windows.
/// The server still runs (WebSocket extension bridge works everywhere).

use super::{AppInfo, ElementHandle, Platform, ScanResult};

pub struct StubPlatform;

impl Platform for StubPlatform {
    fn check_accessibility_permission(&self) -> bool {
        true // No permission needed — stub
    }

    fn scan(&self, _pid: u32) -> Result<ScanResult, String> {
        Err("Native app scanning not available on this platform. Use the Chrome extension for browser automation.".into())
    }

    fn perform_action(&self, _pid: u32, _handle: &ElementHandle, _action: &str) -> Result<(), String> {
        Err("Native automation not available on this platform".into())
    }

    fn set_value(&self, _pid: u32, _handle: &ElementHandle, _value: &str) -> Result<(), String> {
        Err("Native automation not available on this platform".into())
    }

    fn running_apps(&self) -> Vec<AppInfo> {
        vec![]
    }

    fn activate_app(&self, _pid: u32) -> Result<(), String> {
        Err("Native automation not available on this platform".into())
    }

    fn type_text(&self, _text: &str, _pid: u32) -> Result<(), String> {
        Err("Native automation not available on this platform".into())
    }

    fn press_key(&self, _combo: &str, _pid: u32) -> Result<(), String> {
        Err("Native automation not available on this platform".into())
    }

    fn screenshot(&self, _pid: u32) -> Result<Vec<u8>, String> {
        Err("Native automation not available on this platform".into())
    }
}
