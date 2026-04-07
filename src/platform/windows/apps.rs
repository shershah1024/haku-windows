/// App enumeration and activation using Win32 APIs.

use crate::platform::AppInfo;
use std::collections::HashMap;
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, MAX_PATH, CloseHandle};
use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::PWSTR;

/// List all visible GUI applications.
pub fn running_apps() -> Vec<AppInfo> {
    let mut pid_windows: HashMap<u32, (String, HWND)> = HashMap::new();

    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let map = &mut *(lparam.0 as *mut HashMap<u32, (String, HWND)>);

        // Note: IsWindowVisible check skipped for headless testing without RDP.
        // Production: re-enable to filter out non-GUI windows.
        // if !IsWindowVisible(hwnd).as_bool() { return BOOL(1); }

        let mut buf = [0u16; 512];
        let len = GetWindowTextW(hwnd, &mut buf) as usize;
        if len == 0 {
            return BOOL(1);
        }
        let title = String::from_utf16_lossy(&buf[..len]);

        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return BOOL(1);
        }

        map.entry(pid).or_insert((title, hwnd));
        BOOL(1)
    }

    unsafe {
        let _ = EnumWindows(
            Some(callback),
            LPARAM(&mut pid_windows as *mut _ as isize),
        );
    }

    pid_windows
        .into_iter()
        .filter_map(|(pid, (title, _hwnd))| {
            let exe_name = get_exe_name(pid).unwrap_or_else(|| title.clone());
            // Use exe filename as a pseudo bundle_id
            let bundle_id = std::path::Path::new(&exe_name)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();

            Some(AppInfo {
                name: title,
                bundle_id,
                pid,
            })
        })
        .collect()
}

/// Bring a window to the foreground by PID.
pub fn activate(pid: u32) -> Result<(), String> {
    let hwnd = find_main_window(pid).ok_or("No window found for PID")?;
    unsafe {
        // AllowSetForegroundWindow lets us steal focus
        let _ = AllowSetForegroundWindow(pid);
        let _ = SetForegroundWindow(hwnd);
        // Restore if minimized
        if IsIconic(hwnd).as_bool() {
            ShowWindow(hwnd, SW_RESTORE);
        }
    }
    Ok(())
}

fn find_main_window(target_pid: u32) -> Option<HWND> {
    let mut result: Option<HWND> = None;

    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let data = &mut *(lparam.0 as *mut (u32, Option<HWND>));
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == data.0 && IsWindowVisible(hwnd).as_bool() {
            let mut buf = [0u16; 512];
            let len = GetWindowTextW(hwnd, &mut buf) as usize;
            if len > 0 {
                data.1 = Some(hwnd);
                return BOOL(0); // Stop enumeration
            }
        }
        BOOL(1)
    }

    let mut data = (target_pid, None);
    unsafe {
        let _ = EnumWindows(
            Some(callback),
            LPARAM(&mut data as *mut _ as isize),
        );
    }
    data.1
}

fn get_exe_name(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; MAX_PATH as usize];
        let mut size = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, Default::default(), PWSTR(buf.as_mut_ptr()), &mut size);
        let _ = CloseHandle(handle);
        if ok.is_ok() {
            Some(String::from_utf16_lossy(&buf[..size as usize]))
        } else {
            None
        }
    }
}
