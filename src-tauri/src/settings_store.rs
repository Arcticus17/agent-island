//! 用户设置的持久化。
//!
//! 之前所有设置（主题、图标、布局、窗口位置……）都只存在 WebView 的 localStorage 里：
//! 进程被强杀时最后一次写入可能还没落盘，重启就回到默认值（实测丢过一次主题）。
//! 这里落一个 `~/.agent-island/settings.json`，写入走「临时文件 + rename」保证原子性，
//! 前端只在 localStorage 变更后同步过来，读取仍在 localStorage（快、同步、组件不用改）。

use crate::*;
use std::fs;
use std::path::{Path, PathBuf};

/// 设置上限：正常只有几百字节，超过这个数说明写错了，直接拒绝避免把磁盘写爆。
const MAX_SETTINGS_BYTES: usize = 256 * 1024;

pub(crate) fn settings_path() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".agent-island")
        .join("settings.json")
}

/// 读设置；文件不存在 / 损坏 / 超大都当"没有设置"，不要让应用起不来。
pub(crate) fn load_settings(path: &Path) -> Option<String> {
    let raw = fs::read_to_string(path).ok()?.trim_start_matches('\u{feff}').to_string();
    if raw.len() > MAX_SETTINGS_BYTES {
        return None;
    }
    serde_json::from_str::<serde_json::Value>(&raw).ok()?;
    Some(raw)
}

/// 原子写：先写 `settings.json.tmp` 再 rename，避免写一半被杀留下半个 JSON。
pub(crate) fn save_settings(path: &Path, json: &str) -> Result<(), String> {
    if json.len() > MAX_SETTINGS_BYTES {
        return Err(format!("设置过大：{} 字节", json.len()));
    }
    serde_json::from_str::<serde_json::Value>(json).map_err(|error| format!("设置不是合法 JSON：{error}"))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建设置目录失败：{error}"))?;
    }
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, json).map_err(|error| format!("写入设置失败：{error}"))?;
    fs::rename(&temp, path).map_err(|error| format!("替换设置失败：{error}"))?;
    Ok(())
}

#[tauri::command]
pub(crate) fn load_app_settings() -> Option<String> {
    load_settings(&settings_path())
}

#[tauri::command]
pub(crate) fn save_app_settings(settings: String) -> Result<(), String> {
    save_settings(&settings_path(), &settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("agent-island-settings-{tag}-{}", epoch_millis()))
            .join("settings.json")
    }

    #[test]
    fn round_trips_settings_through_disk() {
        let path = temp_path("roundtrip");
        let json = r#"{"agent-island-theme-style-v2":"terminal","agent-island-icon-style":"pixel"}"#;
        save_settings(&path, json).unwrap();
        assert_eq!(load_settings(&path).as_deref(), Some(json));
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn overwrites_atomically_and_leaves_no_temp_file() {
        let path = temp_path("overwrite");
        save_settings(&path, r#"{"a":"1"}"#).unwrap();
        save_settings(&path, r#"{"a":"2"}"#).unwrap();
        assert_eq!(load_settings(&path).as_deref(), Some(r#"{"a":"2"}"#));
        assert!(
            !path.with_extension("json.tmp").exists(),
            "临时文件应该在 rename 后消失"
        );
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn rejects_invalid_and_oversized_payloads() {
        let path = temp_path("reject");
        assert!(save_settings(&path, "not json").is_err());
        assert!(save_settings(&path, &format!("\"{}\"", "x".repeat(MAX_SETTINGS_BYTES))).is_err());
        assert!(!path.exists(), "拒绝之后不该留下文件");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn corrupt_and_missing_files_read_as_none() {
        let path = temp_path("corrupt");
        assert_eq!(load_settings(&path), None);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{broken").unwrap();
        assert_eq!(load_settings(&path), None);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}

