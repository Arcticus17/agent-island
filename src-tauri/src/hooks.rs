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

pub(crate) const HOOK_BRIDGE_SCRIPT: &str = r#"#!/usr/bin/env node
// Agent Island hook bridge (auto-generated, do not edit by hand)
import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

let cfg = null;
try {
  cfg = JSON.parse(readFileSync(join(homedir(), ".agent-island", "hook-config.json"), "utf8"));
} catch {
  process.exit(0);
}
const base = `http://127.0.0.1:${cfg.port}`;
const headers = { "content-type": "application/json", "x-agent-island-token": cfg.token };

let input = "";
process.stdin.on("data", (d) => (input += d));
process.stdin.on("end", async () => {
  try {
    const event = JSON.parse(input || "{}");
    const name = event.hook_event_name || "";
    if (name === "PreToolUse") {
      const tool = event.tool_name || "";
      const command = (event.tool_input && event.tool_input.command) || "";
      let decision = "ask";
      try {
        const res = await fetch(`${base}/event`, {
          method: "POST",
          headers,
          body: JSON.stringify({ name, tool, command, session: event.session_id || "", cwd: event.cwd || "" }),
        });
        const body = await res.json();
        if (body.decision === "allow" || body.decision === "deny" || body.decision === "ask") {
          decision = body.decision;
        } else if (body.id) {
          const id = body.id;
          const deadline = Date.now() + 120000;
          while (Date.now() < deadline) {
            await new Promise((r) => setTimeout(r, 300));
            try {
              const dr = await fetch(`${base}/decision?id=${encodeURIComponent(id)}`, { headers });
              const db = await dr.json();
              if (db.decision) { decision = db.decision; break; }
            } catch {}
          }
        }
      } catch {}
      process.stdout.write(JSON.stringify({
        hookSpecificOutput: {
          hookEventName: "PreToolUse",
          permissionDecision: decision,
          permissionDecisionReason: decision === "allow" ? "Agent Island 允许"
            : decision === "deny" ? "Agent Island 拒绝"
            : "Agent Island 未响应，回退原生确认",
        },
      }));
      process.exit(0);
    }
    if (name === "Stop") {
      try {
        await fetch(`${base}/event`, {
          method: "POST",
          headers,
          body: JSON.stringify({ name: "Stop", session: event.session_id || "", cwd: event.cwd || "" }),
        });
      } catch {}
      process.exit(0);
    }
    if (name === "Notification") {
      try {
        await fetch(`${base}/event`, {
          method: "POST",
          headers,
          body: JSON.stringify({ name: "Notification", message: event.message || "", session: event.session_id || "" }),
        });
      } catch {}
      process.exit(0);
    }
    process.exit(0);
  } catch {
    process.exit(0);
  }
});
"#;

pub(crate) fn classify_command(command: &str) -> bool {
    let mut c = command.trim().to_lowercase();
    if c.contains("&&") || c.contains("||") || c.contains(';') || c.contains('|') || c.contains('>')
    {
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
    let script = script_path.to_string_lossy().into_owned();
    let command = vec!["node".to_string(), script];
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
                list.retain(|entry| {
                    let is_ours = entry
                        .get("hooks")
                        .and_then(|h| h.as_array())
                        .and_then(|arr| arr.first())
                        .and_then(|h| h.get("command"))
                        .and_then(|c| c.as_array())
                        .map(|c| {
                            c.first().and_then(|x| x.as_str()) == Some("node")
                                && c.get(1)
                                    .and_then(|x| x.as_str())
                                    .map(|s| s.contains("agent-island"))
                                    .unwrap_or(false)
                        })
                        .unwrap_or(false);
                    !is_ours
                });
                let mut wrapper = serde_json::Map::new();
                if let Some(m) = matcher {
                    wrapper.insert("matcher".to_string(), serde_json::json!(m));
                }
                wrapper.insert("hooks".to_string(), serde_json::json!([hook_entry.clone()]));
                list.push(serde_json::Value::Object(wrapper));
            }
        };

    replace_entries(hooks_obj, "PreToolUse", Some("Bash"));
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
        for key in ["PreToolUse", "Stop", "Notification"] {
            if let Some(list) = hooks.get_mut(key).and_then(|l| l.as_array_mut()) {
                list.retain(|entry| {
                    let is_ours = entry
                        .get("hooks")
                        .and_then(|h| h.as_array())
                        .and_then(|arr| arr.first())
                        .and_then(|h| h.get("command"))
                        .and_then(|c| c.as_array())
                        .map(|c| {
                            c.first().and_then(|x| x.as_str()) == Some("node")
                                && c.get(1)
                                    .and_then(|x| x.as_str())
                                    .map(|s| s.contains("agent-island"))
                                    .unwrap_or(false)
                        })
                        .unwrap_or(false);
                    !is_ours
                });
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

#[derive(Debug, Clone, Serialize)]
pub(crate) struct HookApprovalPayload {
    id: String,
    session: String,
    tool: String,
    command: String,
    cwd: String,
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
        if all.len() > 65536 {
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
    if uri.starts_with("/event") {
        let event: Value = serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
        let name = event.get("name").and_then(|n| n.as_str()).unwrap_or("");
        if name == "PreToolUse" {
            let command = event
                .get("command")
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .to_string();
            if !enabled {
                write_http_json(&mut stream, "200 OK", "{\"decision\":\"ask\"}");
                return;
            }
            if classify_command(&command) {
                write_http_json(&mut stream, "200 OK", "{\"decision\":\"allow\"}");
                return;
            }
            let id = new_hook_approval_id();
            let approval = HookApproval {
                id: id.clone(),
                session: event
                    .get("session")
                    .and_then(|s| s.as_str())
                    .unwrap_or("")
                    .to_string(),
                tool: event
                    .get("tool")
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
                    .to_string(),
                command: command.clone(),
                cwd: event
                    .get("cwd")
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .to_string(),
                decision: None,
            };
            let payload = HookApprovalPayload {
                id: id.clone(),
                session: approval.session.clone(),
                tool: approval.tool.clone(),
                command: command.clone(),
                cwd: approval.cwd.clone(),
            };
            {
                let mut approvals = state.approvals.lock_recover();
                approvals.retain(|_, a| a.decision.is_none() && a.id != id);
                while approvals.len() > 5 {
                    let oldest = approvals
                        .values()
                        .min_by_key(|a| a.id.clone())
                        .map(|a| a.id.clone());
                    if let Some(k) = oldest {
                        approvals.remove(&k);
                    } else {
                        break;
                    }
                }
                approvals.insert(id.clone(), approval);
            }
            use tauri::Emitter;
            let _ = app.emit("hook-approval", &payload);
            write_http_json(
                &mut stream,
                "200 OK",
                &serde_json::json!({ "decision": "pending", "id": id }).to_string(),
            );
        } else {
            use tauri::Emitter;
            if name == "Stop" {
                let _ = app.emit(
                    "hook-event",
                    serde_json::json!({
                        "kind": "stop",
                        "session": event.get("session").and_then(|s| s.as_str()).unwrap_or(""),
                    }),
                );
            } else if name == "Notification" {
                let _ = app.emit(
                    "hook-event",
                    serde_json::json!({
                        "kind": "notification",
                        "message": event.get("message").and_then(|m| m.as_str()).unwrap_or(""),
                        "session": event.get("session").and_then(|s| s.as_str()).unwrap_or(""),
                    }),
                );
            }
            write_http_json(&mut stream, "200 OK", "{}");
        }
    } else if uri.starts_with("/decision") {
        let id = uri.split("?id=").nth(1).map(|s| s.to_string());
        let decision = id.and_then(|id| {
            state
                .approvals
                .lock()
                .ok()
                .and_then(|approvals| approvals.get(&id).and_then(|a| a.decision.clone()))
        });
        let json = match decision {
            Some(d) => serde_json::json!({ "decision": d }).to_string(),
            None => "{\"decision\":null}".to_string(),
        };
        write_http_json(&mut stream, "200 OK", &json);
    } else {
        write_http_json(&mut stream, "404 Not Found", "{}");
    }
}

pub(crate) fn start_hook_server(app: tauri::AppHandle) {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
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
    state: tauri::State<AppState>,
) -> Result<(), String> {
    let mut approvals = state
        .approvals
        .lock()
        .map_err(|_| "state lock error".to_string())?;
    set_hook_approval_decision(&mut approvals, &id, if allow { "allow" } else { "deny" })
}

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
    state: tauri::State<AppState>,
) -> Result<(), String> {
    let mut approvals = state
        .approvals
        .lock()
        .map_err(|_| "state lock error".to_string())?;
    if let Some(approval) = approvals.get_mut(&id) {
        approval.decision = Some("ask".to_string());
    }
    Ok(())
}
