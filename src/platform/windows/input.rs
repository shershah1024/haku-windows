/// Keyboard and mouse input using SendInput.

use windows::Win32::UI::Input::KeyboardAndMouse::*;

/// Type text character-by-character using KEYEVENTF_UNICODE.
pub fn type_text(text: &str) -> Result<(), String> {
    for ch in text.chars() {
        let code = ch as u16;

        let inputs = [
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(0),
                        wScan: code,
                        dwFlags: KEYEVENTF_UNICODE,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(0),
                        wScan: code,
                        dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
        ];

        unsafe {
            SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    Ok(())
}

/// Send a key combo like "ctrl+s", "alt+f4", "return", "tab".
/// On Windows, "cmd" maps to "ctrl" for cross-platform compatibility.
pub fn press_key(combo: &str) -> Result<(), String> {
    let parts: Vec<String> = combo.split('+').map(|s| s.trim().to_lowercase()).collect();

    let mut modifiers: Vec<VIRTUAL_KEY> = Vec::new();
    let mut key_vk = VIRTUAL_KEY(0);

    for part in &parts {
        match part.as_str() {
            "ctrl" | "control" | "cmd" | "command" => modifiers.push(VK_CONTROL),
            "alt" | "option" => modifiers.push(VK_MENU),
            "shift" => modifiers.push(VK_SHIFT),
            "win" | "super" => modifiers.push(VK_LWIN),
            key => {
                key_vk = key_to_vk(key).ok_or_else(|| format!("unknown key: {key}"))?;
            }
        }
    }

    if key_vk == VIRTUAL_KEY(0) && modifiers.is_empty() {
        return Err("no key specified".into());
    }

    let mut inputs: Vec<INPUT> = Vec::new();

    // Modifiers down
    for &vk in &modifiers {
        inputs.push(key_input(vk, false));
    }

    // Key down + up
    if key_vk != VIRTUAL_KEY(0) {
        inputs.push(key_input(key_vk, false));
        inputs.push(key_input(key_vk, true));
    }

    // Modifiers up (reverse order)
    for &vk in modifiers.iter().rev() {
        inputs.push(key_input(vk, true));
    }

    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }

    Ok(())
}

fn key_input(vk: VIRTUAL_KEY, key_up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if key_up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn key_to_vk(key: &str) -> Option<VIRTUAL_KEY> {
    Some(match key {
        "return" | "enter" => VK_RETURN,
        "tab" => VK_TAB,
        "escape" | "esc" => VK_ESCAPE,
        "space" => VK_SPACE,
        "backspace" | "delete" => VK_BACK,
        "del" => VK_DELETE,
        "up" => VK_UP,
        "down" => VK_DOWN,
        "left" => VK_LEFT,
        "right" => VK_RIGHT,
        "home" => VK_HOME,
        "end" => VK_END,
        "pageup" => VK_PRIOR,
        "pagedown" => VK_NEXT,
        "insert" => VK_INSERT,
        "f1" => VK_F1, "f2" => VK_F2, "f3" => VK_F3, "f4" => VK_F4,
        "f5" => VK_F5, "f6" => VK_F6, "f7" => VK_F7, "f8" => VK_F8,
        "f9" => VK_F9, "f10" => VK_F10, "f11" => VK_F11, "f12" => VK_F12,
        "a" => VK_A, "b" => VK_B, "c" => VK_C, "d" => VK_D,
        "e" => VK_E, "f" => VK_F, "g" => VK_G, "h" => VK_H,
        "i" => VK_I, "j" => VK_J, "k" => VK_K, "l" => VK_L,
        "m" => VK_M, "n" => VK_N, "o" => VK_O, "p" => VK_P,
        "q" => VK_Q, "r" => VK_R, "s" => VK_S, "t" => VK_T,
        "u" => VK_U, "v" => VK_V, "w" => VK_W, "x" => VK_X,
        "y" => VK_Y, "z" => VK_Z,
        "0" => VK_0, "1" => VK_1, "2" => VK_2, "3" => VK_3,
        "4" => VK_4, "5" => VK_5, "6" => VK_6, "7" => VK_7,
        "8" => VK_8, "9" => VK_9,
        _ => return None,
    })
}
