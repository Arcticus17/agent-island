//! 打开外部链接（自更新提示里的"下载页"）。
//!
//! 只允许 https，且做最小校验：这是应用里唯一能把任意字符串交给系统的入口，
//! 不能让它变成"用任意参数启动任意程序"的通道。

use crate::*;

/// 允许打开的 URL：必须是 https、有主机名、没有控制字符。
pub(crate) fn validate_external_url(url: &str) -> Result<(), String> {
    let trimmed = url.trim();
    if trimmed.len() > 2_048 {
        return Err("链接太长".into());
    }
    if trimmed.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        return Err("链接包含非法字符".into());
    }
    let Some(rest) = trimmed.strip_prefix("https://") else {
        return Err("只允许 https 链接".into());
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() || !host.contains('.') {
        return Err("链接缺少有效主机名".into());
    }
    Ok(())
}

#[cfg(windows)]
#[tauri::command]
pub(crate) fn open_external(url: String) -> Result<(), String> {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    validate_external_url(&url)?;
    let target = HSTRING::from(url.trim());
    let operation = HSTRING::from("open");
    let result = unsafe { ShellExecuteW(None, &operation, &target, None, None, SW_SHOWNORMAL) };
    // ShellExecuteW 返回值 <= 32 表示失败
    if result.0 as isize <= 32 {
        return Err("系统拒绝打开该链接".into());
    }
    Ok(())
}

#[cfg(not(windows))]
#[tauri::command]
pub(crate) fn open_external(url: String) -> Result<(), String> {
    validate_external_url(&url)?;
    Err("当前平台不支持打开外部链接".into())
}

#[cfg(test)]
mod tests {
    use super::validate_external_url;

    #[test]
    fn accepts_plain_https_urls() {
        assert!(validate_external_url("https://github.com/owner/repo/releases").is_ok());
        assert!(validate_external_url("https://example.com").is_ok());
        assert!(validate_external_url("  https://example.com/x?y=1  ").is_ok());
    }

    #[test]
    fn rejects_dangerous_or_malformed_urls() {
        assert!(validate_external_url("http://example.com").is_err(), "只允许 https");
        assert!(validate_external_url("file:///C:/Windows/System32/calc.exe").is_err());
        assert!(validate_external_url("javascript:alert(1)").is_err());
        assert!(validate_external_url("https://").is_err());
        assert!(validate_external_url("https://localhost").is_err(), "需要点号主机名");
        assert!(validate_external_url("https://example.com/a b").is_err(), "不允许空白");
        assert!(validate_external_url("https://example.com/\nX").is_err(), "不允许控制字符");
        assert!(validate_external_url(&format!("https://example.com/{}", "a".repeat(2_100))).is_err());
    }
}
