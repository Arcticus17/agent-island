//! 全局快捷键：Ctrl+Shift+A 唤出/收起灵动岛。
//!
//! 岛默认是隐藏态，只能靠鼠标蹭到屏幕顶部才能看见；键盘用户/多显示器场景下
//! 没有"召唤"入口就很难用。这里用 Win32 `RegisterHotKey` 注册一个系统级热键：
//! 在线程自己的消息队列里收 `WM_HOTKEY`，再通过 Tauri 事件通知前端。
//!
//! 不引入 `tauri-plugin-global-shortcut`：只要能解析一个固定组合键，
//! 用已有的 `windows` crate 就够了，少一个依赖、少一份权限配置。

use tauri::Emitter;

pub(crate) const TOGGLE_SHORTCUT: &str = "Ctrl+Shift+A";
pub(crate) const HOTKEY_EVENT: &str = "island-hotkey";

const MOD_ALT: u32 = 0x0001;
const MOD_CONTROL: u32 = 0x0002;
const MOD_SHIFT: u32 = 0x0004;
const MOD_NOREPEAT: u32 = 0x4000;
const WM_HOTKEY: u32 = 0x0312;

/// 解析 "Ctrl+Shift+A" → (modifiers, vk)。只支持单字母/数字键，够用且不引入解析库。
pub(crate) fn parse_shortcut(shortcut: &str) -> Option<(u32, u32)> {
    let mut modifiers = MOD_NOREPEAT;
    let mut key: Option<u32> = None;
    for part in shortcut.split('+').map(str::trim).filter(|part| !part.is_empty()) {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => modifiers |= MOD_CONTROL,
            "shift" => modifiers |= MOD_SHIFT,
            "alt" => modifiers |= MOD_ALT,
            other => {
                if key.is_some() {
                    return None;
                }
                let mut chars = other.chars();
                let ch = chars.next()?;
                if chars.next().is_some() || !ch.is_ascii_alphanumeric() {
                    return None;
                }
                key = Some(ch.to_ascii_uppercase() as u32);
            }
        }
    }
    key.map(|vk| (modifiers, vk))
}

#[cfg(windows)]
pub(crate) fn spawn_toggle_hotkey(app: tauri::AppHandle) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS};
    use windows::Win32::UI::WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY as WIN_WM_HOTKEY};

    let Some((modifiers, vk)) = parse_shortcut(TOGGLE_SHORTCUT) else {
        return;
    };
    std::thread::spawn(move || unsafe {
        // hwnd 传 null：WM_HOTKEY 会投递到本线程的消息队列
        if RegisterHotKey(None, 1, HOT_KEY_MODIFIERS(modifiers), vk).is_err() {
            // 组合键被别的程序占了：静默放弃，不影响应用本体
            return;
        }
        let mut message: MSG = std::mem::zeroed();
        loop {
            let result = GetMessageW(&mut message, None, 0, 0);
            if result.0 <= 0 {
                break;
            }
            if message.message == WIN_WM_HOTKEY || message.message == WM_HOTKEY {
                let _ = app.emit(HOTKEY_EVENT, ());
            }
        }
        let _ = UnregisterHotKey(None, 1);
    });
}

#[cfg(not(windows))]
pub(crate) fn spawn_toggle_hotkey(_app: tauri::AppHandle) {}

#[cfg(test)]
mod tests {
    use super::*;

    const VK_A: u32 = 0x41;

    #[test]
    fn parses_plain_and_modified_shortcuts() {
        assert_eq!(parse_shortcut("Ctrl+Shift+A"), Some((MOD_NOREPEAT | MOD_CONTROL | MOD_SHIFT, VK_A)));
        assert_eq!(parse_shortcut("ctrl+alt+1"), Some((MOD_NOREPEAT | MOD_CONTROL | MOD_ALT, '1' as u32)));
        assert_eq!(parse_shortcut("A"), Some((MOD_NOREPEAT, VK_A)));
    }

    #[test]
    fn rejects_invalid_shortcuts() {
        assert_eq!(parse_shortcut(""), None);
        assert_eq!(parse_shortcut("Ctrl+Shift+"), None);
        assert_eq!(parse_shortcut("Ctrl+A+B"), None);
        assert_eq!(parse_shortcut("Ctrl+F1"), None);
    }
}




