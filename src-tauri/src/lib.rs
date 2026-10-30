mod adapters;
mod agent_catalog;
mod agent_tasks;
mod approval_review;
mod approval_rules;
mod bindings;
mod application;
mod commands;
mod domain;
mod external;
mod hooks;
mod hooked;
mod hotkey;
mod inbox_store;
mod launcher;
mod logging;
mod opencode_events;
mod orchestrator;
pub mod interface;
mod process_scan;
mod session_scan;
mod session_search;
mod settings_store;
#[cfg(target_os = "windows")]
mod single_instance;
mod stats_store;
mod terminal;
mod updates;
mod usage;

use adapters::AgentAdapter;
use agent_tasks::*;
use approval_review::*;
use approval_rules::*;
use bindings::*;
use commands::*;
use external::*;
use hooks::*;
use hooked::*;
use hotkey::*;
use inbox_store::*;
use logging::*;
use opencode_events::*;
use orchestrator::*;
use process_scan::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use session_scan::*;
use session_search::*;
use settings_store::*;
use stats_store::*;
use terminal::*;
use updates::*;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use tauri::menu::{CheckMenuItem, Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::Manager;
use usage::*;

/// Recovers shared state even when another thread panicked while holding it.
///
/// The release profile used to abort the whole process on any panic, so one bad
/// hook request or log record could take the resident island down. Locks now
/// recover the value instead of propagating the panic, and unwinding stays
/// contained to the failing thread.
trait LockRecover<T> {
    fn lock_recover(&self) -> std::sync::MutexGuard<'_, T>;
}

impl<T> LockRecover<T> for Mutex<T> {
    fn lock_recover(&self) -> std::sync::MutexGuard<'_, T> {
        self.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub status: String,
    pub display_status: domain::DisplayStatus,
    pub pid: Option<u32>,
    pub cpu: Option<f32>,
    pub memory: Option<f32>,
    pub uptime: Option<u64>,
    pub cwd: Option<String>,
    pub sessions: usize,
    pub last_active_secs: Option<u64>,
    pub log_path: Option<String>,
    pub recent_output: Vec<String>,
    pub current_file: Option<String>,
    pub log_status: Option<String>,
    pub alert: Option<String>,
    pub can_restart: bool,
    pub stats: Option<AgentStats>,
    pub session_count: usize,
    pub session_list: Vec<AgentSession>,
    pub active_session: Option<interface::snapshot::SessionView>,
    pub history_sessions: Option<Vec<interface::snapshot::SessionSummary>>,
    pub freshness: domain::Freshness,
    pub usage: Option<UsageInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UsageInfo {
    pub tokens_total: u64,
    pub tokens_output: u64,
    pub cost_usd: Option<f64>,
    pub used_percent: Option<f32>,
    pub window_secs: u64,
    pub resets_at_secs: Option<u64>,
    pub credits: Option<f64>,
    pub unlimited: Option<bool>,
    pub stale: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentStats {
    pub total_seconds: u64,
    pub error_count: u32,
    pub done_count: u32,
    pub last_status: Option<String>,
    #[serde(default)]
    pub recent_terminal_hashes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentSession {
    pub id: String,
    pub name: String,
    pub cwd: Option<String>,
    pub log_path: Option<String>,
    pub recent_output: Vec<String>,
    pub current_file: Option<String>,
    pub log_status: Option<String>,
    pub alert: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct SessionScan {
    desktop_root_paths: Vec<String>,
    legacy_sessions: Vec<AgentSession>,
    candidates: Vec<interface::snapshot::SessionCandidate>,
    diagnostics: Vec<interface::diagnostics::DiagnosticRecord>,
    acquisition: AcquisitionCompleteness,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum AcquisitionCompleteness {
    #[default]
    Complete,
    Incomplete,
}

impl SessionScan {
    fn record_issue(&mut self, adapter: &str, issue: domain::DataIssue) {
        self.acquisition = AcquisitionCompleteness::Incomplete;
        self.diagnostics
            .push(interface::diagnostics::DiagnosticRecord {
                view: interface::diagnostics::DiagnosticView::new(
                    adapter,
                    issue,
                    domain::Freshness {
                        observed_at_ms: epoch_millis(),
                        stale: true,
                    },
                    None,
                    0,
                    0,
                ),
                session_id: None,
                project_path: None,
                message_length: 0,
            });
    }
}

fn io_issue(error: &std::io::Error) -> domain::DataIssue {
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        domain::DataIssue::PermissionDenied
    } else {
        domain::DataIssue::LogUnavailable
    }
}

#[derive(Debug, Clone)]
struct AgentCommand {
    cwd: Option<String>,
    cmd: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct AgentDef {
    name: String,
    keyword: String,
    #[serde(default)]
    log_kind: String,
    /// 实际可执行文件/命令；留空时退回 keyword（兼容旧 agents.json）。
    #[serde(default)]
    program: String,
    #[serde(default)]
    resume_args: Vec<String>,
    #[serde(default)]
    send_args: Vec<String>,
    /// 来源：builtin（默认四个）/ custom（agents.json）/ detected（启动时从 PATH 发现）。
    /// 旧 agents.json 没有这个字段，载入时按名字补。
    #[serde(default)]
    source: String,
}

impl AgentDef {
    fn program(&self) -> &str {
        let program = self.program.trim();
        if program.is_empty() {
            self.keyword.as_str()
        } else {
            program
        }
    }

    /// 去重键：同一个可执行文件只应出现一次（agents.json 与自动发现可能重复）。
    fn dedupe_key(&self) -> String {
        self.program().trim().to_ascii_lowercase()
    }

    fn is_detected(&self) -> bool {
        self.source == "detected"
    }
}

fn agents_path() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".agent-island")
        .join("agents.json")
}

static AGENT_DEFS: OnceLock<Mutex<Option<(Instant, Vec<AgentDef>)>>> = OnceLock::new();

fn default_agent_defs() -> Vec<AgentDef> {
    let mut defs = vec![
        AgentDef {
            name: "Claude Code".into(),
            keyword: "claude".into(),
            log_kind: "claude".into(),
            program: String::new(),
            resume_args: vec![
                "{exe}".into(),
                "--resume".into(),
                "{session}".into(),
                "继续".into(),
            ],
            send_args: vec![
                "{exe}".into(),
                "-p".into(),
                "{prompt}".into(),
                "--resume".into(),
                "{session}".into(),
            ],
            source: "builtin".into(),
        },
        AgentDef {
            name: "Codex CLI".into(),
            keyword: "codex".into(),
            log_kind: "codex".into(),
            program: String::new(),
            resume_args: vec![
                "{exe}".into(),
                "exec".into(),
                "resume".into(),
                "{session}".into(),
            ],
            send_args: vec![
                "{exe}".into(),
                "exec".into(),
                "resume".into(),
                "{session}".into(),
                "{prompt}".into(),
            ],
            source: "builtin".into(),
        },
        AgentDef {
            name: "OpenCode".into(),
            keyword: "opencode".into(),
            log_kind: "opencode".into(),
            program: String::new(),
            resume_args: vec![
                "{exe}".into(),
                "run".into(),
                "-s".into(),
                "{session}".into(),
            ],
            send_args: vec![
                "{exe}".into(),
                "run".into(),
                "-s".into(),
                "{session}".into(),
                "{prompt}".into(),
            ],
            source: "builtin".into(),
        },
        AgentDef {
            name: "Hermes".into(),
            keyword: "hermes".into(),
            log_kind: "hermes".into(),
            program: String::new(),
            resume_args: vec!["{exe}".into(), "--resume".into(), "{session}".into()],
            send_args: vec![
                "{exe}".into(),
                "--resume".into(),
                "{session}".into(),
                "-z".into(),
                "{prompt}".into(),
            ],
            source: "builtin".into(),
        },
    ];
    for def in &mut defs {
        def.source = "builtin".into();
    }
    defs
}

fn load_agent_defs() -> Vec<AgentDef> {
    let path = agents_path();
    let mut defs: Vec<AgentDef> = Vec::new();
    if let Ok(text) = fs::read_to_string(&path) {
        if let Ok(parsed) = serde_json::from_str::<Vec<AgentDef>>(&text) {
            defs = parsed;
        }
    }
    // 旧 agents.json 没有 source：按名字区分内置与自定义
    for def in &mut defs {
        if def.source.is_empty() {
            def.source = if is_builtin_agent_name(&def.name) {
                "builtin".into()
            } else {
                "custom".into()
            };
        }
    }
    // 文件里缺少的内置 Agent 继续保留（用户可能只写了自己的自定义项）
    let mut seen: std::collections::HashSet<String> =
        defs.iter().map(AgentDef::dedupe_key).collect();
    for def in default_agent_defs() {
        if seen.insert(def.dedupe_key()) {
            defs.push(def);
        }
    }
    defs
}

fn is_builtin_agent_name(name: &str) -> bool {
    matches!(name, "Claude Code" | "Codex CLI" | "OpenCode" | "Hermes")
}

fn save_agent_defs(defs: &[AgentDef]) -> Result<(), String> {
    let path = agents_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建 Agent 配置目录失败：{error}"))?;
    }
    if path.is_file() {
        let backup = path.with_extension("json.bak");
        if !backup.is_file() {
            fs::copy(&path, &backup).map_err(|error| format!("备份 Agent 配置失败：{error}"))?;
        }
    }
    let body = serde_json::to_string_pretty(defs).map_err(|error| error.to_string())?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, body).map_err(|error| format!("写入 Agent 配置失败：{error}"))?;
    fs::rename(&temp, &path).map_err(|error| format!("替换 Agent 配置失败：{error}"))?;
    Ok(())
}

fn agent_defs() -> Vec<AgentDef> {
    let cache = AGENT_DEFS.get_or_init(|| Mutex::new(None));
    if let Ok(guard) = cache.lock() {
        if let Some((at, defs)) = guard.as_ref() {
            if at.elapsed().as_secs() < 5 {
                return defs.clone();
            }
        }
    }
    let defs = load_agent_defs();
    if let Ok(mut guard) = cache.lock() {
        *guard = Some((Instant::now(), defs.clone()));
    }
    defs
}

#[tauri::command]
fn reload_agent_defs() {
    if let Ok(mut guard) = AGENT_DEFS.get_or_init(|| Mutex::new(None)).lock() {
        *guard = None;
    }
}

/// 扫描用的 Agent 列表：显式配置 + 本机探测到的（source=detected）。
///
/// 自动发现的项只活在内存里，不写回 `agents.json`：用户没要求就不要替他把
/// 配置固化下来，卸载/更新后自然消失。
pub(crate) fn scan_agent_defs() -> Vec<AgentDef> {
    let mut defs = agent_defs();
    let mut seen: std::collections::HashSet<String> =
        defs.iter().map(AgentDef::dedupe_key).collect();
    let mut names: std::collections::HashSet<String> = defs
        .iter()
        .map(|def| def.name.to_ascii_lowercase())
        .collect();
    for entry in crate::agent_catalog::CATALOG {
        let program = entry.program.to_ascii_lowercase();
        if seen.contains(&program) || names.contains(&entry.name.to_ascii_lowercase()) {
            continue;
        }
        if crate::process_scan::resolve_agent_program(entry.program).is_none() {
            continue;
        }
        seen.insert(program);
        names.insert(entry.name.to_ascii_lowercase());
        defs.push(AgentDef {
            name: entry.name.to_string(),
            keyword: entry.program.to_string(),
            log_kind: String::new(),
            program: entry.program.to_string(),
            resume_args: Vec::new(),
            send_args: Vec::new(),
            source: "detected".into(),
        });
    }
    defs
}

/// agent_id（log_kind 优先，否则 keyword）→ (来源, 启动命令)。
/// 给快照补 source/program 用；program 是终端里可以直接启动的命令或完整路径。
pub(crate) fn agent_meta_map() -> std::collections::HashMap<String, (String, String)> {
    scan_agent_defs()
        .into_iter()
        .map(|def| {
            let id = if def.log_kind.is_empty() {
                def.keyword.clone()
            } else {
                def.log_kind.clone()
            };
            let program = def.program().to_string();
            (id, (def.source, program))
        })
        .collect()
}

fn fill_template(
    template: &[String],
    exe: &str,
    session: &str,
    prompt: Option<&str>,
) -> Vec<String> {
    template
        .iter()
        .map(|arg| {
            arg.replace("{exe}", exe)
                .replace("{session}", session)
                .replace("{prompt}", prompt.unwrap_or(""))
        })
        .filter(|s| !s.is_empty())
        .collect()
}

struct ActivityState {
    last_cpu: f32,
    last_active: Instant,
}

struct SessionState {
    activity: HashMap<String, ActivityState>,
    index_cache: session_scan::SessionIndexCache,
    commands: HashMap<String, AgentCommand>,
    stats: HashMap<String, AgentStats>,
    daily: HashMap<String, HashMap<String, AgentStats>>,
    runtime_start: HashMap<String, Instant>,
    last_poll: Instant,
    cache: AgentDataCache,
    last_save: Instant,
}

type AgentDataCache = Option<(
    Instant,
    Vec<AgentInfo>,
    interface::snapshot::AgentViewSnapshot,
    interface::diagnostics::DiagnosticSnapshot,
)>;

struct SendTask {
    lines: Mutex<Vec<String>>,
    done: AtomicBool,
}

struct AppState {
    sys: Mutex<System>,
    session: Mutex<SessionState>,
    stats_path: PathBuf,
    daily_path: PathBuf,
    send_tasks: Mutex<HashMap<String, Arc<SendTask>>>,
    terminals: TerminalMap,
    hook_enabled: AtomicBool,
    hook_token: Mutex<String>,
    approvals: Mutex<HashMap<String, HookApproval>>,
    /// 临时授权（允许 10 分钟 / 本会话），只存在内存，进程退出即失效。
    approvals_grants: Mutex<Vec<TemporaryGrant>>,
    /// 收件箱：审批 / 通知 / 完成事件的本地持久队列。
    inbox_path: PathBuf,
    inbox: Mutex<Vec<InboxItem>>,
    /// 手动会话绑定（agent_id → session），持久化在 ~/.agent-island/session-bindings.json
    bindings: Mutex<HashMap<String, BoundSession>>,
    /// Agent 通过 hooks 自报的会话身份（agent_id → 最近一次上报）
    hooked: Mutex<HashMap<String, HookedSession>>,
    /// 岛内任务模式：`codex exec --json` 的运行实例
    tasks: Mutex<HashMap<String, std::sync::Arc<std::sync::Mutex<agent_tasks::TaskRuntime>>>>,
    /// 团队任务：主 Agent 规划 → 派发 → 汇总
    team: TeamState,
}

#[derive(Debug, Clone, Serialize)]
struct HookApproval {
    id: String,
    session: String,
    tool: String,
    command: String,
    cwd: String,
    /// 用于临时授权 / 记住这条的匹配键：Bash 是命令，Edit/Write 是文件路径。
    rule_key: String,
    decision: Option<String>,
    /// 拒绝理由：会回传给 Agent，让它按理由改道。
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    /// 本地规则评估结果（风险、仓库、分支、diff 预览）。
    #[serde(skip_serializing_if = "Option::is_none")]
    review: Option<ApprovalReview>,
    created_at_ms: u64,
    expires_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HookConfigFile {
    port: u16,
    token: String,
    #[serde(default)]
    enabled: bool,
}

fn load_hook_config() -> Option<HookConfigFile> {
    let text = fs::read_to_string(hook_config_path()).ok()?;
    let text = text.trim_start_matches('\u{feff}');
    serde_json::from_str(text).ok()
}

#[derive(Debug, Clone)]
struct LogSnapshot {
    path: String,
    recent: Vec<String>,
    file: Option<String>,
    cwd: Option<String>,
    log_status: Option<String>,
    alert: Option<String>,
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
}

fn codex_data_root_from(
    configured: Option<&std::ffi::OsStr>,
    home: Option<&Path>,
) -> Option<PathBuf> {
    configured
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| home.map(|path| path.join(".codex")))
}

fn codex_data_root() -> Option<PathBuf> {
    codex_data_root_from(
        std::env::var_os("CODEX_HOME").as_deref(),
        home_dir().as_deref(),
    )
}

#[cfg(test)]
mod tests;

pub fn run() {
    // A resident island must stay single-instance: the duplicate would double the
    // window, the poller and the statistics writes, and it could never receive
    // hook events because the hook server port belongs to the first process.
    // The guard lives for the whole event loop.
    #[cfg(target_os = "windows")]
    let _instance_guard = match single_instance::acquire(single_instance::ISLAND_MUTEX) {
        Ok(guard) => guard,
        Err(single_instance::AlreadyRunning) => {
            single_instance::focus_running_island();
            return;
        }
    };
    let stats_path = stats_path();
    let daily_path = daily_path();
    let inbox_path = inbox_path();
    let hook_cfg = load_hook_config();
    let hook_enabled = hook_cfg.as_ref().map(|c| c.enabled).unwrap_or(false);
    let hook_token = hook_cfg.map(|c| c.token).unwrap_or_default();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState {
            sys: Mutex::new(System::new_all()),
            session: Mutex::new(SessionState {
                activity: HashMap::new(),
                index_cache: session_scan::SessionIndexCache::default(),
                commands: HashMap::new(),
                stats: load_stats(&stats_path),
                daily: load_daily(&daily_path),
                runtime_start: HashMap::new(),
                last_poll: Instant::now(),
                cache: None,
                last_save: Instant::now(),
            }),
            stats_path,
            daily_path,
            send_tasks: Mutex::new(HashMap::new()),
            terminals: Mutex::new(HashMap::new()),
            hook_enabled: AtomicBool::new(hook_enabled),
            hook_token: Mutex::new(hook_token),
            approvals: Mutex::new(HashMap::new()),
            approvals_grants: Mutex::new(Vec::new()),
            inbox_path: inbox_path.clone(),
            inbox: Mutex::new(load_inbox(&inbox_path)),
            bindings: Mutex::new(load_bindings(&bindings_path())),
            hooked: Mutex::new(HashMap::new()),
            tasks: Mutex::new(HashMap::new()),
            team: TeamState::load(),
        })
        .invoke_handler(tauri::generate_handler![
            get_agents,
            get_agent_snapshot,
            get_diagnostics,
            export_diagnostics,
            get_stats_report,
            reload_agent_defs,
            open_project_dir,
            open_path,
            open_terminal,
            stop_agent,
            restart_agent,
            open_session_terminal,
            restart_session,
            stop_session,
            send_to_session,
            get_send_output,
            terminal_open,
            terminal_write,
            terminal_resize,
            terminal_close,
            terminal_list,
            probe_cli_version,
            load_app_settings,
            read_recent_logs,
            open_external,
            search_sessions_command,
            load_session_by_path,
            bind_session,
            unbind_session,
            list_session_bindings,
            start_agent_task,
            list_agent_tasks,
            cancel_agent_task,
            team_capabilities,
            team_start_plan,
            team_get_run,
            team_list_runs,
            team_confirm_plan,
            team_cancel_run,
            reveal_session,
            list_approval_rules,
            add_approval_rule,
            remove_approval_rule,
            list_approval_audit,
            list_inbox,
            resolve_inbox_item,
            snooze_inbox_item,
            detect_agents,
            refresh_agent_detection,
            detect_agent_candidates,
            add_agent_definition,
            save_app_settings,
            get_autostart,
            set_autostart,
            privacy_active,
            open_overview,
            get_hook_status,
            set_hook_enabled,
            respond_hook_approval,
            dismiss_hook_approval,
            focus_agent_terminal
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(|app| {
            log_event("INFO", "app", &format!("started version={}", env!("CARGO_PKG_VERSION")));
            // 全局快捷键：Ctrl+Shift+A 唤出 / 收起
            spawn_toggle_hotkey(app.handle().clone());
            #[cfg(target_os = "windows")]
            {
                use windows::Win32::Foundation::{HWND, RECT};
                use windows::Win32::UI::WindowsAndMessaging::{
                    GetWindowRect, SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE,
                };

                if let Some(window) = app.get_webview_window("main") {
                    if let Ok(hwnd) = window.hwnd() {
                        let raw = hwnd.0 as isize;
                        // Background thread: lock y to 0, no initial SetWindowPos
                        std::thread::spawn(move || {
                            let hwnd2 = HWND(raw as *mut _);
                            let debug = std::env::var_os("AGENT_ISLAND_SCAN_DEBUG").is_some();
                            let mut corrections = 0u32;
                            loop {
                                std::thread::sleep(std::time::Duration::from_millis(150));
                                unsafe {
                                    let mut rc: RECT = std::mem::zeroed();
                                    if GetWindowRect(hwnd2, &mut rc).is_err() {
                                        continue;
                                    }
                                    if rc.top != 0 {
                                        corrections += 1;
                                        if debug && corrections <= 20 {
                                            eprintln!("[pin] correcting top={} left={}", rc.top, rc.left);
                                        }
                                        let w = rc.right - rc.left;
                                        let h = rc.bottom - rc.top;
                                        let _ = SetWindowPos(
                                            hwnd2,
                                            HWND_TOPMOST,
                                            rc.left,
                                            0,
                                            w,
                                            h,
                                            SWP_NOACTIVATE,
                                        );
                                    }
                                }
                            }
                        });
                    }
                }
            }

            let toggle = MenuItem::with_id(app, "toggle", "显示/隐藏", true, None::<&str>)?;
            let autostart = CheckMenuItem::with_id(
                app,
                "autostart",
                "开机自启",
                true,
                autostart_enabled(),
                None::<&str>,
            )?;
            let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&toggle, &autostart, &quit])?;
            let autostart_item = autostart.clone();

            let mut tray = TrayIconBuilder::new();
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.tooltip("Agent Island")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(move |app, event| match event.id.as_ref() {
                    "toggle" => toggle_window(app),
                    "autostart" => {
                        let next = !autostart_enabled();
                        match apply_autostart(next) {
                            Ok(()) => {
                                let _ = autostart_item.set_checked(next);
                            }
                            Err(_) => {
                                let _ = autostart_item.set_checked(autostart_enabled());
                            }
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        toggle_window(tray.app_handle());
                    }
                })
                .build(app)?;

            start_hook_server(app.handle().clone());
            // Existing installs may still have the old bridge script (no
            // PermissionRequest support). Refresh script + hook entries at
            // startup without rotating the token or touching user hooks.
            if app.state::<AppState>().hook_enabled.load(Ordering::SeqCst) {
                refresh_installed_hooks();
            }
            #[cfg(target_os = "windows")]
            start_global_hotkeys(app.handle().clone());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if let tauri::RunEvent::Exit = event {
            if let Some(state) = app_handle.try_state::<AppState>() {
                orchestrator::shutdown(&state);
                if let Ok(session) = state.session.lock() {
                    let _ = save_stats(&session.stats, &state.stats_path);
                    let _ = save_daily(&session.daily, &state.daily_path);
                }
                let inbox = state.inbox.lock_recover();
                let _ = save_inbox(&state.inbox_path, &inbox);
            }
        }
    });
}














