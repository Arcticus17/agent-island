//! 内置终端：用 ConPTY/PTY 把 Agent CLI 真正跑在岛里。
//!
//! 和 `send_to_session` 的区别：那条是"一次性 `-p` 调用"，这里是**交互式 TUI**，
//! 用户可以在岛上直接敲 `claude` / `codex` / `opencode`，看到 Agent 的完整界面。

use crate::*;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::collections::HashMap;
use std::io::{Read, Write};

pub(crate) struct TerminalSession {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

pub(crate) type TerminalMap = Mutex<HashMap<String, TerminalSession>>;

fn pty_size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows: rows.max(2),
        cols: cols.max(20),
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// 只允许启动"已知的 Agent CLI"或系统 shell，避免变成任意命令执行入口。
pub(crate) fn terminal_program_allowed(program: &str) -> bool {
    let lower = program.to_lowercase();
    let base = lower
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(&lower)
        .trim_end_matches(".exe")
        .trim_end_matches(".cmd")
        .trim_end_matches(".bat")
        .trim_end_matches(".ps1")
        .to_string();
    if matches!(
        base.as_str(),
        "cmd" | "powershell" | "pwsh" | "claude" | "codex" | "opencode" | "hermes" | "node" | "bash"
    ) {
        return true;
    }
    // 自定义适配器：只要它是 agent_defs() 里配置过的 keyword 就放行
    agent_defs()
        .iter()
        .any(|def| def.keyword.eq_ignore_ascii_case(&base))
}

/// 把 (program, args) 变成真正可以 CreateProcess 的命令行：`.cmd/.ps1` shim 要过 cmd.exe。
fn terminal_command(program: &str, args: &[String]) -> Vec<String> {
    let mut command = vec![program.to_string()];
    command.extend(args.iter().cloned());
    if looks_like_shim(program) {
        wrap_with_cmd(&command)
    } else {
        command
    }
}

#[tauri::command]
pub(crate) fn terminal_open(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
    program: String,
    args: Vec<String>,
    cwd: Option<String>,
    cols: u16,
    rows: u16,
) -> Result<String, String> {
    let program = program.trim().to_string();
    if program.is_empty() {
        return Err("terminal program is empty".into());
    }
    if !terminal_program_allowed(&program) {
        return Err(format!("不允许在岛内启动的程序: {program}"));
    }
    let command = terminal_command(&program, &args);

    let pair = native_pty_system()
        .openpty(pty_size(cols, rows))
        .map_err(|error| format!("创建终端失败: {error}"))?;
    let mut builder = CommandBuilder::new(&command[0]);
    for argument in &command[1..] {
        builder.arg(argument);
    }
    if let Some(dir) = cwd.as_deref().filter(|dir| !dir.trim().is_empty()) {
        builder.cwd(dir);
    }
    builder.env("TERM", "xterm-256color");

    let child = pair
        .slave
        .spawn_command(builder)
        .map_err(|error| format!("启动终端进程失败: {error}"))?;
    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|error| format!("终端读取失败: {error}"))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|error| format!("终端写入失败: {error}"))?;

    let id = format!(
        "term-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );

    let emit_id = id.clone();
    let handle = app.clone();
    std::thread::spawn(move || {
        use tauri::Emitter;
        let mut buffer = [0u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => {
                    let data = String::from_utf8_lossy(&buffer[..read]).to_string();
                    let _ = handle.emit(
                        "terminal-output",
                        serde_json::json!({ "id": emit_id, "data": data }),
                    );
                }
            }
        }
        let _ = handle.emit("terminal-exit", serde_json::json!({ "id": emit_id }));
    });

    state
        .terminals
        .lock_recover()
        .insert(id.clone(), TerminalSession { master: pair.master, writer, child });
    Ok(id)
}

#[tauri::command]
pub(crate) fn terminal_write(
    state: tauri::State<AppState>,
    id: String,
    data: String,
) -> Result<(), String> {
    let mut sessions = state.terminals.lock_recover();
    let session = sessions.get_mut(&id).ok_or("终端已关闭")?;
    session
        .writer
        .write_all(data.as_bytes())
        .map_err(|error| format!("写入终端失败: {error}"))?;
    session
        .writer
        .flush()
        .map_err(|error| format!("刷新终端失败: {error}"))
}

#[tauri::command]
pub(crate) fn terminal_resize(
    state: tauri::State<AppState>,
    id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let mut sessions = state.terminals.lock_recover();
    let session = sessions.get_mut(&id).ok_or("终端已关闭")?;
    session
        .master
        .resize(pty_size(cols, rows))
        .map_err(|error| format!("调整终端尺寸失败: {error}"))
}

#[tauri::command]
pub(crate) fn terminal_close(state: tauri::State<AppState>, id: String) -> Result<(), String> {
    let session = state.terminals.lock_recover().remove(&id);
    if let Some(mut session) = session {
        let _ = session.child.kill();
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn terminal_list(state: tauri::State<AppState>) -> Vec<String> {
    state
        .terminals
        .lock_recover()
        .keys()
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{terminal_command, terminal_program_allowed};

    #[test]
    fn allows_known_agents_and_rejects_arbitrary_programs() {
        assert!(terminal_program_allowed("claude"));
        assert!(terminal_program_allowed("codex"));
        assert!(terminal_program_allowed("cmd.exe"));
        assert!(terminal_program_allowed(r"C:\Windows\System32\cmd.exe"));
        assert!(!terminal_program_allowed("format.com"));
        assert!(!terminal_program_allowed(r"C:\Windows\System32\reg.exe"));
    }

    #[test]
    fn wraps_npm_shims_through_cmd() {
        let wrapped = terminal_command("claude", &["--resume".into(), "abc".into()]);
        assert_eq!(wrapped[0], "cmd.exe");
        assert_eq!(wrapped[1], "/C");
        assert_eq!(wrapped[2], "claude");

        let direct = terminal_command("cmd.exe", &[]);
        assert_eq!(direct, vec!["cmd.exe".to_string()]);
    }
}
