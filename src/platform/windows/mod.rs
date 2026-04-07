/// Windows platform implementation.
///
/// Uses Win32 APIs: EnumWindows, EnumChildWindows, SendMessage, SendInput.
/// No UIA/accessibility tree — direct window message approach.

mod accessibility;
mod apps;
mod capture;
mod input;
mod tts;

use super::{AppInfo, ElementHandle, Platform, ScanResult};

pub struct WindowsPlatform {
    /// Cache of discovered HWNDs, indexed by ElementHandle::Index
    element_cache: std::sync::Mutex<Vec<isize>>, // HWND is pointer-sized, store as isize
}

impl WindowsPlatform {
    pub fn new() -> Self {
        Self {
            element_cache: std::sync::Mutex::new(Vec::new()),
        }
    }
}

impl Platform for WindowsPlatform {
    fn check_accessibility_permission(&self) -> bool {
        // Windows doesn't require explicit accessibility permission for SendMessage
        true
    }

    fn scan(&self, pid: u32) -> Result<ScanResult, String> {
        let (scan, handles) = accessibility::scan_app(pid)?;
        let mut cache = self.element_cache.lock().unwrap();
        *cache = handles;
        Ok(scan)
    }

    fn perform_action(&self, _pid: u32, handle: &ElementHandle, action: &str) -> Result<(), String> {
        let cache = self.element_cache.lock().unwrap();
        let hwnd = match handle {
            ElementHandle::Index(i) => *cache.get(*i).ok_or("invalid element handle")?,
            ElementHandle::None => return Err("element has no handle".into()),
        };
        accessibility::perform_action(hwnd, action)
    }

    fn set_value(&self, _pid: u32, handle: &ElementHandle, value: &str) -> Result<(), String> {
        let cache = self.element_cache.lock().unwrap();
        let hwnd = match handle {
            ElementHandle::Index(i) => *cache.get(*i).ok_or("invalid element handle")?,
            ElementHandle::None => return Err("element has no handle".into()),
        };
        accessibility::set_value(hwnd, value)
    }

    fn running_apps(&self) -> Vec<AppInfo> {
        apps::running_apps()
    }

    fn activate_app(&self, pid: u32) -> Result<(), String> {
        apps::activate(pid)
    }

    fn type_text(&self, text: &str, _pid: u32) -> Result<(), String> {
        input::type_text(text)
    }

    fn press_key(&self, combo: &str, _pid: u32) -> Result<(), String> {
        input::press_key(combo)
    }

    fn screenshot(&self, pid: u32) -> Result<Vec<u8>, String> {
        capture::screenshot(pid)
    }
}
