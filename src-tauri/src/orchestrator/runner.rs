//! 单个 Claude 无头进程的启动、事件解析、超时与进程树清理。
//!
//! 只认 Claude 自己的结构化事件：`assistant` 里的 tool_use、`user` 里的
//! tool_result、最后的 `result`。stderr 只作为错误说明，不参与状态判定。

use crate::*;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(crate) struct RunnerConfig<'a> {
    pub program: &'a Path,
    pub cwd: &'a Path,
    pub prompt: &'a str,
    pub settings: Option<&'a Path>,
    pub session_id: &'a str,
    pub permission_mode: &'a str,
    pub max_turns: u32,
    pub timeout: Duration,
    /// "json"（一次性结果，适合规划）或 "stream-json"（带过程，适合 worker）。
    pub output_format: &'a str,
    pub json_schema: Option<&'a str>,
    /// Claude print 模式的单进程 USD 上限；None 表示不设。
    pub max_budget_usd: Option<f64>,
    pub env: &'a HashMap<String, String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ProcessOutcome {
    pub ok: bool,
    pub result: Option<String>,
    pub structured: Option<Value>,
    pub error: Option<String>,
    pub exit_code: Option<i32>,
    pub cost_usd: Option<f64>,
    pub session_id: String,
    pub log: Vec<String>,
    pub timed_out: bool,
}

#[derive(Default)]
struct Parsed {
    result: Option<String>,
    structured: Option<Value>,
    is_error: bool,
    cost_usd: Option<f64>,
    session_id: Option<String>,
    terminal_reason: Option<String>,
    log: Vec<String>,
}

fn cap_text(text: &str, limit: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= limit {
        trimmed.to_string()
    } else {
        let mut value: String = trimmed.chars().take(limit).collect();
        value.push('…');
        value
    }
}

fn push_log(log: &mut Vec<String>, line: impl Into<String>) {
    if log.len() >= 160 {
        return;
    }
    let line = cap_text(&line.into(), 260);
    if !line.is_empty() {
        log.push(line);
    }
}

fn tool_detail(name: &str, input: &Value) -> String {
    let field = match name {
        "Bash" => "command",
        "Write" | "Edit" | "MultiEdit" | "Read" | "NotebookEdit" => "file_path",
        _ => "",
    };
    if !field.is_empty() {
        if let Some(value) = input.get(field).and_then(Value::as_str) {
            return cap_text(value, 180);
        }
    }
    cap_text(&input.to_string(), 180)
}

fn parse_stream_line(line: &str, parsed: &mut Parsed) {
    let trimmed = line.trim();
    if !trimmed.starts_with('{') {
        return;
    }
    let Ok(event) = serde_json::from_str::<Value>(trimmed) else {
        return;
    };
    match event.get("type").and_then(Value::as_str).unwrap_or("") {
        "assistant" => {
            let Some(content) = event
                .get("message")
                .and_then(|message| message.get("content"))
                .and_then(Value::as_array)
            else {
                return;
            };
            for item in content {
                if item.get("type").and_then(Value::as_str) != Some("tool_use") {
                    continue;
                }
                let name = item.get("name").and_then(Value::as_str).unwrap_or("工具");
                let input = item.get("input").cloned().unwrap_or(Value::Null);
                push_log(
                    &mut parsed.log,
                    format!("{name}  {}", tool_detail(name, &input)),
                );
            }
        }
        "user" => {
            let Some(content) = event
                .get("message")
                .and_then(|message| message.get("content"))
                .and_then(Value::as_array)
            else {
                return;
            };
            for item in content {
                if item.get("type").and_then(Value::as_str) != Some("tool_result") {
                    continue;
                }
                let text = match item.get("content") {
                    Some(Value::String(text)) => text.clone(),
                    Some(value) => value.to_string(),
                    None => String::new(),
                };
                let failed = item
                    .get("is_error")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                push_log(
                    &mut parsed.log,
                    format!(
                        "{} {}",
                        if failed { "✗" } else { "↳" },
                        cap_text(&text, 200)
                    ),
                );
            }
        }
        "result" => {
            parsed.is_error = event
                .get("is_error")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            parsed.result = event
                .get("result")
                .and_then(Value::as_str)
                .map(|value| value.to_string());
            parsed.structured = event.get("structured_output").cloned();
            parsed.cost_usd = event.get("total_cost_usd").and_then(Value::as_f64);
            parsed.session_id = event
                .get("session_id")
                .and_then(Value::as_str)
                .map(|value| value.to_string());
            parsed.terminal_reason = event
                .get("terminal_reason")
                .and_then(Value::as_str)
                .map(|value| value.to_string());
        }
        _ => {}
    }
}

fn parse_json_output(text: &str, parsed: &mut Parsed) {
    let trimmed = text.trim();
    let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
        return;
    };
    parsed.is_error = value
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    parsed.result = value
        .get("result")
        .and_then(Value::as_str)
        .map(|value| value.to_string());
    parsed.structured = value.get("structured_output").cloned();
    parsed.cost_usd = value.get("total_cost_usd").and_then(Value::as_f64);
    parsed.session_id = value
        .get("session_id")
        .and_then(Value::as_str)
        .map(|value| value.to_string());
    parsed.terminal_reason = value
        .get("terminal_reason")
        .and_then(Value::as_str)
        .map(|value| value.to_string());
}

pub(crate) fn kill_process_tree(pid: u32) {
    #[cfg(target_os = "windows")]
    {
        let _ = quiet_command("taskkill.exe")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .spawn();
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = quiet_command("kill")
            .args(["-TERM", &pid.to_string()])
            .spawn();
    }
}

fn build_command(config: &RunnerConfig) -> std::process::Command {
    let program = config.program.to_string_lossy().into_owned();
    let mut command = quiet_command(&program);
    command
        .arg("-p")
        .arg(config.prompt)
        .arg("--output-format")
        .arg(config.output_format)
        .arg("--max-turns")
        .arg(config.max_turns.to_string())
        .arg("--permission-mode")
        .arg(config.permission_mode)
        .arg("--session-id")
        .arg(config.session_id)
        .arg("--setting-sources")
        .arg("local");
    if config.output_format == "stream-json" {
        command.arg("--verbose");
    }
    if let Some(settings) = config.settings {
        command.arg("--settings").arg(settings);
    }
    if let Some(schema) = config.json_schema {
        command.arg("--json-schema").arg(schema);
    }
    if let Some(budget) = config.max_budget_usd {
        command.arg("--max-budget-usd").arg(format!("{budget:.4}"));
    }
    command
        .current_dir(config.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.envs(config.env);
    command
}

pub(crate) fn run_claude(config: &RunnerConfig, on_spawn: impl FnOnce(u32)) -> ProcessOutcome {
    let mut command = build_command(config);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return ProcessOutcome {
                error: Some(format!("启动 Claude 失败：{error}")),
                session_id: config.session_id.to_string(),
                ..ProcessOutcome::default()
            }
        }
    };
    let pid = child.id();
    on_spawn(pid);

    let timed_out = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));
    {
        let timed_out = timed_out.clone();
        let done = done.clone();
        let timeout = config.timeout;
        std::thread::spawn(move || {
            let deadline = Instant::now() + timeout;
            while Instant::now() < deadline {
                if done.load(Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            if !done.load(Ordering::SeqCst) {
                timed_out.store(true, Ordering::SeqCst);
                kill_process_tree(pid);
            }
        });
    }

    let stderr_handle = child.stderr.take().map(|stderr| {
        std::thread::spawn(move || {
            let mut buffer = String::new();
            let mut limited = stderr.take(16 * 1024);
            let _ = limited.read_to_string(&mut buffer);
            buffer
        })
    });

    let mut raw = String::new();
    let mut parsed = Parsed::default();
    if let Some(stdout) = child.stdout.take() {
        if config.output_format == "json" {
            let mut reader = BufReader::new(stdout);
            let _ = reader.read_to_string(&mut raw);
        } else {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                parse_stream_line(&line, &mut parsed);
            }
        }
    }
    let status = child.wait();
    done.store(true, Ordering::SeqCst);
    let stderr = stderr_handle
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();

    if config.output_format == "json" {
        parse_json_output(&raw, &mut parsed);
    }

    let exit_code = status.as_ref().ok().and_then(|status| status.code());
    let exit_ok = status
        .as_ref()
        .map(|status| status.success())
        .unwrap_or(false);
    let timed_out = timed_out.load(Ordering::SeqCst);
    let session_id = parsed
        .session_id
        .clone()
        .unwrap_or_else(|| config.session_id.to_string());

    let mut error = None;
    if timed_out {
        error = Some(format!(
            "任务超时（{} 秒），已终止进程树",
            config.timeout.as_secs()
        ));
    } else if parsed.is_error {
        let budget_exhausted = parsed.terminal_reason.as_deref() == Some("budget_exhausted");
        let detail = parsed
            .result
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| {
                if budget_exhausted {
                    format!(
                        "预算不足：本轮已花 ${:.3}，超过上限 ${:.2} 后 Claude 停止。提高预算或新建一轮后重试。",
                        parsed.cost_usd.unwrap_or(0.0),
                        config.max_budget_usd.unwrap_or(0.0)
                    )
                } else {
                    "Claude 报告执行失败".to_string()
                }
            });
        error = Some(if stderr.trim().is_empty() || budget_exhausted {
            detail
        } else {
            format!("{detail}（{}）", cap_text(&stderr, 300))
        });
    } else if !exit_ok {
        error = Some(match exit_code {
            Some(code) => format!("Claude 退出码 {code}"),
            None => "Claude 进程被终止".to_string(),
        });
    } else if parsed.result.is_none() {
        error = Some("Claude 进程结束了，但没有返回结果".to_string());
    }
    if error.is_none() && !stderr.trim().is_empty() {
        // stderr 里的 tracing/网关提示不是失败，但要在诊断里留一行。
        push_log(
            &mut parsed.log,
            format!("stderr: {}", cap_text(&stderr, 200)),
        );
    }
    if let Some(reason) = parsed.terminal_reason.as_deref() {
        if reason != "completed" && error.is_none() {
            error = Some(format!("Claude 结束原因：{reason}"));
        }
    }

    let ok = error.is_none();
    ProcessOutcome {
        ok,
        result: parsed.result,
        structured: parsed.structured,
        error,
        exit_code,
        cost_usd: parsed.cost_usd,
        session_id,
        log: parsed.log,
        timed_out,
    }
}

pub(crate) struct CodexConfig<'a> {
    pub program: &'a Path,
    pub cwd: &'a Path,
    pub prompt: &'a str,
    pub schema: Option<&'a Path>,
    pub last_message: &'a Path,
    pub timeout: Duration,
}

fn parse_codex_structured(text: &str) -> Option<Value> {
    let trimmed = text.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return value.is_object().then_some(value);
    }
    // 容忍模型把 JSON 包在 ```json fence 里；只接受最终的一个对象。
    let fenced = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|value| value.strip_suffix("```"))
        .map(str::trim)?;
    serde_json::from_str::<Value>(fenced)
        .ok()
        .filter(Value::is_object)
}

pub(crate) fn run_codex(config: &CodexConfig<'_>, on_spawn: impl FnOnce(u32)) -> ProcessOutcome {
    let mut command = quiet_command(&config.program.to_string_lossy());
    command.arg("exec").arg("--json");
    if let Some(schema) = config.schema {
        command.arg("--output-schema").arg(schema);
    }
    command
        .arg("--output-last-message")
        .arg(config.last_message)
        .arg("--sandbox")
        .arg("workspace-write")
        .arg("--skip-git-repo-check")
        .arg("--ephemeral")
        .arg("-C")
        .arg(config.cwd)
        .arg("--")
        .arg(config.prompt)
        .current_dir(config.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return ProcessOutcome {
                error: Some(format!("启动 Codex 失败：{error}")),
                ..ProcessOutcome::default()
            }
        }
    };
    let pid = child.id();
    on_spawn(pid);

    let timed_out = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));
    {
        let timed_out = timed_out.clone();
        let done = done.clone();
        let timeout = config.timeout;
        std::thread::spawn(move || {
            let deadline = Instant::now() + timeout;
            while Instant::now() < deadline {
                if done.load(Ordering::SeqCst) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            if !done.load(Ordering::SeqCst) {
                timed_out.store(true, Ordering::SeqCst);
                kill_process_tree(pid);
            }
        });
    }

    let stderr_handle = child.stderr.take().map(|stderr| {
        std::thread::spawn(move || {
            let mut buffer = String::new();
            let mut limited = stderr.take(16 * 1024);
            let _ = limited.read_to_string(&mut buffer);
            buffer
        })
    });

    let mut log = Vec::new();
    let mut last_event_message = None;
    if let Some(stdout) = child.stdout.take() {
        let reader = BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            let Ok(event) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            match event.get("type").and_then(Value::as_str) {
                Some("item.completed") => {
                    let item = event.get("item").unwrap_or(&Value::Null);
                    match item.get("type").and_then(Value::as_str) {
                        Some("agent_message") => {
                            if let Some(text) = item.get("text").and_then(Value::as_str) {
                                last_event_message = Some(text.to_string());
                                push_log(&mut log, format!("Codex: {}", cap_text(text, 300)));
                            }
                        }
                        Some("command_execution") => {
                            let command = item
                                .get("command")
                                .and_then(Value::as_str)
                                .unwrap_or("command");
                            let exit = item.get("exit_code").and_then(Value::as_i64);
                            push_log(
                                &mut log,
                                format!(
                                    "命令：{}（exit {}）",
                                    cap_text(command, 200),
                                    exit.map_or("-".to_string(), |value| value.to_string())
                                ),
                            );
                        }
                        Some("file_change") => {
                            let files = item
                                .get("changes")
                                .and_then(Value::as_array)
                                .map(|changes| {
                                    changes
                                        .iter()
                                        .filter_map(|change| {
                                            change.get("path").and_then(Value::as_str)
                                        })
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                })
                                .unwrap_or_default();
                            push_log(&mut log, format!("改动：{}", cap_text(&files, 300)));
                        }
                        _ => {}
                    }
                }
                Some("error") => {
                    if let Some(message) = event.get("message").and_then(Value::as_str) {
                        push_log(&mut log, format!("Codex error: {}", cap_text(message, 300)));
                    }
                }
                Some("turn.completed") => {
                    if let Some(usage) = event.get("usage") {
                        let input = usage
                            .get("input_tokens")
                            .and_then(Value::as_u64)
                            .unwrap_or(0);
                        let output = usage
                            .get("output_tokens")
                            .and_then(Value::as_u64)
                            .unwrap_or(0);
                        push_log(
                            &mut log,
                            format!("Codex usage: input={input}, output={output}"),
                        );
                    }
                }
                _ => {}
            }
        }
    }
    let status = child.wait();
    done.store(true, Ordering::SeqCst);
    let stderr = stderr_handle
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();

    let exit_code = status.as_ref().ok().and_then(|status| status.code());
    let exit_ok = status
        .as_ref()
        .map(|status| status.success())
        .unwrap_or(false);
    let timed_out = timed_out.load(Ordering::SeqCst);
    let last_text = fs::read_to_string(config.last_message).ok();
    let structured = if config.schema.is_some() {
        last_text
            .as_deref()
            .and_then(parse_codex_structured)
            .or_else(|| {
                last_event_message
                    .as_deref()
                    .and_then(parse_codex_structured)
            })
    } else {
        None
    };
    let result = last_text.or(last_event_message);

    let mut error = None;
    if timed_out {
        error = Some(format!(
            "Codex 任务超时（{} 秒），已终止进程树",
            config.timeout.as_secs()
        ));
    } else if !exit_ok {
        error = Some(match exit_code {
            Some(code) => format!("Codex 退出码 {code}"),
            None => "Codex 进程被终止".to_string(),
        });
    } else if config.schema.is_some() && structured.is_none() {
        error = Some("Codex 结束了，但没有返回符合 schema 的结构化结果".to_string());
    } else if config.schema.is_none()
        && result.as_deref().map_or(true, |text| text.trim().is_empty())
    {
        error = Some("Codex 结束了，但没有返回文本结果".to_string());
    }
    if error.is_none() && !stderr.trim().is_empty() {
        push_log(&mut log, format!("stderr: {}", cap_text(&stderr, 200)));
    }

    ProcessOutcome {
        ok: error.is_none(),
        result,
        structured,
        error,
        exit_code,
        cost_usd: None,
        session_id: String::new(),
        log,
        timed_out,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "live Codex call; run manually with --ignored"]
    fn codex_worker_live_returns_schema_json() {
        let Some(program) = crate::orchestrator::capability::codex_program() else {
            eprintln!("skip: codex not installed");
            return;
        };
        let dir = std::env::temp_dir().join(format!(
            "agent-island-codex-worker-{}",
            crate::epoch_millis()
        ));
        fs::create_dir_all(&dir).unwrap();
        let schema = dir.join("schema.json");
        fs::write(
            &schema,
            serde_json::json!({
                "type": "object",
                "properties": {
                    "status": { "type": "string", "enum": ["completed", "blocked", "failed"] },
                    "summary": { "type": "string" },
                    "files_changed": { "type": "array", "items": { "type": "string" } },
                    "verification": { "type": "string" }
                },
                "required": ["status", "summary", "files_changed", "verification"],
                "additionalProperties": false
            })
            .to_string(),
        )
        .unwrap();
        let last = dir.join("last.txt");
        let prompt = "Create CODEX_WORKER_OK.txt in the current directory with exact content CODEX_WORKER_OK. Do not modify anything else.";
        let outcome = run_codex(
            &CodexConfig {
                program: &program,
                cwd: &dir,
                prompt,
                schema: Some(&schema),
                last_message: &last,
                timeout: Duration::from_secs(180),
            },
            |_| {},
        );
        assert!(outcome.ok, "codex failed: {:?}", outcome.error);
        let structured = outcome.structured.expect("structured result");
        assert_eq!(structured["status"], "completed");
        assert_eq!(
            fs::read_to_string(dir.join("CODEX_WORKER_OK.txt"))
                .unwrap()
                .trim(),
            "CODEX_WORKER_OK"
        );
        let _ = fs::remove_dir_all(dir);
    }
}
