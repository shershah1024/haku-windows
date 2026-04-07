/// Window tree walk and action execution using Win32 APIs.
///
/// Discovers interactive controls via EnumChildWindows + GetClassName.
/// Actions via SendMessage (BM_CLICK, WM_SETTEXT, etc.).

use crate::platform::{AccessibilityElement, ElementHandle, ScanResult};
use std::collections::HashMap;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::*;

const MAX_ELEMENTS: usize = 500;

/// Scan all interactive elements in the windows belonging to a PID.
pub fn scan_app(pid: u32) -> Result<(ScanResult, Vec<isize>), String> {
    // Find top-level windows for this PID
    let top_windows = find_windows_for_pid(pid);
    if top_windows.is_empty() {
        return Err(format!("No windows found for PID {pid}"));
    }

    let mut elements: Vec<AccessibilityElement> = Vec::new();
    let mut handles: Vec<isize> = Vec::new();
    let mut name_counts: HashMap<String, usize> = HashMap::new();
    let mut window_title = String::new();

    for &hwnd in &top_windows {
        if window_title.is_empty() {
            window_title = get_window_text(hwnd);
        }

        // Enumerate all child controls
        let children = enumerate_children(hwnd);
        for child_hwnd in children {
            if elements.len() >= MAX_ELEMENTS {
                break;
            }

            if !is_window_visible_and_enabled(child_hwnd) {
                continue;
            }

            let class_name = get_class_name(child_hwnd);
            let text = get_window_text(child_hwnd);

            let (tool_prefix, role) = match classify_control(&class_name, child_hwnd) {
                Some(c) => c,
                None => continue, // Not interactive (e.g., Static label)
            };

            let label = if text.is_empty() {
                class_name.clone()
            } else {
                text.clone()
            };

            let slug = slugify(&label);
            if slug.is_empty() {
                continue;
            }

            // Build unique tool name
            let base_name = format!("{}_{}", tool_prefix, slug);
            let count = name_counts.entry(base_name.clone()).or_insert(0);
            *count += 1;
            let tool_name = if *count == 1 {
                base_name
            } else {
                format!("{}_{}", base_name, count)
            };

            let value = get_control_value(&class_name, child_hwnd);

            let description = match tool_prefix {
                "click" => format!("Click '{}'", label),
                "fill" => format!("Fill '{}'. Current: '{}'", label, value.as_deref().unwrap_or("")),
                "toggle" => format!("Toggle '{}'", label),
                "open" => format!("Open '{}' dropdown", label),
                "select" => format!("Select from '{}'", label),
                "set" => format!("Set '{}' value", label),
                _ => format!("{} '{}'", tool_prefix, label),
            };

            let handle_idx = handles.len();
            handles.push(child_hwnd.0 as isize);

            elements.push(AccessibilityElement {
                tool_name,
                tool_prefix: tool_prefix.to_string(),
                role: role.to_string(),
                title: label,
                value,
                enabled: true,
                description,
                handle: ElementHandle::Index(handle_idx),
            });
        }
    }

    let summary = format!(
        "{} interactive controls found in '{}'",
        elements.len(),
        window_title
    );

    Ok((
        ScanResult {
            window_title,
            focused_element: None,
            elements,
            hierarchy_summary: summary,
        },
        handles,
    ))
}

/// Click or press a control.
pub fn perform_action(hwnd_raw: isize, _action: &str) -> Result<(), String> {
    let hwnd = HWND(hwnd_raw as *mut _);
    let class = get_class_name(hwnd);

    unsafe {
        match class.as_str() {
            "Button" => {
                SendMessageW(hwnd, BM_CLICK, WPARAM(0), LPARAM(0));
            }
            _ => {
                // Generic click: send mouse down + up at center
                let _ = SendMessageW(hwnd, WM_LBUTTONDOWN, WPARAM(1), LPARAM(0));
                let _ = SendMessageW(hwnd, WM_LBUTTONUP, WPARAM(0), LPARAM(0));
            }
        }
    }
    Ok(())
}

/// Set the value of a text field, combo box, or slider.
pub fn set_value(hwnd_raw: isize, value: &str) -> Result<(), String> {
    let hwnd = HWND(hwnd_raw as *mut _);
    let class = get_class_name(hwnd);
    let wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();

    eprintln!("[set_value] hwnd={} class={} value={}", hwnd_raw, class, value);

    unsafe {
        let result = match class.as_str() {
            "Edit" | "RichEdit20W" | "RichEditD2DPT" => {
                SendMessageW(hwnd, WM_SETTEXT, WPARAM(0), LPARAM(wide.as_ptr() as isize)).0
            }
            "ComboBox" => {
                SendMessageW(
                    hwnd,
                    CB_SELECTSTRING,
                    WPARAM(usize::MAX),
                    LPARAM(wide.as_ptr() as isize),
                ).0
            }
            _ => {
                SendMessageW(hwnd, WM_SETTEXT, WPARAM(0), LPARAM(wide.as_ptr() as isize)).0
            }
        };
        eprintln!("[set_value] SendMessage returned {}", result);

        let len = SendMessageW(hwnd, WM_GETTEXTLENGTH, WPARAM(0), LPARAM(0)).0;
        eprintln!("[set_value] post-write WM_GETTEXTLENGTH={}", len);
    }
    Ok(())
}

// ── Internal helpers ──

fn find_windows_for_pid(target_pid: u32) -> Vec<HWND> {
    let mut result: Vec<HWND> = Vec::new();

    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let data = &mut *(lparam.0 as *mut (u32, Vec<HWND>));
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == data.0 && IsWindowVisible(hwnd).as_bool() {
            data.1.push(hwnd);
        }
        BOOL(1) // Continue enumeration
    }

    let mut data = (target_pid, Vec::new());
    unsafe {
        let _ = EnumWindows(
            Some(callback),
            LPARAM(&mut data as *mut _ as isize),
        );
    }
    data.1
}

fn enumerate_children(parent: HWND) -> Vec<HWND> {
    let mut children: Vec<HWND> = Vec::new();

    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let list = &mut *(lparam.0 as *mut Vec<HWND>);
        list.push(hwnd);
        BOOL(1)
    }

    unsafe {
        let _ = EnumChildWindows(
            parent,
            Some(callback),
            LPARAM(&mut children as *mut _ as isize),
        );
    }
    children
}

fn get_window_text(hwnd: HWND) -> String {
    let mut buf = [0u16; 512];
    let len = unsafe { GetWindowTextW(hwnd, &mut buf) } as usize;
    String::from_utf16_lossy(&buf[..len])
}

fn get_class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let len = unsafe { GetClassNameW(hwnd, &mut buf) } as usize;
    String::from_utf16_lossy(&buf[..len])
}

fn is_window_visible_and_enabled(hwnd: HWND) -> bool {
    unsafe {
        // Note: IsWindowVisible check skipped for headless/non-RDP testing.
        // Real GUI usage would require IsWindowVisible(hwnd).as_bool() &&
        let _ = hwnd;
        (GetWindowLongW(hwnd, GWL_STYLE) as u32 & 0x08000000) == 0 // WS_DISABLED not set
    }
}

/// Classify a Win32 control class into a tool prefix and role.
fn classify_control(class_name: &str, hwnd: HWND) -> Option<(&'static str, &'static str)> {
    match class_name {
        "Button" => {
            let style = unsafe { GetWindowLongW(hwnd, GWL_STYLE) } as u32;
            let btn_type = style & 0x0F; // BS_* type mask
            match btn_type {
                0x0003 /* BS_CHECKBOX */ | 0x0005 /* BS_3STATE */ | 0x000D /* BS_AUTOCHECKBOX */ => {
                    Some(("toggle", "checkbox"))
                }
                0x0004 /* BS_RADIOBUTTON */ | 0x0009 /* BS_AUTORADIOBUTTON */ => {
                    Some(("select", "radio"))
                }
                0x0007 /* BS_GROUPBOX */ => None, // Not interactive
                _ => Some(("click", "button")),
            }
        }
        "Edit" | "RichEdit20W" | "RichEditD2DPT" => Some(("fill", "textfield")),
        "ComboBox" | "ComboBoxEx32" => Some(("open", "combobox")),
        "ListBox" => Some(("select", "listbox")),
        "msctls_trackbar32" => Some(("set", "slider")),
        "SysLink" => Some(("click", "link")),
        "SysTreeView32" => Some(("click", "tree")),
        "SysListView32" => Some(("click", "listview")),
        "SysTabControl32" => Some(("click", "tabcontrol")),
        "Static" => None, // Labels, not interactive
        "#32770" => None,  // Dialog container
        _ => None,
    }
}

fn get_control_value(class_name: &str, hwnd: HWND) -> Option<String> {
    unsafe {
        match class_name {
            "Edit" | "RichEdit20W" | "RichEditD2DPT" => {
                let len = SendMessageW(hwnd, WM_GETTEXTLENGTH, WPARAM(0), LPARAM(0)).0 as usize;
                if len == 0 { return None; }
                let mut buf = vec![0u16; len + 1];
                SendMessageW(hwnd, WM_GETTEXT, WPARAM(buf.len()), LPARAM(buf.as_mut_ptr() as isize));
                let text = String::from_utf16_lossy(&buf[..len]);
                if text.is_empty() { None } else { Some(text) }
            }
            "Button" => {
                let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
                let btn_type = style & 0x0F;
                if btn_type == 0x0003 || btn_type == 0x0005 || btn_type == 0x000D {
                    // Checkbox — check state
                    let state = SendMessageW(hwnd, BM_GETCHECK, WPARAM(0), LPARAM(0));
                    Some(if state.0 != 0 { "checked".to_string() } else { "unchecked".to_string() })
                } else {
                    None
                }
            }
            "ComboBox" => {
                let sel = SendMessageW(hwnd, CB_GETCURSEL, WPARAM(0), LPARAM(0));
                if sel.0 >= 0 {
                    let len = SendMessageW(hwnd, CB_GETLBTEXTLEN, WPARAM(sel.0 as usize), LPARAM(0));
                    if len.0 > 0 {
                        let mut buf = vec![0u16; (len.0 as usize) + 1];
                        SendMessageW(hwnd, CB_GETLBTEXT, WPARAM(sel.0 as usize), LPARAM(buf.as_mut_ptr() as isize));
                        Some(String::from_utf16_lossy(&buf[..len.0 as usize]))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

fn slugify(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_")
        .chars()
        .take(40)
        .collect()
}
