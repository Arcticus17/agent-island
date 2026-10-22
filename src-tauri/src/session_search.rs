//! 跨会话搜索：在本地会话文件里按关键字找消息。
//!
//! 只读**尾部一段**（默认 512 KB）并复用各 adapter 的解析器，避免为了搜索把几十个
//! 会话文件整个读进内存。命中后返回片段与文件路径，前端只负责展示与"在资源管理器里定位"。
//!
//! 有意不做：不建倒排索引（文件量小、收益低）、不搜工具输出（只搜用户/助手消息文本）。

use crate::*;
use std::path::{Path, PathBuf};

/// 最多扫描多少个会话文件（新的优先）。
pub(crate) const MAX_FILES: usize = 40;
/// 单文件只读尾部这么多字节。
pub(crate) const TAIL_BYTES: u64 = 512 * 1024;
/// 片段两侧各留多少字符。
const SNIPPET_PAD: usize = 40;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub(crate) struct SessionSearchHit {
    /// claude / codex
    pub agent: String,
    pub session: String,
    pub role: String,
    pub snippet: String,
    pub at_ms: u64,
    pub path: String,
}

fn searchable_roots() -> Vec<(&'static str, PathBuf)> {
    let mut roots = Vec::new();
    if let Some(home) = home_dir() {
        roots.push(("claude", home.join(".claude").join("projects")));
    }
    if let Some(root) = codex_data_root() {
        roots.push(("codex", root.join("sessions")));
    }
    roots
}

fn role_label(role: domain::MessageRole) -> String {
    match role {
        domain::MessageRole::User => "User".into(),
        domain::MessageRole::Assistant => "Assistant".into(),
        domain::MessageRole::Tool => "Tool".into(),
    }
}

/// 从命中位置向两侧展开，做一个单行片段。
pub(crate) fn snippet_around(text: &str, at: usize, len: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    // byte 偏移 → 字符偏移（关键字可能含中文，必须先转换）
    let char_at = text[..at].chars().count();
    let char_len = text[at..at + len].chars().count();
    let start = char_at.saturating_sub(SNIPPET_PAD);
    let end = (char_at + char_len + SNIPPET_PAD).min(chars.len());
    let mut snippet: String = chars[start..end].iter().collect();
    snippet = snippet.replace(['\n', '\r', '\t'], " ");
    if start > 0 {
        snippet.insert(0, '…');
    }
    if end < chars.len() {
        snippet.push('…');
    }
    snippet
}

/// 在单个文件里找命中；`limit` 限制返回条数。
pub(crate) fn search_in_file(
    agent: &str,
    path: &Path,
    query: &str,
    limit: usize,
) -> Vec<SessionSearchHit> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() || limit == 0 {
        return Vec::new();
    }
    let text = read_tail(path, TAIL_BYTES);
    if text.is_empty() {
        return Vec::new();
    }
    let report = match agent {
        "codex" => adapters::codex::CodexAdapter.parse(&text),
        "claude" => adapters::claude::ClaudeAdapter.parse(&text),
        _ => return Vec::new(),
    };
    let at_ms = std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0);
    let session = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut hits = Vec::new();
    for message in &report.messages {
        if message.role == domain::MessageRole::Tool {
            continue;
        }
        let haystack = message.text.to_lowercase();
        let Some(at) = haystack.find(&needle) else {
            continue;
        };
        hits.push(SessionSearchHit {
            agent: agent.to_string(),
            session: session.clone(),
            role: role_label(message.role),
            snippet: snippet_around(&message.text, at, needle.len()),
            at_ms,
            path: path.to_string_lossy().into_owned(),
        });
        if hits.len() >= limit {
            break;
        }
    }
    hits
}

/// 跨会话搜索：新文件优先，凑够 `limit` 条就停。
pub(crate) fn search_sessions(query: &str, limit: usize) -> Vec<SessionSearchHit> {
    let needle = query.trim();
    if needle.chars().count() < 2 || limit == 0 {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for (agent, root) in searchable_roots() {
        if hits.len() >= limit {
            break;
        }
        if !root.exists() {
            continue;
        }
        for path in session_scan::list_newest_files(&root, 3, "jsonl", MAX_FILES) {
            if hits.len() >= limit {
                break;
            }
            let remaining = limit - hits.len();
            hits.extend(search_in_file(agent, &path, needle, remaining));
        }
    }
    hits
}

#[tauri::command]
pub(crate) fn search_sessions_command(query: String, limit: Option<usize>) -> Vec<SessionSearchHit> {
    search_sessions(&query, limit.unwrap_or(30).clamp(1, 100))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("agent-island-search-{tag}-{}", epoch_millis()))
    }

    fn write_claude_session(path: &Path, texts: &[&str]) {
        let mut body = String::new();
        for (index, text) in texts.iter().enumerate() {
            body.push_str(&format!(
                "{{\"type\":\"assistant\",\"sessionId\":\"s-{index}\",\"uuid\":\"a-{index}\",\"timestamp\":\"2026-10-22T10:0{index}:00Z\",\"message\":{{\"role\":\"assistant\",\"content\":[{{\"type\":\"text\",\"text\":\"{text}\"}}]}}}}\n"
            ));
        }
        std::fs::write(path, body).unwrap();
    }

    #[test]
    fn finds_messages_and_builds_snippets() {
        let dir = temp_dir("hit");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session-a.jsonl");
        write_claude_session(&path, &["请检查布局保存与状态显示", "无关的一句话"]);
        let hits = search_in_file("claude", &path, "布局", 5);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].session, "session-a");
        assert_eq!(hits[0].role, "Assistant");
        assert!(hits[0].snippet.contains("布局"));
        assert!(!hits[0].snippet.contains('\n'));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn is_case_insensitive_and_respects_the_limit() {
        let dir = temp_dir("limit");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session-b.jsonl");
        write_claude_session(&path, &["NPM install one", "npm install two", "npm install three"]);
        assert_eq!(search_in_file("claude", &path, "npm", 2).len(), 2);
        assert!(search_in_file("claude", &path, "NPM", 5).len() >= 3);
        assert!(search_in_file("claude", &path, "", 5).is_empty());
        assert!(search_in_file("claude", &path, "不存在的词", 5).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn snippet_keeps_context_and_survives_multibyte_text() {
        let text = format!("{}关键内容{}", "前".repeat(60), "后".repeat(60));
        let at = text.find("关键内容").unwrap();
        let snippet = snippet_around(&text, at, "关键内容".len());
        assert!(snippet.contains("关键内容"));
        assert!(snippet.starts_with('…') && snippet.ends_with('…'));
        assert!(snippet.chars().count() < text.chars().count());
    }


    #[test]
    fn only_session_files_under_known_roots_can_be_revealed() {
        let home = home_dir().unwrap();
        let claude = home.join(".claude").join("projects").join("p").join("s.jsonl");
        assert!(reveal_allowed(&claude));
        assert!(!reveal_allowed(&home.join(".claude").join("projects").join("p").join("notes.txt")));
        assert!(!reveal_allowed(&home.join("Downloads").join("evil.jsonl")));
    }

    #[test]
    fn short_queries_do_not_scan() {
        assert!(search_sessions("x", 10).is_empty());
        assert!(search_sessions("  ", 10).is_empty());
        assert!(search_sessions("ab", 0).is_empty());
    }
}
/// 判断路径是否落在允许"在资源管理器里定位"的目录内。
pub(crate) fn reveal_allowed(path: &Path) -> bool {
    if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
        return false;
    }
    let Some(home) = home_dir() else {
        return false;
    };
    let mut roots: Vec<PathBuf> = vec![home.join(".claude").join("projects")];
    if let Some(root) = codex_data_root() {
        roots.push(root.join("sessions"));
    }
    roots
        .iter()
        .any(|root| path.starts_with(root))
}

#[cfg(windows)]
#[tauri::command]
pub(crate) fn reveal_session(path: String) -> Result<(), String> {
    let candidate = PathBuf::from(path.trim());
    if !candidate.is_absolute() {
        return Err("需要绝对路径".into());
    }
    if !reveal_allowed(&candidate) {
        return Err("只能定位 Agent 会话文件".into());
    }
    std::process::Command::new("explorer.exe")
        .arg(format!("/select,{}", candidate.display()))
        .spawn()
        .map_err(|error| format!("无法打开资源管理器：{error}"))?;
    Ok(())
}

#[cfg(not(windows))]
#[tauri::command]
pub(crate) fn reveal_session(path: String) -> Result<(), String> {
    let candidate = PathBuf::from(path.trim());
    if !candidate.is_absolute() || !reveal_allowed(&candidate) {
        return Err("只能定位 Agent 会话文件".into());
    }
    Err("当前平台不支持定位文件".into())
}
