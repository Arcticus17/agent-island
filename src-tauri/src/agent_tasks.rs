//! 岛内任务模式：用 `codex exec --json` 让 Agent 明确说出它做了什么。
//!
//! 这是 Codex 唯一的显式事件通道（0.163.0 起支持 `--json`：Print events to stdout as JSONL）。
//! 事件形态来自真实运行捕获（`tests/fixtures/codex/exec-json.jsonl`）：
//!
//! ```text
//! {"type":"thread.started","thread_id":"…"}   ← 权威 session id
//! {"type":"turn.started"}
//! {"type":"item.completed","item":{…}}
//! {"type":"error","message":"…"}
//! {"type":"turn.failed","error":{"message":"…"}}
//! ```
//!
//! 设计原则：只认这些明确的事件；无法识别的 JSON 与 stderr 里的 tracing 行一律忽略，
//! 进程退出本身也算一次结束——这样即使 CLI 升级改了事件名，任务也不会永远"运行中"。

use crate::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CodexEventKind {
    ThreadStarted,
    TurnStarted,
    TurnCompleted,
    TurnFailed,
    ItemCompleted,
    Error,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodexEvent {
    pub kind: CodexEventKind,
    pub thread_id: Option<String>,
    pub message: Option<String>,
}

/// 解析一行 `codex exec --json` 输出。非 JSON（例如 tracing 日志）返回 None。
pub(crate) fn parse_codex_event(line: &str) -> Option<CodexEvent> {
    let trimmed = line.trim();
    if !trimmed.starts_with('{') {
        return None;
    }
    let value: Value = serde_json::from_str(trimmed).ok()?;
    let kind = match value.get("type").and_then(Value::as_str)? {
        "thread.started" => CodexEventKind::ThreadStarted,
        "turn.started" => CodexEventKind::TurnStarted,
        "turn.completed" => CodexEventKind::TurnCompleted,
        "turn.failed" => CodexEventKind::TurnFailed,
        "item.completed" => CodexEventKind::ItemCompleted,
        "error" => CodexEventKind::Error,
        _ => CodexEventKind::Other,
    };
    let thread_id = value
        .get("thread_id")
        .and_then(Value::as_str)
        .map(str::to_owned);
    // 失败原因可能在顶层 message，也可能嵌在 item/error 里
    let message = value
        .get("message")
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            value
                .get("item")
                .and_then(|item| item.get("message"))
                .and_then(Value::as_str)
        })
        .map(str::to_owned);
    Some(CodexEvent {
        kind,
        thread_id,
        message,
    })
}

/// 任务状态机的纯函数部分：事件 + 进程是否已退出 → 下一步状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TaskState {
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

pub(crate) fn next_state(
    current: TaskState,
    event: Option<&CodexEvent>,
    exited: Option<bool>,
    exit_ok: bool,
) -> TaskState {
    if current != TaskState::Running {
        return current;
    }
    if let Some(event) = event {
        match event.kind {
            CodexEventKind::TurnFailed => return TaskState::Failed,
            CodexEventKind::TurnCompleted => return TaskState::Succeeded,
            _ => {}
        }
    }
    match exited {
        Some(true) => {
            if exit_ok {
                TaskState::Succeeded
            } else {
                TaskState::Failed
            }
        }
        _ => TaskState::Running,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> &'static str {
        include_str!("../tests/fixtures/codex/exec-json.jsonl")
    }

    #[test]
    fn parses_the_real_event_stream() {
        let events: Vec<CodexEvent> = fixture().lines().filter_map(parse_codex_event).collect();
        assert!(events.len() >= 5, "真实输出里应有多个事件");
        assert_eq!(events[0].kind, CodexEventKind::ThreadStarted);
        assert_eq!(
            events[0].thread_id.as_deref(),
            Some("01a17db0-e9d9-7410-a496-cb9866d2090c")
        );
        assert_eq!(events[1].kind, CodexEventKind::TurnStarted);
        assert!(events
            .iter()
            .any(|event| event.kind == CodexEventKind::ItemCompleted));
        assert!(events.iter().any(|event| event.kind == CodexEventKind::Error));
        let failed = events
            .iter()
            .find(|event| event.kind == CodexEventKind::TurnFailed)
            .expect("turn.failed");
        assert_eq!(failed.message.as_deref(), Some("workspace routing discovery timed out"));
    }

    #[test]
    fn ignores_non_json_and_unknown_types() {
        assert!(parse_codex_event("2026-10-27T14:08:33Z ERROR codex_models_manager: timed out").is_none());
        assert!(parse_codex_event("").is_none());
        assert!(parse_codex_event("not json").is_none());
        let other = parse_codex_event("{\"type\":\"something.new\",\"x\":1}").expect("json");
        assert_eq!(other.kind, CodexEventKind::Other);
        assert!(other.thread_id.is_none());
    }

    #[test]
    fn state_machine_follows_events_and_process_exit() {
        let started = CodexEvent { kind: CodexEventKind::TurnStarted, thread_id: None, message: None };
        let failed = CodexEvent {
            kind: CodexEventKind::TurnFailed,
            thread_id: None,
            message: Some("boom".into()),
        };
        let completed = CodexEvent { kind: CodexEventKind::TurnCompleted, thread_id: None, message: None };
        assert_eq!(next_state(TaskState::Running, Some(&started), None, false), TaskState::Running);
        assert_eq!(next_state(TaskState::Running, Some(&failed), None, false), TaskState::Failed);
        assert_eq!(next_state(TaskState::Running, Some(&completed), None, false), TaskState::Succeeded);
        // 没有结束事件时，进程退出也算结束——不能永远"运行中"
        assert_eq!(next_state(TaskState::Running, None, Some(true), true), TaskState::Succeeded);
        assert_eq!(next_state(TaskState::Running, None, Some(true), false), TaskState::Failed);
        // 已结束的状态不再变化
        assert_eq!(next_state(TaskState::Failed, Some(&completed), Some(true), true), TaskState::Failed);
    }
}
/// 一个正在跑的任务：进程 + 事件状态。前端只看 `TaskSnapshot`。
pub(crate) struct TaskRuntime {
    pub agent_id: String,
    pub state: TaskState,
    pub session_id: Option<String>,
    pub message: Option<String>,
    pub child: Option<std::process::Child>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct TaskSnapshot {
    pub id: String,
    pub agent_id: String,
    pub state: TaskState,
    pub session_id: Option<String>,
    pub message: Option<String>,
}

impl TaskRuntime {
    pub(crate) fn snapshot(&self, id: &str) -> TaskSnapshot {
        TaskSnapshot {
            id: id.to_string(),
            agent_id: self.agent_id.clone(),
            state: self.state,
            session_id: self.session_id.clone(),
            message: self.message.clone(),
        }
    }
}

/// 启动一个 `codex exec --json` 任务；事件在后台线程里解析。
#[tauri::command]
pub(crate) fn start_agent_task(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
    prompt: String,
) -> Result<String, String> {
    let prompt = prompt.trim().to_string();
    if prompt.is_empty() {
        return Err("任务内容不能为空".into());
    }
    let mut child = std::process::Command::new("codex")
        .args(["exec", "--json", "--skip-git-repo-check", &prompt])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|error| format!("启动 codex 失败：{error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "无法读取 codex 输出".to_string())?;
    let id = format!("task-{}", epoch_millis());
    let task = std::sync::Arc::new(std::sync::Mutex::new(TaskRuntime {
        agent_id: "codex".into(),
        state: TaskState::Running,
        session_id: None,
        message: None,
        child: Some(child),
    }));
    state.tasks.lock_recover().insert(id.clone(), task.clone());

    let app_for_thread = app.clone();
    let id_for_thread = id.clone();
    std::thread::spawn(move || {
        use std::io::BufRead;
        let reader = std::io::BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            let Some(event) = parse_codex_event(&line) else {
                continue;
            };
            let mut runtime = task.lock().unwrap_or_else(|poison| poison.into_inner());
            if let Some(thread_id) = &event.thread_id {
                runtime.session_id = Some(thread_id.clone());
            }
            if let Some(message) = &event.message {
                runtime.message = Some(message.clone());
            }
            let next = next_state(runtime.state, Some(&event), None, false);
            runtime.state = next;
            let snapshot = runtime.snapshot(&id_for_thread);
            drop(runtime);
            // Agent 自报的 thread id 就是权威 session id
            if let Some(session_id) = &snapshot.session_id {
                let app_state = app_for_thread.state::<AppState>();
                let mut hooked = app_state.hooked.lock_recover();
                record_hooked(&mut hooked, "codex", session_id, None, epoch_millis());
            }
            use tauri::Emitter;
            let _ = app_for_thread.emit("agent-task", &snapshot);
            if matches!(snapshot.state, TaskState::Failed | TaskState::Succeeded) {
                break;
            }
        }
        // 进程退出本身也是一次结束：CLI 升级改了事件名也不会永远"运行中"
        let mut runtime = task.lock().unwrap_or_else(|poison| poison.into_inner());
        let exit_ok = runtime
            .child
            .as_mut()
            .and_then(|child| child.wait().ok())
            .map(|status| status.success())
            .unwrap_or(false);
        let next = next_state(runtime.state, None, Some(true), exit_ok);
        runtime.state = next;
        runtime.child = None;
        let snapshot = runtime.snapshot(&id_for_thread);
        drop(runtime);
        use tauri::Emitter;
        let _ = app_for_thread.emit("agent-task", &snapshot);
    });
    Ok(id)
}

#[tauri::command]
pub(crate) fn list_agent_tasks(state: tauri::State<AppState>) -> Vec<TaskSnapshot> {
    let tasks = state.tasks.lock_recover();
    let mut list: Vec<TaskSnapshot> = tasks
        .iter()
        .map(|(id, task)| {
            task.lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .snapshot(id)
        })
        .collect();
    list.sort_by(|left, right| left.id.cmp(&right.id));
    list
}

#[tauri::command]
pub(crate) fn cancel_agent_task(
    state: tauri::State<AppState>,
    id: String,
) -> Result<(), String> {
    let task = state
        .tasks
        .lock_recover()
        .get(&id)
        .cloned()
        .ok_or_else(|| format!("任务不存在：{id}"))?;
    let mut runtime = task.lock().unwrap_or_else(|poison| poison.into_inner());
    if let Some(child) = runtime.child.as_mut() {
        let _ = child.kill();
    }
    runtime.state = TaskState::Cancelled;
    Ok(())
}
