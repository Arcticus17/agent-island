//! 审批允许清单 + 审计。
//!
//! 内置 `classify_command` 只认得极少数安全命令，其余都要人工点一次——同一条
//! `npm test` 一天点十次就会变成无脑点「允许」。这里让用户自己维护少量规则：
//!
//! - 规则 = 工具（可选，`Bash`/`Edit`…）+ 命令模式（支持 `*` 通配）
//! - 命中规则 → 直接回 `allow`，不弹卡片，但必须留痕（审计）
//! - 审计写 `~/.agent-island/approval-audit.jsonl`，只保留最近 200 条

use crate::*;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const MAX_AUDIT_ENTRIES: usize = 200;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct ApprovalRule {
    pub id: String,
    /// 空表示"任意工具"
    #[serde(default)]
    pub tool: String,
    pub pattern: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct AuditEntry {
    pub at_ms: u64,
    pub id: String,
    pub tool: String,
    pub command: String,
    /// 命中的规则 id / pattern
    pub rule: String,
    /// auto_allow / manual_allow / manual_deny
    pub decision: String,
}

pub(crate) fn rules_path() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".agent-island")
        .join("approval-rules.json")
}

pub(crate) fn audit_path() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".agent-island")
        .join("approval-audit.jsonl")
}

/// 单条规则的匹配：`*` 通配、其余按字面子串（大小写不敏感）。
pub(crate) fn pattern_matches(pattern: &str, command: &str) -> bool {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return false;
    }
    let pattern = pattern.to_lowercase();
    let command = command.to_lowercase();
    if !pattern.contains('*') {
        return command.contains(&pattern);
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    let mut cursor = 0usize;
    for (index, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        match command[cursor..].find(part) {
            Some(found) => {
                // 第一段必须从头开始匹配，这样 `npm *` 不会命中 `sudo npm x`
                if index == 0 && found != 0 && !pattern.starts_with('*') {
                    return false;
                }
                cursor += found + part.len();
            }
            None => return false,
        }
    }
    if let Some(last) = parts.last() {
        if !last.is_empty() && !pattern.ends_with('*') && !command.ends_with(last) {
            return false;
        }
    }
    true
}

pub(crate) fn match_rule<'a>(
    rules: &'a [ApprovalRule],
    tool: &str,
    command: &str,
) -> Option<&'a ApprovalRule> {
    rules.iter().find(|rule| {
        let tool_ok = rule.tool.trim().is_empty() || rule.tool.eq_ignore_ascii_case(tool.trim());
        tool_ok && pattern_matches(&rule.pattern, command)
    })
}

/// 与前端 `suggestRulePattern` 保持同一语义：取前两段加通配。
/// 只用于临时授权（10 分钟 / 本会话），不会写进持久规则。
pub(crate) fn suggest_rule_pattern(command: &str) -> String {
    let trimmed = command.split_whitespace().collect::<Vec<_>>().join(" ");
    if trimmed.is_empty() {
        return String::new();
    }
    let tokens: Vec<&str> = trimmed.split(' ').collect();
    if tokens.len() <= 2 {
        // A two-token command is usually already precise (`git status`,
        // `npm test`); adding a wildcard would over-grant.
        return trimmed;
    }
    let head = tokens[..2].join(" ");
    format!("{head} *")
}

pub(crate) fn load_rules(path: &Path) -> Vec<ApprovalRule> {
    fs::read_to_string(path)
        .ok()
        .map(|raw| raw.trim_start_matches('\u{feff}').to_string())
        .and_then(|raw| serde_json::from_str::<Vec<ApprovalRule>>(&raw).ok())
        .unwrap_or_default()
}

pub(crate) fn save_rules(path: &Path, rules: &[ApprovalRule]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建规则目录失败：{error}"))?;
    }
    let body = serde_json::to_string_pretty(rules).map_err(|error| error.to_string())?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, body).map_err(|error| format!("写入规则失败：{error}"))?;
    fs::rename(&temp, path).map_err(|error| format!("替换规则失败：{error}"))?;
    Ok(())
}

/// 追加一条审计；超过上限就重写文件只留最新 N 条。
pub(crate) fn append_audit(path: &Path, entry: &AuditEntry) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建审计目录失败：{error}"))?;
    }
    let line = serde_json::to_string(entry).map_err(|error| error.to_string())?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("打开审计文件失败：{error}"))?;
    writeln!(file, "{line}").map_err(|error| format!("写审计失败：{error}"))?;
    trim_audit(path);
    Ok(())
}

fn trim_audit(path: &Path) {
    let Ok(raw) = fs::read_to_string(path) else {
        return;
    };
    let lines: Vec<&str> = raw.lines().filter(|line| !line.trim().is_empty()).collect();
    if lines.len() <= MAX_AUDIT_ENTRIES {
        return;
    }
    let kept = &lines[lines.len() - MAX_AUDIT_ENTRIES..];
    let _ = fs::write(path, format!("{}\n", kept.join("\n")));
}

pub(crate) fn load_audit(path: &Path, limit: usize) -> Vec<AuditEntry> {
    let Ok(raw) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut entries: Vec<AuditEntry> = raw
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<AuditEntry>(line).ok())
        .collect();
    if entries.len() > limit {
        entries = entries.split_off(entries.len() - limit);
    }
    entries.reverse();
    entries
}

pub(crate) fn new_rule_id() -> String {
    format!("rule-{}", epoch_millis())
}

pub(crate) fn record_auto_allow(path: &Path, id: &str, tool: &str, command: &str, rule: &ApprovalRule) {
    let entry = AuditEntry {
        at_ms: epoch_millis(),
        id: id.to_string(),
        tool: tool.to_string(),
        command: command.to_string(),
        rule: if rule.id.is_empty() { rule.pattern.clone() } else { rule.id.clone() },
        decision: "auto_allow".into(),
    };
    let _ = append_audit(path, &entry);
}

#[tauri::command]
pub(crate) fn list_approval_rules() -> Vec<ApprovalRule> {
    load_rules(&rules_path())
}

#[tauri::command]
pub(crate) fn add_approval_rule(tool: String, pattern: String) -> Result<Vec<ApprovalRule>, String> {
    let pattern = pattern.trim().to_string();
    if pattern.is_empty() {
        return Err("命令模式不能为空".into());
    }
    if pattern.len() > 200 {
        return Err("命令模式太长".into());
    }
    let mut rules = load_rules(&rules_path());
    if rules.iter().any(|rule| rule.tool.eq_ignore_ascii_case(tool.trim()) && rule.pattern == pattern) {
        return Ok(rules);
    }
    rules.push(ApprovalRule { id: new_rule_id(), tool: tool.trim().to_string(), pattern });
    save_rules(&rules_path(), &rules)?;
    Ok(rules)
}

#[tauri::command]
pub(crate) fn remove_approval_rule(id: String) -> Result<Vec<ApprovalRule>, String> {
    let mut rules = load_rules(&rules_path());
    rules.retain(|rule| rule.id != id);
    save_rules(&rules_path(), &rules)?;
    Ok(rules)
}

#[tauri::command]
pub(crate) fn list_approval_audit(limit: Option<usize>) -> Vec<AuditEntry> {
    load_audit(&audit_path(), limit.unwrap_or(20).clamp(1, 200))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("agent-island-approval-{tag}-{}", epoch_millis()))
            .join("state.json")
    }

    fn rule(tool: &str, pattern: &str) -> ApprovalRule {
        ApprovalRule { id: format!("r-{pattern}"), tool: tool.into(), pattern: pattern.into() }
    }

    #[test]
    fn matches_literal_and_wildcard_patterns() {
        assert!(pattern_matches("npm test", "npm test -- --watch"));
        assert!(pattern_matches("npm *", "npm run build"));
        assert!(pattern_matches("git status*", "git status"));
        assert!(pattern_matches("*--dry-run", "rm -rf build --dry-run"));
        assert!(!pattern_matches("npm *", "sudo npm run build"));
        assert!(!pattern_matches("npm test", "npm run test"));
        assert!(!pattern_matches("", "anything"));
    }

    #[test]
    fn tolerates_a_utf8_bom_and_broken_json() {
        let path = temp_path("bom");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "\u{feff}[{\"id\":\"r1\",\"tool\":\"Bash\",\"pattern\":\"npm *\"}]").unwrap();
        assert_eq!(load_rules(&path).len(), 1, "PowerShell 的 UTF8 会带 BOM");
        fs::write(&path, "{not json").unwrap();
        assert!(load_rules(&path).is_empty());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn rules_respect_the_tool_filter() {
        let rules = vec![rule("Bash", "npm *"), rule("", "git status")];
        assert_eq!(match_rule(&rules, "Bash", "npm run build").unwrap().pattern, "npm *");
        assert!(match_rule(&rules, "Edit", "npm run build").is_none());
        assert_eq!(match_rule(&rules, "Edit", "git status").unwrap().pattern, "git status");
    }

    #[test]
    fn rules_round_trip_and_deduplicate() {
        let path = temp_path("rules");
        save_rules(&path, &[rule("Bash", "npm *")]).unwrap();
        assert_eq!(load_rules(&path).len(), 1);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn audit_keeps_only_the_latest_entries() {
        let path = temp_path("audit");
        for index in 0..(MAX_AUDIT_ENTRIES + 20) {
            append_audit(
                &path,
                &AuditEntry {
                    at_ms: index as u64,
                    id: format!("id-{index}"),
                    tool: "Bash".into(),
                    command: "npm test".into(),
                    rule: "r-npm".into(),
                    decision: "auto_allow".into(),
                },
            )
            .unwrap();
        }
        let raw = fs::read_to_string(&path).unwrap();
        assert_eq!(raw.lines().count(), MAX_AUDIT_ENTRIES);
        let latest = load_audit(&path, 3);
        assert_eq!(latest.len(), 3);
        assert_eq!(latest[0].id, format!("id-{}", MAX_AUDIT_ENTRIES + 19));
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn suggests_a_prefix_pattern_for_temporary_grants() {
        assert_eq!(suggest_rule_pattern("npm test -- --watch"), "npm test *");
        assert_eq!(suggest_rule_pattern("git status"), "git status");
        assert_eq!(suggest_rule_pattern("  npm   install  left-pad "), "npm install *");
        assert_eq!(suggest_rule_pattern(""), "");
    }

    #[test]
    fn audit_records_auto_allow_with_the_rule() {
        let path = temp_path("record");
        let matched = rule("Bash", "npm *");
        record_auto_allow(&path, "req-1", "Bash", "npm run build", &matched);
        let entries = load_audit(&path, 5);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].decision, "auto_allow");
        assert_eq!(entries[0].rule, "r-npm *");
        assert_eq!(entries[0].command, "npm run build");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}

