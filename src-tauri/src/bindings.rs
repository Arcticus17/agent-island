//! 手动会话绑定：把"这个 Agent 就是那个会话"变成一个显式、可撤销、可持久化的事实。
//!
//! 岛外启动的 Agent 只能靠日志推断（`Inferred`）。与其一直猜，不如让用户点一次：
//! 绑定后该 Agent 的活动会话固定为指定 session，跨重启仍生效，直到手动解除。

use crate::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct BoundSession {
    pub session_id: String,
    pub bound_at_ms: u64,
}

pub(crate) fn bindings_path() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".agent-island")
        .join("session-bindings.json")
}

/// 读绑定表；文件损坏/不存在都当空表，不让界面起不来。
pub(crate) fn load_bindings(path: &Path) -> HashMap<String, BoundSession> {
    fs::read_to_string(path)
        .ok()
        .map(|raw| raw.trim_start_matches('\u{feff}').to_string())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/// 原子写绑定表。
pub(crate) fn save_bindings(
    path: &Path,
    bindings: &HashMap<String, BoundSession>,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建绑定目录失败：{error}"))?;
    }
    let body = serde_json::to_string_pretty(bindings).map_err(|error| error.to_string())?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, body).map_err(|error| format!("写入绑定失败：{error}"))?;
    fs::rename(&temp, path).map_err(|error| format!("替换绑定失败：{error}"))?;
    Ok(())
}

#[tauri::command]
pub(crate) fn bind_session(
    state: tauri::State<AppState>,
    agent_id: String,
    session_id: String,
) -> Result<Vec<(String, String)>, String> {
    let agent_id = agent_id.trim().to_string();
    let session_id = session_id.trim().to_string();
    if agent_id.is_empty() || session_id.is_empty() {
        return Err("agent_id 和 session_id 都不能为空".into());
    }
    let mut bindings = state.bindings.lock_recover();
    bindings.insert(
        agent_id,
        BoundSession {
            session_id,
            bound_at_ms: epoch_millis(),
        },
    );
    save_bindings(&bindings_path(), &bindings)?;
    Ok(list_of(&bindings))
}

#[tauri::command]
pub(crate) fn unbind_session(
    state: tauri::State<AppState>,
    agent_id: String,
) -> Result<Vec<(String, String)>, String> {
    let mut bindings = state.bindings.lock_recover();
    bindings.remove(agent_id.trim());
    save_bindings(&bindings_path(), &bindings)?;
    Ok(list_of(&bindings))
}

#[tauri::command]
pub(crate) fn list_session_bindings(state: tauri::State<AppState>) -> Vec<(String, String)> {
    list_of(&state.bindings.lock_recover())
}

fn list_of(bindings: &HashMap<String, BoundSession>) -> Vec<(String, String)> {
    let mut entries: Vec<(String, String)> = bindings
        .iter()
        .map(|(agent, bound)| (agent.clone(), bound.session_id.clone()))
        .collect();
    entries.sort();
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("agent-island-bindings-{tag}-{}", epoch_millis()))
            .join("session-bindings.json")
    }

    #[test]
    fn round_trips_bindings_and_tolerates_a_bom() {
        let path = temp_path("roundtrip");
        let mut bindings = HashMap::new();
        bindings.insert(
            "claude".to_string(),
            BoundSession { session_id: "s-1".into(), bound_at_ms: 42 },
        );
        save_bindings(&path, &bindings).unwrap();
        assert_eq!(load_bindings(&path), bindings);

        // PowerShell 的 UTF8 会带 BOM：必须能读
        fs::write(&path, "\u{feff}{\"claude\":{\"session_id\":\"s-2\",\"bound_at_ms\":7}}").unwrap();
        let loaded = load_bindings(&path);
        assert_eq!(loaded["claude"].session_id, "s-2");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn corrupt_or_missing_files_read_as_empty() {
        let path = temp_path("corrupt");
        assert!(load_bindings(&path).is_empty());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{not json").unwrap();
        assert!(load_bindings(&path).is_empty());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
