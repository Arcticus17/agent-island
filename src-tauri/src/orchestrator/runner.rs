//! 单个 Claude 无头进程的启动、事件解析、超时与进程树清理。
//!
//! 只认 Claude 自己的结构化事件：`assistant` 里的 tool_use、`user` 里的
//! tool_result、最后的 `result`。stderr 只作为错误说明，不参与状态判定。

use crate::*;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
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
                push_log(&mut parsed.log, format!("{name}  {}", tool_detail(name, &input)));
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
                let failed = item.get("is_error").and_then(Value::as_bool).unwrap_or(false);
                push_log(
                    &mut parsed.log,
                    format!("{} {}", if failed { "✗" } else { "↳" }, cap_text(&text, 200)),
                );
            }
        }
        "result" => {
            parsed.is_error = event.get("is_error").and_then(Value::as_bool).unwrap_or(false);
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
    parsed.is_error = value.get("is_error").and_then(Value::as_bool).unwrap_or(false);
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
    command
        .current_dir(config.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.envs(config.env);
    command
}

pub(crate) fn run_claude(
    config: &RunnerConfig,
    on_spawn: impl FnOnce(u32),
) -> ProcessOutcome {
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
    let exit_ok = status.as_ref().map(|status| status.success()).unwrap_or(false);
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
        let detail = parsed
            .result
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "Claude 报告执行失败".to_string());
        error = Some(if stderr.trim().is_empty() {
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
        push_log(&mut parsed.log, format!("stderr: {}", cap_text(&stderr, 200)));
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
