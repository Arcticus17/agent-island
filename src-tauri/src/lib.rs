mod adapters;
mod application;
mod commands;
mod domain;
mod hooks;
pub mod interface;
mod process_scan;
mod session_scan;
mod settings_store;
#[cfg(target_os = "windows")]
mod single_instance;
mod stats_store;
mod terminal;
mod updates;
mod usage;

use adapters::AgentAdapter;
use commands::*;
use hooks::*;
use process_scan::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use session_scan::*;
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
    #[serde(default)]
    resume_args: Vec<String>,
    #[serde(default)]
    send_args: Vec<String>,
}

static AGENT_DEFS: OnceLock<Mutex<Option<(Instant, Vec<AgentDef>)>>> = OnceLock::new();

fn default_agent_defs() -> Vec<AgentDef> {
    vec![
        AgentDef {
            name: "Claude Code".into(),
            keyword: "claude".into(),
            log_kind: "claude".into(),
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
        },
        AgentDef {
            name: "Codex CLI".into(),
            keyword: "codex".into(),
            log_kind: "codex".into(),
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
        },
        AgentDef {
            name: "OpenCode".into(),
            keyword: "opencode".into(),
            log_kind: "opencode".into(),
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
        },
        AgentDef {
            name: "Hermes".into(),
            keyword: "hermes".into(),
            log_kind: "hermes".into(),
            resume_args: vec!["{exe}".into(), "--resume".into(), "{session}".into()],
            send_args: vec![
                "{exe}".into(),
                "--resume".into(),
                "{session}".into(),
                "-z".into(),
                "{prompt}".into(),
            ],
        },
    ]
}

fn load_agent_defs() -> Vec<AgentDef> {
    let path = home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".agent-island")
        .join("agents.json");
    if let Ok(text) = fs::read_to_string(&path) {
        if let Ok(defs) = serde_json::from_str::<Vec<AgentDef>>(&text) {
            if !defs.is_empty() {
                return defs;
            }
        }
    }
    default_agent_defs()
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
}

#[derive(Debug, Clone, Serialize)]
struct HookApproval {
    id: String,
    session: String,
    tool: String,
    command: String,
    cwd: String,
    decision: Option<String>,
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
    let hook_cfg = load_hook_config();
    let hook_enabled = hook_cfg.as_ref().map(|c| c.enabled).unwrap_or(false);
    let hook_token = hook_cfg.map(|c| c.token).unwrap_or_default();
    let app = tauri::Builder::default()
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
            #[cfg(target_os = "windows")]
            start_global_hotkeys(app.handle().clone());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if let tauri::RunEvent::Exit = event {
            if let Some(state) = app_handle.try_state::<AppState>() {
                if let Ok(session) = state.session.lock() {
                    let _ = save_stats(&session.stats, &state.stats_path);
                    let _ = save_daily(&session.daily, &state.daily_path);
                }
            }
        }
    });
}


