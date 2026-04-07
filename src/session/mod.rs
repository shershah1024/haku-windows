mod browser;
mod manager;

pub use browser::BrowserSessionManager;
pub use manager::SessionManager;

/// Return the DOM scanner JavaScript (embedded).
pub fn dom_scanner_js() -> &'static str {
    include_str!("dom_scanner.js")
}
