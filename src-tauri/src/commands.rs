//! Tauri command surface: window control, session ops, autostart, privacy.

use crate::*;

#[cfg(target_os = "windows")]
unsafe fn parent_pid_of(pid: u32) -> Option<u32> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..std::mem::zeroed()
    };
    let mut found = None;
    if Process32FirstW(snapshot, &mut entry).is_ok() {
        loop {
            if entry.th32ProcessID == pid {
                found = Some(entry.th32ParentProcessID);
                break;
            }
            if Process32NextW(snapshot, &mut entry).is_err() {
                break;
            }
        }
    }
    let _ = CloseHandle(snapshot);
    found
}

#[cfg(target_os = "windows")]
unsafe fn window_hwnd_for_pid(pid: u32) -> Option<windows::Win32::Foundation::HWND> {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextLengthW, GetWindowThreadProcessId, IsWindowVisible,
    };
    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let list = &mut *(lparam.0 as *mut Vec<HWND>);
        list.push(hwnd);
        BOOL(1)
    }
    let mut all: Vec<HWND> = Vec::new();
    let _ = EnumWindows(Some(enum_proc), LPARAM(&mut all as *mut Vec<HWND> as isize));
    let self_pid = std::process::id();
    if pid == self_pid {
        return None;
    }
    let mut best: Option<(i32, HWND)> = None;
    for hwnd in all {
        let mut win_pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut win_pid));
        if win_pid != pid {
            continue;
        }
        if !IsWindowVisible(hwnd).as_bool() {
            continue;
        }
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            continue;
        }
        if best.as_ref().map(|(l, _)| len > *l).unwrap_or(true) {
            best = Some((len, hwnd));
        }
    }
    best.map(|(_, h)| h)
}

#[tauri::command]
pub(crate) fn focus_agent_terminal(
    name: String,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (name, state);
        return Err("not supported on this platform".into());
    }
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::WindowsAndMessaging::{
            SetForegroundWindow, ShowWindow, SW_RESTORE,
        };
        let mut sys = state
            .sys
            .lock()
            .map_err(|_| "state lock error".to_string())?;
        let pids = find_agent_pids(&mut sys, &name)?;
        if pids.is_empty() {
            return Err("agent is not running".into());
        }
        for pid in pids {
            let mut target = pid;
            for _ in 0..8 {
                unsafe {
                    if let Some(hwnd) = window_hwnd_for_pid(target) {
                        let _ = ShowWindow(hwnd, SW_RESTORE);
                        if SetForegroundWindow(hwnd).as_bool() {
                            return Ok(());
                        }
                        return Err("Windows 未允许切换到 Agent 终端，请从任务栏选择该窗口".into());
                    }
                }
                let Some(parent) = (unsafe { parent_pid_of(target) }) else {
                    break;
                };
                if parent == 0 || parent == target {
                    break;
                }
                target = parent;
            }
        }
        Err("未找到 Agent 所在的终端窗口".into())
    }
}

pub(crate) fn collect_cached_with<Clock, Scan>(
    cache: &mut AgentDataCache,
    mut clock: Clock,
    scan: Scan,
) -> (
    Vec<AgentInfo>,
    interface::snapshot::AgentViewSnapshot,
    interface::diagnostics::DiagnosticSnapshot,
    Option<(bool, Instant)>,
)
where
    Clock: FnMut() -> Instant,
    Scan: FnOnce() -> (
        Vec<AgentInfo>,
        interface::snapshot::AgentViewSnapshot,
        interface::diagnostics::DiagnosticSnapshot,
        bool,
    ),
{
    let checked_at = clock();
    if let Some((cached_at, cached_agents, cached_snapshot, cached_diagnostics)) = cache.as_ref() {
        if checked_at.saturating_duration_since(*cached_at).as_millis() < 500 {
            return (
                cached_agents.clone(),
                cached_snapshot.clone(),
                cached_diagnostics.clone(),
                None,
            );
        }
    }

    let (agents, snapshot, diagnostics, changed) = scan();
    let completed_at = clock();
    *cache = Some((
        completed_at,
        agents.clone(),
        snapshot.clone(),
        diagnostics.clone(),
    ));
    (agents, snapshot, diagnostics, Some((changed, completed_at)))
}

/// OpenCode 服务探测节流：每 3 秒最多问一次
static OPENCODE_PROBE_AT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub(crate) fn collect_agent_data(
    state: &AppState,
) -> (Vec<AgentInfo>, interface::snapshot::AgentViewSnapshot) {
    let mut sys = state.sys.lock_recover();
    let mut session = state.session.lock_recover();
    let mut cache = session.cache.take();
    // 岛内终端的 shell pid：Agent 作为它们的后代即可被权威识别
    let pty_roots: std::collections::HashSet<u32> = state.terminals.lock_recover().values().filter_map(|terminal| terminal.pid).collect();
    // 岛内启动过哪些 Agent、各自什么时候启动的（用于只认"启动之后"的会话）
    let agent_ids: Vec<String> = agent_defs()
        .iter()
        .map(|def| if def.log_kind.is_empty() { def.keyword.clone() } else { def.log_kind.clone() })
        .collect();
    let mut launch_facts: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    for terminal in state.terminals.lock_recover().values() {
        if let Some(agent_id) = launcher::agent_for_program(&terminal.program, &agent_ids) {
            let entry = launch_facts.entry(agent_id).or_insert(terminal.started_at_ms);
            *entry = (*entry).max(terminal.started_at_ms);
        }
    }
    let bindings = state.bindings.lock_recover();
    // OpenCode：配置了本机服务地址时，问它"哪个会话在忙"——这是它自己报的，不是猜的。
    // 每 3 秒最多问一次，避免每轮轮询都打一次 HTTP。
    if let Some((host, port)) = configured_endpoint() {
        let now = epoch_millis();
        let should_probe = OPENCODE_PROBE_AT
            .load(std::sync::atomic::Ordering::Relaxed)
            .checked_add(3_000)
            .map(|next| now >= next)
            .unwrap_or(true);
        if should_probe {
            OPENCODE_PROBE_AT.store(now, std::sync::atomic::Ordering::Relaxed);
            if let Ok(body) = http_get(&host, port, "/session/status") {
                if let Some(session_id) = busy_session(&body) {
                    let mut hooked = state.hooked.lock_recover();
                    record_hooked(&mut hooked, "opencode", &session_id, None, now);
                }
            }
        }
    }
    // 只取仍然有效的自报身份（超时的会话不该继续声称"已确认"）
    let now_ms = epoch_millis();
    let hooked_sessions: std::collections::HashMap<String, crate::hooked::HookedSession> = {
        let hooked = state.hooked.lock_recover();
        hooked
            .iter()
            .filter(|(agent_id, _)| fresh_hooked(&hooked, agent_id, now_ms).is_some())
            .map(|(agent_id, record)| (agent_id.clone(), record.clone()))
            .collect()
    };
    let (agents, snapshot, _diagnostics, scan_result) =
        collect_cached_with(&mut cache, Instant::now, || {
            let managed_pids = state.team.active_pids();
            scan_agents(
                &mut sys,
                &mut session,
                &pty_roots,
                &launch_facts,
                &bindings,
                &hooked_sessions,
                &managed_pids,
            )
        });
    session.cache = cache;
    if let Some((changed, completed_at)) = scan_result {
        if changed || completed_at.duration_since(session.last_save).as_secs() >= 30 {
            let _ = save_stats(&session.stats, &state.stats_path);
            let _ = save_daily(&session.daily, &state.daily_path);
            session.last_save = completed_at;
        }
    }
    (agents, snapshot)
}

#[tauri::command]
pub(crate) fn get_agents(state: tauri::State<AppState>) -> Vec<AgentInfo> {
    collect_agent_data(&state).0
}

#[tauri::command]
pub(crate) fn get_agent_snapshot(
    state: tauri::State<AppState>,
) -> interface::snapshot::AgentViewSnapshot {
    collect_agent_data(&state).1
}

pub(crate) fn diagnostics_from_cache(
    cache: &AgentDataCache,
) -> interface::diagnostics::DiagnosticsResponse {
    cache
        .as_ref()
        .map(|(_, _, _, diagnostics)| diagnostics.into())
        .unwrap_or_else(|| interface::diagnostics::DiagnosticsResponse {
            generated_at_ms: 0,
            issues: Vec::new(),
            sources: Vec::new(),
        })
}

pub(crate) fn export_diagnostics_snapshot(
    destination: &Path,
    snapshot: &interface::diagnostics::DiagnosticSnapshot,
) -> Result<(), String> {
    let os_version = System::long_os_version().unwrap_or_else(|| "unknown".into());
    interface::diagnostics::write_export_report(
        destination,
        snapshot,
        env!("CARGO_PKG_VERSION"),
        &os_version,
    )
}

#[tauri::command]
pub(crate) fn get_diagnostics(
    state: tauri::State<AppState>,
) -> interface::diagnostics::DiagnosticsResponse {
    let session = state.session.lock_recover();
    let mut response = diagnostics_from_cache(&session.cache);
    response.sources = usage_source_health();
    response
}

#[tauri::command]
pub(crate) fn export_diagnostics(
    destination: String,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    let snapshot = {
        let session = state.session.lock_recover();
        session
            .cache
            .as_ref()
            .map(|(_, _, _, diagnostics)| diagnostics.clone())
            .unwrap_or_default()
    };
    export_diagnostics_snapshot(Path::new(&destination), &snapshot)
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct AgentStatsRow {
    name: String,
    total_seconds: u64,
    error_count: u32,
    done_count: u32,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct StatsReportDay {
    date: String,
    agents: Vec<AgentStatsRow>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct StatsReport {
    days: Vec<StatsReportDay>,
}

#[tauri::command]
pub(crate) fn get_stats_report(state: tauri::State<AppState>, days: u32) -> StatsReport {
    let session = state.session.lock_recover();
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = days.clamp(1, 30);
    let mut out = Vec::new();
    for i in 0..days {
        let key = date_key(now_secs.saturating_sub(i as u64 * 86400));
        let mut agents: Vec<AgentStatsRow> = session
            .daily
            .get(&key)
            .map(|map| {
                map.iter()
                    .map(|(name, s)| AgentStatsRow {
                        name: name.clone(),
                        total_seconds: s.total_seconds,
                        error_count: s.error_count,
                        done_count: s.done_count,
                    })
                    .collect()
            })
            .unwrap_or_default();
        agents.sort_by(|a, b| b.total_seconds.cmp(&a.total_seconds));
        out.push(StatsReportDay { date: key, agents });
    }
    StatsReport { days: out }
}

pub(crate) fn find_agent_cwd(sys: &mut System, name: &str) -> Result<String, String> {
    let keyword = keyword_for(name).ok_or_else(|| format!("unknown agent: {name}"))?;
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let processes = matching_processes(sys, &keyword);
    processes
        .iter()
        .find_map(|p| p.cwd().map(|path| path.to_string_lossy().into_owned()))
        .ok_or_else(|| "agent working directory not found".to_string())
}

#[tauri::command]
pub(crate) fn open_project_dir(name: String, state: tauri::State<AppState>) -> Result<(), String> {
    let mut sys = state
        .sys
        .lock()
        .map_err(|_| "state lock error".to_string())?;
    let dir = find_agent_cwd(&mut sys, &name)?;
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer.exe")
            .arg(&dir)
            .spawn()
            .map_err(|e| format!("failed to open folder: {e}"))?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = dir;
        return Err("not supported on this platform".into());
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn open_path(path: String) -> Result<(), String> {
    let path = path.trim().to_string();
    if path.is_empty() {
        return Err("empty path".into());
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer.exe")
            .arg(&path)
            .spawn()
            .map_err(|e| format!("failed to open path: {e}"))?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
        return Err("not supported on this platform".into());
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn open_terminal(name: String, state: tauri::State<AppState>) -> Result<(), String> {
    let mut sys = state
        .sys
        .lock()
        .map_err(|_| "state lock error".to_string())?;
    let dir = find_agent_cwd(&mut sys, &name)?;
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
        std::process::Command::new("cmd.exe")
            .arg("/K")
            .current_dir(&dir)
            .creation_flags(CREATE_NEW_CONSOLE)
            .spawn()
            .map_err(|e| format!("failed to open terminal: {e}"))?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = dir;
        return Err("not supported on this platform".into());
    }
    Ok(())
}

pub(crate) fn find_agent_pids(sys: &mut System, name: &str) -> Result<Vec<u32>, String> {
    let keyword = keyword_for(name).ok_or_else(|| format!("unknown agent: {name}"))?;
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let pids = matching_processes(sys, &keyword)
        .iter()
        .map(|p| p.pid().as_u32())
        .collect();
    Ok(pids)
}

pub(crate) fn paths_match(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        let trimmed = s.trim_end_matches(['/', '\\']);
        if cfg!(windows) {
            trimmed.to_lowercase()
        } else {
            trimmed.to_string()
        }
    };
    norm(a) == norm(b)
}

#[cfg(target_os = "windows")]
pub(crate) fn quiet_command(program: &str) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    let mut command = std::process::Command::new(program);
    command.creation_flags(0x0800_0000);
    command
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn quiet_command(program: &str) -> std::process::Command {
    std::process::Command::new(program)
}

pub(crate) fn find_session_cwd(name: &str, session_id: &str) -> Result<String, String> {
    build_sessions(name)
        .into_iter()
        .find(|s| s.id == session_id)
        .and_then(|s| s.cwd)
        .ok_or_else(|| "session not found or has no working directory".to_string())
}

pub(crate) fn open_terminal_in_dir(dir: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
        std::process::Command::new("cmd.exe")
            .arg("/K")
            .current_dir(dir)
            .creation_flags(CREATE_NEW_CONSOLE)
            .spawn()
            .map_err(|e| format!("failed to open terminal: {e}"))?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = dir;
        return Err("not supported on this platform".into());
    }
    Ok(())
}

#[cfg(target_os = "windows")]
pub(crate) fn restart_program_available(program: &str, cwd: Option<&str>) -> Result<bool, String> {
    // `where.exe C:\...\agent.exe` treats the drive colon as path:pattern
    // and rejects valid paths. Resolve explicit paths directly instead.
    if program.contains(['\\', '/']) || std::path::Path::new(program).is_absolute() {
        let path = std::path::Path::new(program);
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::path::Path::new(cwd.unwrap_or(".")).join(path)
        };
        return Ok(resolved.is_file());
    }
    let mut command = quiet_command("where.exe");
    command.arg(program);
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    command
        .output()
        .map(|result| result.status.success())
        .map_err(|error| format!("无法检查 Agent 启动程序: {error}"))
}

pub(crate) fn spawn_command_in_dir(cmd: &[String], cwd: Option<&str>) -> Result<(), String> {
    if cmd.first().is_none_or(|program| program.trim().is_empty()) {
        return Err("未找到可启动的 Agent 命令".into());
    }
    if cmd
        .iter()
        .any(|arg| arg.contains(['&', '|', '<', '>', '^', '%', '!', '\r', '\n', '"']))
    {
        return Err("恢复命令包含不支持的终端特殊字符，无法安全启动".into());
    }
    #[cfg(target_os = "windows")]
    {
        if !restart_program_available(&cmd[0], cwd)? {
            return Err(
                "未找到 Agent 命令，请先安装对应 CLI 并将其加入 PATH 后重新打开小岛".into(),
            );
        }
    }
    let cmdline = cmd
        .iter()
        .map(|arg| {
            if arg.contains(' ') && !arg.starts_with('"') {
                format!("\"{}\"", arg.replace('"', "\"\""))
            } else {
                arg.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
        let mut builder = std::process::Command::new("cmd.exe");
        builder
            .arg("/K")
            .arg(&cmdline)
            .creation_flags(CREATE_NEW_CONSOLE);
        if let Some(dir) = cwd {
            builder.current_dir(dir);
        }
        builder
            .spawn()
            .map_err(|e| format!("failed to restart agent: {e}"))?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = cmdline;
        let _ = cwd;
        return Err("not supported on this platform".into());
    }
    Ok(())
}

pub(crate) fn resume_command_for(name: &str, session_id: &str, base: &[String]) -> Vec<String> {
    let exe = base.first().cloned().unwrap_or_else(|| name.to_string());
    if !session_id.is_empty() {
        if let Some(def) = agent_defs().iter().find(|d| d.name == name) {
            if !def.resume_args.is_empty() {
                return fill_template(&def.resume_args, &exe, session_id, None);
            }
        }
    }
    match name {
        "Claude Code" => vec![exe, "--resume".into(), session_id.into(), "继续".into()],
        "Codex CLI" => {
            let mut cmd = base.to_vec();
            cmd.push("resume".into());
            cmd.push(session_id.into());
            cmd
        }
        "OpenCode" => {
            let mut cmd = base.to_vec();
            cmd.push("--continue".into());
            cmd
        }
        "Hermes" => vec![exe, "--resume".into(), session_id.into()],
        _ => base.to_vec(),
    }
}

pub(crate) fn send_command_for(
    name: &str,
    session_id: &str,
    prompt: &str,
    base: &[String],
) -> Result<Vec<String>, String> {
    let exe = base.first().cloned().unwrap_or_else(|| name.to_string());
    let is_builtin = matches!(name, "Claude Code" | "Codex CLI" | "OpenCode" | "Hermes");
    if !is_builtin {
        if let Some(def) = agent_defs().iter().find(|d| d.name == name) {
            if !def.send_args.is_empty() {
                return Ok(fill_template(
                    &def.send_args,
                    &exe,
                    session_id,
                    Some(prompt),
                ));
            }
        }
    }
    match name {
        "Claude Code" => {
            let mut cmd = vec![exe, "-p".into(), prompt.into()];
            if !session_id.is_empty() {
                cmd.push("--resume".into());
                cmd.push(session_id.into());
            }
            Ok(cmd)
        }
        "Codex CLI" => {
            let mut cmd = base.to_vec();
            if session_id.is_empty() {
                cmd.push("exec".into());
            } else {
                cmd.push("exec".into());
                cmd.push("resume".into());
                cmd.push(session_id.into());
            }
            cmd.push(prompt.into());
            Ok(cmd)
        }
        "OpenCode" => {
            let mut cmd = base.to_vec();
            cmd.push("run".into());
            if !session_id.is_empty() {
                cmd.push("--session".into());
                cmd.push(session_id.into());
            }
            cmd.push(prompt.into());
            Ok(cmd)
        }
        "Hermes" => {
            let mut cmd = vec![exe];
            if !session_id.is_empty() {
                cmd.push("--resume".into());
                cmd.push(session_id.into());
            }
            cmd.push("-z".into());
            cmd.push(prompt.into());
            Ok(cmd)
        }
        _ => Err("该 Agent 暂不支持发消息".into()),
    }
}

pub(crate) fn fallback_send_command(
    name: &str,
    session_id: &str,
    prompt: &str,
) -> Result<Vec<String>, String> {
    let cli = match name {
        "Claude Code" => "claude",
        "Codex CLI" => "codex",
        "OpenCode" => "opencode",
        "Hermes" => "hermes",
        _ => return Err("该 Agent 暂不支持发消息".into()),
    };
    let mut args: Vec<String> = vec!["/C".into(), cli.into()];
    match name {
        "Claude Code" => {
            args.push("-p".into());
            args.push(prompt.into());
            if !session_id.is_empty() {
                args.push("--resume".into());
                args.push(session_id.into());
            }
        }
        "Codex CLI" => {
            if session_id.is_empty() {
                args.push("exec".into());
            } else {
                args.push("exec".into());
                args.push("resume".into());
                args.push(session_id.into());
            }
            args.push(prompt.into());
        }
        "OpenCode" => {
            args.push("run".into());
            if !session_id.is_empty() {
                args.push("--session".into());
                args.push(session_id.into());
            }
            args.push(prompt.into());
        }
        "Hermes" => {
            if !session_id.is_empty() {
                args.push("--resume".into());
                args.push(session_id.into());
            }
            args.push("-z".into());
            args.push(prompt.into());
        }
        _ => return Err("该 Agent 暂不支持发消息".into()),
    }
    let mut cmd = vec!["cmd.exe".into()];
    cmd.extend(args);
    Ok(cmd)
}

pub(crate) fn looks_like_shim(program: &str) -> bool {
    let lower = program.to_lowercase();
    !lower.contains('\\')
        && !lower.contains('/')
        && !lower.ends_with(".exe")
        && (lower == "claude"
            || lower == "codex"
            || lower == "opencode"
            || lower == "hermes"
            || lower.ends_with(".cmd")
            || lower.ends_with(".ps1"))
}

pub(crate) fn wrap_with_cmd(cmd: &[String]) -> Vec<String> {
    let mut wrapped = vec!["cmd.exe".to_string(), "/C".to_string()];
    wrapped.extend(cmd.iter().cloned());
    wrapped
}

pub(crate) fn clean_agent_base(name: &str, base: &[String]) -> Vec<String> {
    let uses_windows_apps = base
        .first()
        .map(|p| p.contains("WindowsApps"))
        .unwrap_or(false);
    if !uses_windows_apps {
        return base.to_vec();
    }
    let cli = match name {
        "Claude Code" => "claude",
        "Codex CLI" => "codex",
        "OpenCode" => "opencode",
        "Hermes" => "hermes",
        _ => return base.to_vec(),
    };
    vec![cli.to_string()]
}

#[tauri::command]
pub(crate) fn stop_agent(name: String, state: tauri::State<AppState>) -> Result<(), String> {
    let mut sys = state
        .sys
        .lock()
        .map_err(|_| "state lock error".to_string())?;
    let pids = find_agent_pids(&mut sys, &name)?;
    if pids.is_empty() {
        return Err("agent is not running".into());
    }
    for pid in pids {
        let _ = quiet_command("taskkill.exe")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .spawn();
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn restart_agent(name: String, state: tauri::State<AppState>) -> Result<(), String> {
    let session = state
        .session
        .lock()
        .map_err(|_| "state lock error".to_string())?;
    let command = session
        .commands
        .get(&name)
        .ok_or_else(|| "no command recorded for this agent".to_string())?;
    let base = clean_agent_base(&name, &command.cmd);
    let cwd = command.cwd.clone();
    drop(session);
    spawn_command_in_dir(&base, cwd.as_deref())
}

#[tauri::command]
pub(crate) fn open_session_terminal(name: String, session_id: String) -> Result<(), String> {
    let dir = find_session_cwd(&name, &session_id)?;
    open_terminal_in_dir(&dir)
}

#[tauri::command]
pub(crate) fn restart_session(
    name: String,
    session_id: String,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    let dir = find_session_cwd(&name, &session_id)?;
    let session = state
        .session
        .lock()
        .map_err(|_| "state lock error".to_string())?;
    let base = session
        .commands
        .get(&name)
        .map(|command| clean_agent_base(&name, &command.cmd));
    let resume_cmd = interactive_resume_command(&name, &session_id, base.as_deref())?;
    drop(session);
    spawn_command_in_dir(&resume_cmd, Some(&dir))
}

pub(crate) fn interactive_resume_command(
    name: &str,
    session_id: &str,
    base: Option<&[String]>,
) -> Result<Vec<String>, String> {
    if session_id.trim().is_empty() {
        return Err("缺少要恢复的会话编号".into());
    }
    // Codex interactive resume is verified against the installed CLI help.
    // Other providers retain their existing configured resume arguments.
    if let Some(cli) = match name {
        "Claude Code" => Some("claude"),
        "OpenCode" => Some("opencode"),
        "Hermes" => Some("hermes"),
        _ => None,
    } {
        return Ok(resume_command_for(name, session_id, &[cli.to_string()]));
    }
    let (cli, args): (&str, &[&str]) = match name {
        "Codex CLI" => ("codex", &["resume"]),
        _ => {
            let base = base
                .filter(|cmd| !cmd.is_empty())
                .ok_or("该 Agent 未记录启动命令，暂不支持恢复会话")?;
            let supported = agent_defs()
                .iter()
                .any(|def| def.name == name && !def.resume_args.is_empty());
            if !supported {
                return Err("该 Agent 未配置会话恢复命令".into());
            }
            return Ok(resume_command_for(name, session_id, base));
        }
    };
    let mut command = vec![cli.to_string()];
    command.extend(args.iter().map(|arg| arg.to_string()));
    command.push(session_id.to_string());
    Ok(command)
}

#[tauri::command]
pub(crate) fn send_to_session(
    name: String,
    session_id: String,
    prompt: String,
    state: tauri::State<AppState>,
) -> Result<String, String> {
    let prompt = prompt.trim().to_string();
    if prompt.is_empty() {
        return Err("prompt is empty".into());
    }
    let (cmd, dir) = {
        let session = state
            .session
            .lock()
            .map_err(|_| "state lock error".to_string())?;
        let dir = if session_id.is_empty() {
            None
        } else {
            find_session_cwd(&name, &session_id).ok()
        };
        let cmd = match session.commands.get(&name) {
            Some(command) => {
                let base = clean_agent_base(&name, &command.cmd);
                send_command_for(&name, &session_id, &prompt, &base)?
            }
            None => fallback_send_command(&name, &session_id, &prompt)?,
        };
        let dir = dir.or_else(|| session.commands.get(&name).and_then(|c| c.cwd.clone()));
        (cmd, dir)
    };
    let cmd = if looks_like_shim(cmd.first().map(|s| s.as_str()).unwrap_or("")) {
        wrap_with_cmd(&cmd)
    } else {
        cmd
    };

    let program = cmd.first().ok_or_else(|| "empty command".to_string())?;
    let mut command = std::process::Command::new(program);
    command.args(&cmd[1..]);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    if let Some(dir) = dir {
        command.current_dir(dir);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("failed to start agent ({program}): {e}"))?;

    let task = Arc::new(SendTask {
        lines: Mutex::new(Vec::new()),
        done: AtomicBool::new(false),
    });
    let task_id = format!(
        "send-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    state
        .send_tasks
        .lock()
        .map_err(|_| "state lock error".to_string())?
        .insert(task_id.clone(), task.clone());

    if let Some(stdout) = child.stdout.take() {
        let t = task.clone();
        std::thread::spawn(move || read_pipe_to_task(stdout, t));
    }
    if let Some(stderr) = child.stderr.take() {
        let t = task.clone();
        std::thread::spawn(move || read_pipe_to_task(stderr, t));
    }
    std::thread::spawn(move || {
        let _ = child.wait();
        task.done.store(true, Ordering::SeqCst);
    });

    Ok(task_id)
}

pub(crate) fn read_pipe_to_task<R: Read + Send + 'static>(reader: R, task: Arc<SendTask>) {
    let reader = BufReader::new(reader);
    for line in reader.lines() {
        if let Ok(line) = line {
            if let Ok(mut lines) = task.lines.lock() {
                lines.push(line);
            }
        }
    }
}

#[derive(Serialize)]
pub(crate) struct SendOutput {
    done: bool,
    lines: Vec<String>,
}

#[tauri::command]
pub(crate) fn get_send_output(
    task_id: String,
    state: tauri::State<AppState>,
) -> Result<SendOutput, String> {
    let mut tasks = state
        .send_tasks
        .lock()
        .map_err(|_| "state lock error".to_string())?;
    let task = tasks
        .get(&task_id)
        .ok_or_else(|| "task not found".to_string())?;
    let done = task.done.load(Ordering::SeqCst);
    let lines = task.lines.lock_recover().clone();
    if done {
        tasks.remove(&task_id);
    }
    Ok(SendOutput { done, lines })
}

#[tauri::command]
pub(crate) fn stop_session(
    name: String,
    session_id: String,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    let dir = find_session_cwd(&name, &session_id)?;
    let mut sys = state
        .sys
        .lock()
        .map_err(|_| "state lock error".to_string())?;
    let keyword = keyword_for(&name).ok_or_else(|| format!("unknown agent: {name}"))?;
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let pids: Vec<u32> = matching_processes(&sys, &keyword)
        .iter()
        .filter(|p| {
            p.cwd()
                .map(|c| paths_match(c.to_string_lossy().as_ref(), &dir))
                .unwrap_or(false)
        })
        .map(|p| p.pid().as_u32())
        .collect();
    if pids.is_empty() {
        return Err("no process found for this session".into());
    }
    for pid in pids {
        let _ = quiet_command("taskkill.exe")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .spawn();
    }
    Ok(())
}

pub(crate) const AUTOSTART_NAME: &str = "AgentIsland";

#[cfg(target_os = "windows")]
pub(crate) fn autostart_enabled() -> bool {
    let mut command = quiet_command("reg.exe");
    command.args([
        "query",
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
        "/v",
        AUTOSTART_NAME,
    ]);
    command
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn autostart_enabled() -> bool {
    false
}

#[cfg(target_os = "windows")]
pub(crate) fn apply_autostart(enabled: bool) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    if enabled {
        let value = format!("\"{}\"", exe.display());
        let status = quiet_command("reg.exe")
            .args([
                "add",
                key,
                "/v",
                AUTOSTART_NAME,
                "/t",
                "REG_SZ",
                "/d",
                &value,
                "/f",
            ])
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err("failed to enable autostart".into());
        }
    } else {
        let status = quiet_command("reg.exe")
            .args(["delete", key, "/v", AUTOSTART_NAME, "/f"])
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err("failed to disable autostart".into());
        }
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn apply_autostart(enabled: bool) -> Result<(), String> {
    let _ = enabled;
    Err("not supported on this platform".into())
}

#[tauri::command]
pub(crate) fn get_autostart() -> bool {
    autostart_enabled()
}

#[tauri::command]
pub(crate) fn set_autostart(enabled: bool) -> Result<(), String> {
    apply_autostart(enabled)
}

pub(crate) const RECORDING_KEYWORDS: &[&str] = &[
    "obs64",
    "obs32",
    "obs",
    "zoom",
    "teams",
    "discord",
    "wechat",
    "weixin",
    "wemeet",
    "wemeetapp",
    "tencentmeeting",
    "gamebarpresencewriter",
    "bandicam",
    "sharex",
    "fraps",
];

pub(crate) fn is_recording_process(name: &str) -> bool {
    let lowered = name.to_lowercase();
    RECORDING_KEYWORDS
        .iter()
        .any(|keyword| lowered.contains(keyword))
}

#[tauri::command]
pub(crate) fn privacy_active(state: tauri::State<AppState>) -> bool {
    let mut sys = state.sys.lock_recover();
    sys.refresh_processes(ProcessesToUpdate::All, false);
    sys.processes()
        .values()
        .any(|p| is_recording_process(&p.name().to_string_lossy()))
}

#[cfg(test)]
mod tests {
    use super::is_recording_process;

    #[test]
    fn recording_detection_prefers_extra_masks_over_a_missed_recorder() {
        assert!(is_recording_process("obs64.exe"));
        assert!(is_recording_process("ShareX.exe"));
        assert!(is_recording_process("Zoom.exe"));
        // Substring matching intentionally over-activates (Obsidian also
        // contains "obs"): an extra mask only hides content, a missed
        // recorder would expose it while capturing the desktop.
        assert!(is_recording_process("Obsidian.exe"));
        assert!(!is_recording_process("notepad.exe"));
        assert!(!is_recording_process("code.exe"));
    }
}

pub(crate) fn toggle_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
        } else {
            let _ = window.show();
            let _ = window.set_focus();
        }
    }
}

#[tauri::command]
pub(crate) fn open_overview(app: tauri::AppHandle) -> Result<(), String> {    let window = app
        .get_webview_window("overview")
        .ok_or("总览窗口尚未创建")?;
    window
        .unminimize()
        .map_err(|error| format!("无法还原总览窗口: {error}"))?;
    window
        .show()
        .map_err(|error| format!("无法显示总览窗口: {error}"))?;
    // The capsule itself is topmost, so an ordinary focused window remains behind it.
    window
        .set_always_on_top(true)
        .map_err(|error| format!("无法置顶总览窗口: {error}"))?;
    window
        .set_focus()
        .map_err(|error| format!("无法聚焦总览窗口: {error}"))?;
    Ok(())
}

#[tauri::command]
pub(crate) fn open_team(app: tauri::AppHandle) -> Result<(), String> {
    open_overview(app.clone())?;
    use tauri::Emitter;
    let _ = app.emit("open-team", ());
    Ok(())
}

#[cfg(target_os = "windows")]
pub(crate) fn start_global_hotkeys(app: tauri::AppHandle) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, MOD_ALT, MOD_CONTROL};
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, TranslateMessage, MSG, WM_HOTKEY,
    };
    std::thread::spawn(move || unsafe {
        let toggle_id = 1;
        let overview_id = 2;
        if RegisterHotKey(None, toggle_id, MOD_CONTROL | MOD_ALT, b'I' as u32).is_err() {
            eprintln!("failed to register Ctrl+Alt+I");
        }
        if RegisterHotKey(None, overview_id, MOD_CONTROL | MOD_ALT, b'O' as u32).is_err() {
            eprintln!("failed to register Ctrl+Alt+O");
        }
        let mut msg = MSG::default();
        loop {
            let result = GetMessageW(&mut msg, None, 0, 0);
            if result.0 == 0 || result.0 == -1 {
                break;
            }
            if msg.message == WM_HOTKEY {
                let id = msg.wParam.0 as i32;
                if id == toggle_id {
                    toggle_window(&app);
                } else if id == overview_id {
                    let _ = open_overview(app.clone());
                }
            }
            let _ = TranslateMessage(&msg);
            let _ = DispatchMessageW(&msg);
        }
    });
}

