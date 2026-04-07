/// Platform abstraction layer.
///
/// Windows implements this with Win32 APIs (SendMessage, EnumChildWindows).
/// macOS/Linux stubs exist for development builds only.

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(not(target_os = "windows"))]
mod stub;

/// Represents a discovered interactive UI element.
#[derive(Debug, Clone)]
pub struct AccessibilityElement {
    pub tool_name: String,
    pub tool_prefix: String,
    pub role: String,
    pub title: String,
    pub value: Option<String>,
    pub enabled: bool,
    pub description: String,
    pub handle: ElementHandle,
}

/// Opaque, platform-specific handle to a UI element.
#[derive(Debug, Clone)]
pub enum ElementHandle {
    Index(usize),
    None,
}

/// Result of scanning an app's UI.
#[derive(Debug, Clone)]
pub struct ScanResult {
    pub window_title: String,
    pub focused_element: Option<String>,
    pub elements: Vec<AccessibilityElement>,
    pub hierarchy_summary: String,
}

/// Info about a running application.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AppInfo {
    pub name: String,
    pub bundle_id: String,
    pub pid: u32,
}

/// The platform trait — everything OS-specific goes here.
pub trait Platform: Send + Sync {
    fn check_accessibility_permission(&self) -> bool;
    fn scan(&self, pid: u32) -> Result<ScanResult, String>;
    fn perform_action(&self, pid: u32, handle: &ElementHandle, action: &str) -> Result<(), String>;
    fn set_value(&self, pid: u32, handle: &ElementHandle, value: &str) -> Result<(), String>;
    fn running_apps(&self) -> Vec<AppInfo>;
    fn activate_app(&self, pid: u32) -> Result<(), String>;
    fn type_text(&self, text: &str, pid: u32) -> Result<(), String>;
    fn press_key(&self, combo: &str, pid: u32) -> Result<(), String>;
    fn screenshot(&self, pid: u32) -> Result<Vec<u8>, String>;
}

/// Create the platform implementation for the current OS.
pub fn create() -> Box<dyn Platform> {
    #[cfg(target_os = "windows")]
    {
        Box::new(windows::WindowsPlatform::new())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Box::new(stub::StubPlatform)
    }
}
