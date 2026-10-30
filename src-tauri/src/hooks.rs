//! Claude hooks bridge: settings merge, local event server, and approval cards.

use crate::*;

pub(crate) const HOOK_PORT: u16 = 8799;

pub(crate) fn hook_dir() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".agent-island")
}

pub(crate) fn hook_config_path() -> PathBuf {
    hook_dir().join("hook-config.json")
}

pub(crate) fn hook_script_path() -> PathBuf {
    hook_dir().join("hook-bridge.mjs")
}

pub(crate) const HOOK_BRIDGE_SCRIPT: &str = include_str!("hook_bridge.mjs");

pub(crate) fn classify_command(command: &str) -> bool {
    // 只读验证经常写成 `find … 2>/dev/null; echo …`。把无害的
    // "丢弃 stderr" 归一化掉，再按 && / ; 拆成单条命令逐段判断；
    // 真正的写文件、管道、命令替换仍然整体拒绝。
    let cleaned = command
        .trim()
        .to_lowercase()
        .replace("2>/dev/null", "")
        .replace("2>nul", "")
        .replace(">/dev/null", "")
        .replace("2>&1", "");
    if ["|", "`", "$(", ">", "<"].iter().any(|token| cleaned.contains(token)) {
        return false;
    }
    let segments: Vec<&str> = cleaned
        .split("&&")
        .flat_map(|part| part.split(';'))
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect();
    if segments.is_empty() {
        return false;
    }
    segments.iter().all(|segment| classify_single_command(segment))
}

fn classify_single_command(command: &str) -> bool {
    let mut c = command.trim().to_lowercase();
    if c.is_empty() {
        return false;
    }
    c = c
        .strip_prefix("cmd /c")
        .or_else(|| c.strip_prefix("cmd.exe /c"))
        .or_else(|| c.strip_prefix("powershell -command"))
        .or_else(|| c.strip_prefix("powershell.exe -command"))
        .unwrap_or(&c)
        .trim()
        .to_string();
    let (exe, args) = if let Some(rest) = c.strip_prefix('"') {
        match rest.find('"') {
            Some(end) => (&rest[..end], rest[end + 1..].trim_start()),
            None => (c.as_str(), ""),
        }
    } else {
        match c.find(char::is_whitespace) {
            Some(i) => (&c[..i], c[i..].trim_start()),
            None => (c.as_str(), ""),
        }
    };
    let base = exe
        .trim_matches('"')
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(exe);
    let base = base
        .strip_suffix(".exe")
        .or_else(|| base.strip_suffix(".cmd"))
        .unwrap_or(base);
    let full = if args.is_empty() {
        base.to_string()
    } else {
        format!("{base} {args}")
    };
    let c = full.as_str();
    if c.is_empty() {
        return true;
    }
    const SAFE_PREFIX: &[&str] = &[
        "ls ",
        "dir ",
        "cat ",
        "type ",
        "get-content ",
        "gc ",
        "get-childitem",
        "gci",
        "test-path",
        "get-item ",
        "select-string ",
        "measure-object ",
        "resolve-path ",
        "get-filehash ",
        "od ",
        "stat ",
        "file ",
        "echo ",
        "pwd",
        "cd ",
        "chdir ",
        "where ",
        "which ",
        "rg ",
        "grep ",
        "find ",
        "findstr ",
        "git status",
        "git log",
        "git diff",
        "git branch",
        "git remote",
        "git show",
        "git rev-parse",
        "git worktree list",
        "git config --get",
        "printf ",
        "git --version",
        "git -v",
        "node --version",
        "node -v",
        "npm --version",
        "npm -v",
        "python --version",
        "py --version",
        "pip --version",
        "pip list",
        "cargo --version",
        "rustc --version",
        "code --version",
        "codex --version",
        "claude --version",
        "date",
        "time ",
        "whoami",
        "hostname",
        "tasklist",
        "ver",
        "set ",
        "env",
        "sort ",
        "head ",
        "tail ",
        "wc ",
        "touch ",
        "test -",
        "make -n",
        "nix --version",
    ];
    SAFE_PREFIX.iter().any(|p| c.starts_with(p))
}

/// 判断一条 hook 配置是不是 Agent Island 自己装的。
///
/// Claude Code 2.1.x 只执行字符串形式的 `command`；数组形式会被静默忽略。
/// 这里同时识别两种形态，保证旧版本安装的数组 hook 在重新安装/卸载时不会变成孤儿。
fn hook_entry_is_ours(entry: &Value) -> bool {
    let Some(hooks) = entry.get("hooks").and_then(Value::as_array) else {
        return false;
    };
    hooks.iter().any(|hook| match hook.get("command") {
        Some(Value::String(command)) => {
            command.contains("agent-island") && command.contains("hook-bridge")
        }
        Some(Value::Array(parts)) => {
            parts.first().and_then(Value::as_str) == Some("node")
                && parts
                    .get(1)
                    .and_then(Value::as_str)
                    .is_some_and(|script| script.contains("agent-island"))
        }
        _ => false,
    })
}

pub(crate) fn merge_hooks_settings(path: &Path, script_path: &Path) -> Result<(), String> {
    let text = if path.is_file() {
        fs::read_to_string(path).map_err(|e| e.to_string())?
    } else {
        "{}".to_string()
    };
    let mut root: Value = if text.trim().is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(&text).unwrap_or_else(|_| serde_json::json!({}))
    };
    let script = script_path.to_string_lossy();
    // 字符串形式才是 Claude Code 2.1.x 认可的 hook command；数组形式会被静默忽略。
    let command = format!("node \"{script}\"");
    let hook_entry = serde_json::json!({ "type": "command", "command": command });

    let hooks = root
        .as_object_mut()
        .ok_or_else(|| "settings.json 不是对象".to_string())?
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));
    let hooks_obj = hooks
        .as_object_mut()
        .ok_or_else(|| "hooks 不是对象".to_string())?;

    let replace_entries =
        |obj: &mut serde_json::Map<String, Value>, key: &str, matcher: Option<&str>| {
            let entries = obj
                .entry(key.to_string())
                .or_insert_with(|| serde_json::json!([]));
            if let Some(list) = entries.as_array_mut() {
                list.retain(|entry| !hook_entry_is_ours(entry));
                let mut wrapper = serde_json::Map::new();
                if let Some(m) = matcher {
                    wrapper.insert("matcher".to_string(), serde_json::json!(m));
                }
                wrapper.insert("hooks".to_string(), serde_json::json!([hook_entry.clone()]));
                list.push(serde_json::Value::Object(wrapper));
            }
        };

    replace_entries(hooks_obj, "PreToolUse", Some("Bash"));
    // PermissionRequest fires only when Claude is about to ask the user, so
    // Edit/Write previews do not turn every in-project edit into a prompt.
    replace_entries(hooks_obj, "PermissionRequest", Some("Edit|Write|MultiEdit"));
    replace_entries(hooks_obj, "Stop", None);
    replace_entries(hooks_obj, "Notification", None);

    let data = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    fs::write(path, data).map_err(|e| e.to_string())
}

pub(crate) fn remove_hooks_settings(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Ok(());
    }
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut root: Value = if text.trim().is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(&text).unwrap_or_else(|_| serde_json::json!({}))
    };
    if let Some(hooks) = root.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        for key in [
            "PreToolUse",
            "PermissionRequest",
            "Stop",
            "Notification",
        ] {
            if let Some(list) = hooks.get_mut(key).and_then(|l| l.as_array_mut()) {
                list.retain(|entry| !hook_entry_is_ours(entry));
                if list.is_empty() {
                    hooks.remove(key);
                }
            }
        }
        if hooks.is_empty() {
            if let Some(obj) = root.as_object_mut() {
                obj.remove("hooks");
            }
        }
    }
    let data = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    fs::write(path, data).map_err(|e| e.to_string())
}

pub(crate) fn claude_settings_path() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".claude").join("settings.json"))
}

pub(crate) fn install_hooks(state: &AppState) -> Result<(), String> {
    let dir = hook_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let token = format!(
        "{:x}{:x}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let cfg = HookConfigFile {
        port: HOOK_PORT,
        token: token.clone(),
        enabled: true,
    };
    let cfg_data = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    fs::write(hook_config_path(), cfg_data).map_err(|e| e.to_string())?;
    fs::write(hook_script_path(), HOOK_BRIDGE_SCRIPT).map_err(|e| e.to_string())?;

    let settings_path = claude_settings_path().ok_or_else(|| "无法定位用户目录".to_string())?;
    let backup = settings_path.with_extension("json.agent-island.bak");
    if settings_path.is_file() && !backup.is_file() {
        fs::copy(&settings_path, &backup).map_err(|e| e.to_string())?;
    }
    merge_hooks_settings(&settings_path, &hook_script_path())?;

    *state.hook_token.lock_recover() = token;
    state.hook_enabled.store(true, Ordering::SeqCst);
    Ok(())
}

pub(crate) fn uninstall_hooks(state: &AppState) -> Result<(), String> {
    if let Some(settings_path) = claude_settings_path() {
        remove_hooks_settings(&settings_path)?;
    }
    if let Some(mut cfg) = load_hook_config() {
        cfg.enabled = false;
        let data = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
        fs::write(hook_config_path(), data).map_err(|e| e.to_string())?;
    }
    state.hook_enabled.store(false, Ordering::SeqCst);
    Ok(())
}

/// Refresh the generated bridge script and merge the current hook entries
/// without rotating the token. Called at startup for existing installs so a
/// new event (PermissionRequest) reaches them without a manual toggle.
pub(crate) fn refresh_installed_hooks() {
    let Some(settings_path) = claude_settings_path() else {
        return;
    };
    if !settings_path.is_file() {
        return;
    }
    // Only refresh an install that already references our bridge. If the user
    // removed the hooks by hand (while the config still says enabled), do not
    // silently put them back.
    let already_installed = fs::read_to_string(&settings_path)
        .map(|text| text.contains("hook-bridge.mjs"))
        .unwrap_or(false);
    if !already_installed {
        return;
    }
    let dir = hook_dir();
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    if fs::write(hook_script_path(), HOOK_BRIDGE_SCRIPT).is_err() {
        return;
    }
    let _ = merge_hooks_settings(&settings_path, &hook_script_path());
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct HookApprovalPayload {
    id: String,
    session: String,
    tool: String,
    command: String,
    cwd: String,
    rule_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    risk: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    risk_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    repo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diff: Option<DiffPreview>,
    created_at_ms: u64,
    expires_at_ms: u64,
}

/// In-memory grant for "allow for 10 minutes" / "allow this session".
#[derive(Debug, Clone)]
pub(crate) struct TemporaryGrant {
    pub(crate) tool: String,
    pub(crate) pattern: String,
    pub(crate) session: Option<String>,
    pub(crate) expires_at_ms: u64,
}

pub(crate) fn grant_matches(
    grants: &mut Vec<TemporaryGrant>,
    tool: &str,
    command: &str,
    session: &str,
    now_ms: u64,
) -> bool {
    grants.retain(|grant| grant.expires_at_ms > now_ms);
    grants.iter().any(|grant| {
        let tool_ok = grant.tool.trim().is_empty() || grant.tool.eq_ignore_ascii_case(tool.trim());
        let session_ok = grant
            .session
            .as_deref()
            .map(|value| value.is_empty() || value == session)
            .unwrap_or(true);
        tool_ok && session_ok && pattern_matches(&grant.pattern, command)
    })
}

pub(crate) fn new_hook_approval_id() -> String {
    format!(
        "a{:x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    )
}

pub(crate) fn read_http_request(
    stream: &mut std::net::TcpStream,
) -> Option<(String, String, String)> {
    use std::io::Read;
    let mut all = Vec::new();
    let mut buf = [0u8; 8192];
    let (uri, token, content_length) = loop {
        let n = stream.read(&mut buf).ok()?;
        if n == 0 {
            return None;
        }
        all.extend_from_slice(&buf[..n]);
        if all.len() > 256 * 1024 {
            return None;
        }
        let text = String::from_utf8_lossy(&all);
        if let Some(idx) = text.find("\r\n\r\n") {
            let headers = text[..idx].to_string();
            let first = headers.lines().next().unwrap_or("").to_string();
            let uri = first.split_whitespace().nth(1).unwrap_or("/").to_string();
            // HTTP 头名大小写不敏感：Node 的 undici 发小写，curl/其他客户端发
            // `Content-Length` 这种规范写法。只认小写会让别的 agent 的 hook
            // 静默丢掉 body（审批永远不弹），所以这里统一按不区分大小写匹配。
            let header = |name: &str| -> Option<String> {
                headers.lines().find_map(|line| {
                    let (key, value) = line.split_once(':')?;
                    if key.trim().eq_ignore_ascii_case(name) {
                        Some(value.trim().to_string())
                    } else {
                        None
                    }
                })
            };
            let token = header("x-agent-island-token").unwrap_or_default();
            let content_length: usize = header("content-length")
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(0);
            let body_start = idx + 4;
            if all.len() - body_start >= content_length {
                let body = String::from_utf8_lossy(&all[body_start..body_start + content_length])
                    .into_owned();
                break (uri, token, body);
            }
        }
    };
    Some((uri, token, content_length))
}

pub(crate) fn write_http_json(stream: &mut std::net::TcpStream, status: &str, body: &str) {
    use std::io::Write;
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.flush();
}

fn short_session(session: &str) -> String {
    if session.chars().count() <= 8 {
        session.to_string()
    } else {
        session.chars().take(8).collect()
    }
}

fn session_body(session: &str) -> String {
    if session.is_empty() {
        "会话未知".to_string()
    } else {
        format!("会话 {} 已结束", short_session(session))
    }
}

fn notification_dedupe(session: &str, message: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(message.as_bytes());
    format!("notification:{session}:{:x}", digest)
}

fn approval_payload(approval: &HookApproval) -> HookApprovalPayload {
    let review = approval.review.as_ref();
    HookApprovalPayload {
        id: approval.id.clone(),
        session: approval.session.clone(),
        tool: approval.tool.clone(),
        command: approval.command.clone(),
        cwd: approval.cwd.clone(),
        rule_key: approval.rule_key.clone(),
        risk: review.map(|value| match value.risk {
            RiskLevel::Low => "low".to_string(),
            RiskLevel::Medium => "medium".to_string(),
            RiskLevel::High => "high".to_string(),
        }),
        risk_reason: review.map(|value| value.reason.clone()),
        repo: review.and_then(|value| value.repo.clone()),
        branch: review.and_then(|value| value.branch.clone()),
        diff: review.and_then(|value| value.diff.clone()),
        created_at_ms: approval.created_at_ms,
        expires_at_ms: approval.expires_at_ms,
    }
}

fn record_approval_inbox(
    app: &tauri::AppHandle,
    state: &AppState,
    approval: &HookApproval,
) {
    let severity = approval
        .review
        .as_ref()
        .map(|review| review.risk.severity())
        .unwrap_or("normal");
    let tool = if approval.tool.trim().is_empty() {
        "未知工具".to_string()
    } else {
        approval.tool.clone()
    };
    let title = format!("需要审批 · {tool}");
    let body = if approval.command.trim().is_empty() {
        approval.cwd.clone()
    } else {
        approval.command.clone()
    };
    let item = InboxItem::new(
        "approval",
        severity,
        "claude",
        "Claude Code",
        &approval.session,
        &title,
        &body,
        &format!("approval:{}", approval.id),
    )
    .with_approval_id(&approval.id);
    push_inbox(app, state, item);
}

fn create_approval(
    app: &tauri::AppHandle,
    state: &AppState,
    session: String,
    tool: String,
    command: String,
    cwd: String,
    rule_key: String,
    review: ApprovalReview,
    team_task: Option<(String, String)>,
    ttl_ms: u64,
) -> String {
    let id = new_hook_approval_id();
    let now = epoch_millis();
    let (run_id, task_id) = match team_task {
        Some((run_id, task_id)) => (Some(run_id), Some(task_id)),
        None => (None, None),
    };
    let approval = HookApproval {
        id: id.clone(),
        session,
        tool,
        command,
        cwd,
        rule_key,
        decision: None,
        reason: None,
        review: Some(review),
        run_id,
        task_id,
        created_at_ms: now,
        expires_at_ms: now.saturating_add(ttl_ms),
    };
    let payload = approval_payload(&approval);
    {
        let mut approvals = state.approvals.lock_recover();
        approvals.retain(|_, value| value.decision.is_none() && value.id != id);
        while approvals.len() > 5 {
            let oldest = approvals
                .values()
                .min_by_key(|value| value.created_at_ms)
                .map(|value| value.id.clone());
            if let Some(key) = oldest {
                approvals.remove(&key);
            } else {
                break;
            }
        }
        approvals.insert(id.clone(), approval.clone());
    }
    use tauri::Emitter;
    // Persist the inbox row before the card event: a fast click on "allow"
    // must always find an item to resolve.
    record_approval_inbox(app, state, &approval);
    let _ = app.emit("hook-approval", &payload);
    id
}

fn tool_display(tool: &str, command: &str, input: &Value, review: &ApprovalReview) -> String {
    if !command.trim().is_empty() {
        return command.to_string();
    }
    let file = input
        .get("file_path")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if !file.is_empty() {
        let diff = review
            .diff
            .as_ref()
            .map(|value| format!(" (+{} −{})", value.added, value.removed))
            .unwrap_or_default();
        return format!("{file}{diff}");
    }
    if tool.trim().is_empty() {
        "未提供命令或文件".to_string()
    } else {
        tool.to_string()
    }
}

fn rule_key_for(tool: &str, input: &Value, display: &str) -> String {
    let lower = tool.trim().to_ascii_lowercase();
    if matches!(lower.as_str(), "edit" | "write" | "multiedit") {
        return input
            .get("file_path")
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| display.to_string());
    }
    display.to_string()
}

pub(crate) fn handle_hook_conn(mut stream: std::net::TcpStream, app: tauri::AppHandle) {
    let Some((uri, token, body)) = read_http_request(&mut stream) else {
        return;
    };
    let state = app.state::<AppState>();
    let valid = !token.is_empty() && *state.hook_token.lock_recover() == token;
    if !valid {
        write_http_json(&mut stream, "403 Forbidden", "{}");
        return;
    }
    let enabled = state.hook_enabled.load(Ordering::SeqCst);
    let event: Value = serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
    // 任何 hook 都带着 Agent 自报的 session/cwd：记下来，供快照判定"已确认"。
    {
        let session = event.get("session").and_then(|s| s.as_str()).unwrap_or("");
        let cwd = event.get("cwd").and_then(|c| c.as_str());
        let mut hooked = state.hooked.lock_recover();
        record_hooked(&mut hooked, "claude", session, cwd, epoch_millis());
    }

    if uri.starts_with("/event") {
        let name = event.get("name").and_then(|n| n.as_str()).unwrap_or("");
        let tool = event
            .get("tool")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();
        let command = event
            .get("command")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();
        let session_id = event
            .get("session")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();
        let cwd = event
            .get("cwd")
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();
        let mut tool_input = event
            .get("tool_input")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        // Older bridges (and simple external clients) send only the top-level
        // command. Keep risk assessment working for them by folding it back
        // into the tool input.
        if !command.is_empty()
            && tool_input
                .get("command")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .is_empty()
        {
            if let Some(object) = tool_input.as_object_mut() {
                object.insert("command".to_string(), serde_json::json!(command.clone()));
            }
        }

        if name == "PreToolUse" || name == "PermissionRequest" {
            if !enabled {
                write_http_json(&mut stream, "200 OK", "{\"decision\":\"ask\"}");
                return;
            }
            // 团队任务：只读命令和文件编辑自动放行；危险命令继续走人工审批，
            // 但任务会进入"等待审批"，小岛上的审批卡负责真正的决定。
            let team_task = state.team.task_for_session(&session_id);
            if team_task.is_some() {
                let file_edit =
                    matches!(tool.as_str(), "Edit" | "Write" | "MultiEdit" | "NotebookEdit");
                if file_edit {
                    let file_path = tool_input
                        .get("file_path")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let inside = state
                        .team
                        .session_worktree(&session_id)
                        .is_some_and(|root| crate::orchestrator::worktree::path_within(&root, file_path));
                    if inside {
                        write_http_json(
                            &mut stream,
                            "200 OK",
                            &serde_json::json!({
                                "decision": "allow",
                                "reason": "团队任务：文件在 worktree 内，自动放行",
                            })
                            .to_string(),
                        );
                        return;
                    }
                    write_http_json(
                        &mut stream,
                        "200 OK",
                        &serde_json::json!({
                            "decision": "deny",
                            "reason": "团队任务只能修改自己的 worktree 内的文件；主工作区不被允许",
                        })
                        .to_string(),
                    );
                    return;
                }
                if classify_command(&command) {
                    write_http_json(
                        &mut stream,
                        "200 OK",
                        &serde_json::json!({
                            "decision": "allow",
                            "reason": "团队任务：只读命令自动放行",
                        })
                        .to_string(),
                    );
                    return;
                }
                log_event(
                    "INFO",
                    "hook",
                    &format!(
                        "team approval needed tool={tool} session={session_id} len={}",
                        command.len()
                    ),
                );
            }
            let review = review_tool(if tool.is_empty() { "Bash" } else { &tool }, &tool_input, &cwd);
            let display = tool_display(&tool, &command, &tool_input, &review);

            // Persistent allow rules apply to both PreToolUse and
            // PermissionRequest. PermissionRequest is where Edit/Write show up.
            let rules = load_rules(&rules_path());
            if let Some(rule) = match_rule(&rules, &tool, &display) {
                let id = new_hook_approval_id();
                record_auto_allow(&audit_path(), &id, &tool, &display, rule);
                let program = display.split_whitespace().next().unwrap_or("");
                log_event(
                    "INFO",
                    "hook",
                    &format!(
                        "auto-allowed tool={tool} rule={} program={program} len={}",
                        rule.id,
                        display.len()
                    ),
                );
                use tauri::Emitter;
                let _ = app.emit(
                    "hook-event",
                    serde_json::json!({
                        "kind": "notification",
                        "message": format!("已按规则自动允许：{display}"),
                        "session": session_id,
                    }),
                );
                write_http_json(&mut stream, "200 OK", "{\"decision\":\"allow\"}");
                return;
            }

            // Temporary grants are memory-only and expire automatically.
            if grant_matches(
                &mut state.approvals_grants.lock_recover(),
                &tool,
                &display,
                &session_id,
                epoch_millis(),
            ) {
                write_http_json(&mut stream, "200 OK", "{\"decision\":\"allow\"}");
                return;
            }

            // PreToolUse already had an explicit low-risk allowlist; keep the
            // cheap path for Bash without a review round-trip.
            if name == "PreToolUse" && classify_command(&command) {
                write_http_json(&mut stream, "200 OK", "{\"decision\":\"allow\"}");
                return;
            }

            let rule_key = rule_key_for(&tool, &tool_input, &display);
            let approval_command = display.clone();
            let ttl_ms = if team_task.is_some() { 5 * 60_000 } else { 120_000 };
            let id = create_approval(
                &app,
                &state,
                session_id,
                tool,
                display,
                cwd,
                rule_key,
                review,
                team_task.clone(),
                ttl_ms,
            );
            if let Some((run_id, task_id)) = team_task {
                if let Some(view) = state.team.begin_task_approval(
                    &run_id,
                    &task_id,
                    &id,
                    &approval_command,
                ) {
                    use tauri::Emitter;
                    let _ = app.emit("team-run", &view);
                }
            }
            write_http_json(
                &mut stream,
                "200 OK",
                &serde_json::json!({ "decision": "pending", "id": id }).to_string(),
            );
            return;
        }

        use tauri::Emitter;
        if name == "Stop" {
            let _ = app.emit(
                "hook-event",
                serde_json::json!({
                    "kind": "stop",
                    "session": session_id,
                }),
            );
            let item = InboxItem::new(
                "done",
                "normal",
                "claude",
                "Claude Code",
                &session_id,
                "任务完成",
                &session_body(&session_id),
                &format!("done:{session_id}"),
            );
            push_inbox(&app, &state, item);
        } else if name == "Notification" {
            let message = event
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("");
            let _ = app.emit(
                "hook-event",
                serde_json::json!({
                    "kind": "notification",
                    "message": message,
                    "session": session_id,
                }),
            );
            let item = InboxItem::new(
                "notification",
                "normal",
                "claude",
                "Claude Code",
                &session_id,
                "Claude 通知",
                message,
                &notification_dedupe(&session_id, message),
            );
            push_inbox(&app, &state, item);
        }
        write_http_json(&mut stream, "200 OK", "{}");
        return;
    }

    if uri.starts_with("/decision") {
        let id = uri.split("?id=").nth(1).map(|s| s.to_string());
        let result = id.and_then(|id| {
            let approvals = state.approvals.lock_recover();
            approvals.get(&id).and_then(|approval| {
                approval.decision.as_ref().map(|decision| {
                    (decision.clone(), approval.reason.clone())
                })
            })
        });
        let json = match result {
            Some((decision, reason)) => serde_json::json!({
                "decision": decision,
                "reason": reason.unwrap_or_default(),
            })
            .to_string(),
            None => "{\"decision\":null}".to_string(),
        };
        write_http_json(&mut stream, "200 OK", &json);
        return;
    }

    write_http_json(&mut stream, "404 Not Found", "{}");
}

pub(crate) fn start_hook_server(app: tauri::AppHandle) {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let cleanup_app = app.clone();
    std::thread::spawn(move || {
        let listener = match std::net::TcpListener::bind(("127.0.0.1", HOOK_PORT)) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("hook server failed to bind: {e}");
                return;
            }
        };
        for stream in listener.incoming().flatten() {
            let app = app.clone();
            std::thread::spawn(move || handle_hook_conn(stream, app));
        }
    });

    // The bridge gives up after 120s and falls back to Claude's native prompt.
    // Remove the corresponding island card/inbox row at the same deadline so a
    // stale "needs approval" is never shown as if it were still actionable.
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(15));
        let state = cleanup_app.state::<AppState>();
        let now = epoch_millis();
        let expired: Vec<(String, Option<String>, Option<String>)> = {
            let mut approvals = state.approvals.lock_recover();
            let entries: Vec<(String, Option<String>, Option<String>)> = approvals
                .values()
                .filter(|approval| {
                    approval.decision.is_none() && approval.expires_at_ms <= now
                })
                .map(|approval| {
                    (
                        approval.id.clone(),
                        approval.run_id.clone(),
                        approval.task_id.clone(),
                    )
                })
                .collect();
            for (id, run_id, _) in &entries {
                if let Some(approval) = approvals.get_mut(id) {
                    // 团队任务没有"回退原生确认"可退：超时直接拒绝，
                    // 让 worker 明确失败，而不是继续挂在 print 模式里。
                    approval.decision = Some(if run_id.is_some() { "deny" } else { "ask" }.to_string());
                }
            }
            entries
        };
        if expired.is_empty() {
            continue;
        }
        let ids: Vec<String> = expired.iter().map(|(id, _, _)| id.clone()).collect();
        for (id, run_id, task_id) in &expired {
            resolve_approval_inbox(&cleanup_app, &state, id);
            if let (Some(run_id), Some(task_id)) = (run_id, task_id) {
                if let Some(view) =
                    state
                        .team
                        .fail_task_approval(run_id, task_id, "审批超时，已按拒绝处理")
                {
                    use tauri::Emitter;
                    let _ = cleanup_app.emit("team-run", &view);
                }
            }
        }
        use tauri::Emitter;
        let _ = cleanup_app.emit("hook-approval-expired", serde_json::json!({ "ids": ids }));
    });
}

#[tauri::command]
pub(crate) fn get_hook_status(state: tauri::State<AppState>) -> bool {
    state.hook_enabled.load(Ordering::SeqCst)
}

#[tauri::command]
pub(crate) fn set_hook_enabled(
    enabled: bool,
    state: tauri::State<AppState>,
) -> Result<String, String> {
    if enabled {
        install_hooks(&state)?;
        Ok("已接入 Claude hooks".to_string())
    } else {
        uninstall_hooks(&state)?;
        Ok("已断开 Claude hooks".to_string())
    }
}

#[tauri::command]
pub(crate) fn respond_hook_approval(
    id: String,
    allow: bool,
    scope: Option<String>,
    reason: Option<String>,
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    let now = epoch_millis();
    let (tool, rule_key, session, run_id, task_id) = {
        let approvals = state.approvals.lock_recover();
        let approval = approvals
            .get(&id)
            .ok_or_else(|| "approval_not_found".to_string())?;
        (
            approval.tool.clone(),
            approval.rule_key.clone(),
            approval.session.clone(),
            approval.run_id.clone(),
            approval.task_id.clone(),
        )
    };

    if allow {
        if let Some(scope) = scope.as_deref() {
            let (expires_at_ms, session_scope) = match scope {
                "ten_minutes" => (now.saturating_add(10 * 60_000), None),
                "session" => (now.saturating_add(8 * 60 * 60_000), Some(session.clone())),
                _ => (0, None),
            };
            if expires_at_ms > now {
                state.approvals_grants.lock_recover().push(TemporaryGrant {
                    tool: tool.clone(),
                    pattern: suggest_rule_pattern(&rule_key),
                    session: session_scope,
                    expires_at_ms,
                });
            }
        }
    }

    {
        let mut approvals = state.approvals.lock_recover();
        let approval = approvals
            .get_mut(&id)
            .ok_or_else(|| "approval_not_found".to_string())?;
        approval.decision = Some(if allow { "allow" } else { "deny" }.to_string());
        approval.reason = reason.filter(|value| !value.trim().is_empty());
    }
    resolve_approval_inbox(&app, &state, &id);
    if let (Some(run_id), Some(task_id)) = (run_id, task_id) {
        if let Some(view) = state.team.finish_task_approval(&run_id, &task_id) {
            use tauri::Emitter;
            let _ = app.emit("team-run", &view);
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn set_hook_approval_decision(
    approvals: &mut HashMap<String, HookApproval>,
    id: &str,
    decision: &str,
) -> Result<(), String> {
    let approval = approvals
        .get_mut(id)
        .ok_or_else(|| "approval_not_found".to_string())?;
    approval.decision = Some(decision.to_string());
    Ok(())
}

#[tauri::command]
pub(crate) fn dismiss_hook_approval(
    id: String,
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    {
        let mut approvals = state.approvals.lock_recover();
        if let Some(approval) = approvals.get_mut(&id) {
            approval.decision = Some("ask".to_string());
        }
    }
    resolve_approval_inbox(&app, &state, &id);
    Ok(())
}


