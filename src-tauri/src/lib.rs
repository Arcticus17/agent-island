mod adapters;
mod application;
mod domain;
pub mod interface;
#[cfg(target_os = "windows")]
mod single_instance;

use adapters::AgentAdapter;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
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

fn keyword_for(name: &str) -> Option<String> {
    agent_defs()
        .iter()
        .find(|d| d.name == name)
        .map(|d| d.keyword.clone())
}

fn matching_processes<'a>(sys: &'a System, keyword: &str) -> Vec<&'a sysinfo::Process> {
    sys.processes()
        .iter()
        .filter(|(_pid, process)| {
            let name = process.name().to_string_lossy().to_lowercase();
            let args = process
                .cmd()
                .iter()
                .map(|s| s.to_string_lossy().to_lowercase())
                .collect::<Vec<_>>();
            matches_agent_process(&name, &args, keyword)
        })
        .map(|(_pid, process)| process)
        .collect()
}

fn matches_agent_process(name: &str, args: &[String], keyword: &str) -> bool {
    // A Codex desktop app-server owns many tasks, not a single CLI session.
    // Never infer CLI ownership from arbitrary prompt text or a parent shell.
    if keyword.eq_ignore_ascii_case("codex") {
        if args.iter().skip(1).any(|arg| arg == "app-server") {
            return false;
        }
        if name.eq_ignore_ascii_case("codex.exe") || name.eq_ignore_ascii_case("codex") {
            return true;
        }
        if !matches!(name, "node" | "node.exe") {
            return false;
        }
        return args.get(1).is_some_and(|entrypoint| {
            let normalized = entrypoint.replace('\\', "/");
            normalized.ends_with("/@openai/codex/bin/codex.js")
        });
    }
    // Preserve configured keyword matching for other/custom providers.
    name.contains(keyword) || args.join(" ").contains(keyword)
}

#[cfg(test)]
mod process_classifier_tests {
    use super::matches_agent_process;

    fn matches(name: &str, args: &[&str], keyword: &str) -> bool {
        matches_agent_process(
            name,
            &args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>(),
            keyword,
        )
    }

    #[test]
    fn excludes_desktop_server_and_code_mode_helpers_from_cli() {
        assert!(!matches(
            "codex.exe",
            &["codex.exe", "app-server", "--listen", "stdio://"],
            "codex"
        ));
        assert!(!matches(
            "codex-code-mode-host.exe",
            &["codex-code-mode-host.exe"],
            "codex"
        ));
    }

    #[test]
    fn ignores_shell_prompts_and_unrelated_node_arguments() {
        assert!(!matches(
            "powershell.exe",
            &["powershell.exe", "-Command", "echo codex"],
            "codex"
        ));
        assert!(!matches(
            "node.exe",
            &["node.exe", "server.js", "codex"],
            "codex"
        ));
    }

    #[test]
    fn accepts_native_cli_and_official_node_entrypoint() {
        assert!(matches(
            "codex.exe",
            &["codex.exe", "resume", "session-id"],
            "codex"
        ));
        assert!(matches(
            "node.exe",
            &[
                "node.exe",
                r"C:\Users\中文 用户\node_modules\@openai\codex\bin\codex.js"
            ],
            "codex"
        ));
        assert!(!matches(
            "node.exe",
            &[
                "node.exe",
                "/node_modules/@openai/codex/bin/codex.js",
                "app-server"
            ],
            "codex"
        ));
    }

    #[test]
    fn preserves_custom_keyword_matching() {
        assert!(matches(
            "powershell.exe",
            &["powershell.exe", "custom-agent-probe"],
            "custom-agent-probe"
        ));
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProcessObservation {
    pid: u32,
    project_path: Option<String>,
    started_at_ms: u64,
}

fn process_fact_from_observations(
    agent_id: &str,
    name: &str,
    observations: &[ProcessObservation],
) -> interface::snapshot::ProcessFact {
    let project_path = observations
        .first()
        .and_then(|observation| observation.project_path.clone())
        .filter(|first| {
            let normalized = normalize_process_path(first);
            observations.iter().all(|observation| {
                observation
                    .project_path
                    .as_deref()
                    .map(normalize_process_path)
                    .as_deref()
                    == Some(normalized.as_str())
            })
        });

    interface::snapshot::ProcessFact {
        name: name.into(),
        identity: domain::ProcessIdentity {
            agent_id: agent_id.into(),
            project_path,
            process_ids: observations
                .iter()
                .map(|observation| observation.pid)
                .collect(),
            started_at_ms: observations
                .iter()
                .map(|observation| observation.started_at_ms)
                .max()
                .unwrap_or(0),
        },
        process_state: if observations.is_empty() {
            domain::ProcessState::Stopped
        } else {
            domain::ProcessState::Running
        },
        activity: interface::snapshot::ProcessActivity::Unknown,
    }
}

fn normalize_process_path(path: &str) -> String {
    path.trim_end_matches(['/', '\\'])
        .replace('/', "\\")
        .to_lowercase()
}

/// Process-fact placeholder for the compatibility `AgentInfo` fields.
///
/// The island's authoritative status is derived from structured events in
/// `interface::snapshot`; `apply_snapshot_projection` writes that back onto
/// `AgentInfo`. Text heuristics used to feed these fields and were removed so a
/// message body can never decide a status again.
fn legacy_process_status(
    total_cpu: f32,
    total_mem_mb: f32,
    busy: bool,
) -> (&'static str, domain::DisplayStatus) {
    if busy {
        ("working", domain::DisplayStatus::Working)
    } else if total_cpu > 80.0 || total_mem_mb > 1024.0 {
        ("high_load", domain::DisplayStatus::Working)
    } else {
        ("idle", domain::DisplayStatus::Idle)
    }
}

fn scan_agents(
    sys: &mut System,
    session: &mut SessionState,
) -> (
    Vec<AgentInfo>,
    interface::snapshot::AgentViewSnapshot,
    interface::diagnostics::DiagnosticSnapshot,
    bool,
) {
    refresh_agent_processes(sys);
    let now = Instant::now();
    let delta = now.duration_since(session.last_poll).as_secs();
    session.last_poll = now;
    let mut agents: Vec<AgentInfo> = Vec::new();
    let mut process_facts = Vec::new();
    let mut session_candidates = Vec::new();
    let mut diagnostic_records = Vec::new();
    let mut acquisition_issues = Vec::new();
    let mut stats_changed = false;

    for def in agent_defs() {
        let agent_name = &def.name;
        let agent_id = if def.log_kind.is_empty() {
            def.keyword.as_str()
        } else {
            def.log_kind.as_str()
        };
        let processes = matching_processes(sys, &def.keyword);
        let observations: Vec<ProcessObservation> = processes
            .iter()
            .map(|process| ProcessObservation {
                pid: process.pid().as_u32(),
                project_path: process
                    .cwd()
                    .map(|path| path.to_string_lossy().into_owned()),
                started_at_ms: process.start_time().saturating_mul(1_000),
            })
            .collect();
        let mut process_fact = process_fact_from_observations(agent_id, agent_name, &observations);
        let process_cwd = process_fact.identity.project_path.clone();
        let mut session_scan =
            build_session_scan_for_process(agent_name, Some(&process_fact.identity));
        if session_scan.acquisition == AcquisitionCompleteness::Incomplete {
            acquisition_issues.push(
                session_scan
                    .diagnostics
                    .first()
                    .map(|record| record.view.clone())
                    .unwrap_or_else(|| {
                        interface::diagnostics::DiagnosticView::new(
                            agent_id,
                            domain::DataIssue::AcquisitionIncomplete,
                            domain::Freshness {
                                observed_at_ms: epoch_millis(),
                                stale: true,
                            },
                            None,
                            0,
                            0,
                        )
                    }),
            );
        }
        let session_list = std::mem::take(&mut session_scan.legacy_sessions);
        session_candidates.append(&mut session_scan.candidates);
        diagnostic_records.append(&mut session_scan.diagnostics);
        if processes.is_empty() {
            process_facts.push(process_fact);
            session.activity.remove(agent_name);
            session.runtime_start.remove(agent_name);
            let first = session_list.first();
            agents.push(AgentInfo {
                id: agent_id.to_string(),
                name: agent_name.clone(),
                status: "stopped".to_string(),
                display_status: domain::DisplayStatus::Stopped,
                pid: None,
                cpu: None,
                memory: None,
                uptime: None,
                cwd: first.and_then(|s| s.cwd.clone()),
                sessions: 0,
                last_active_secs: None,
                log_path: first.and_then(|s| s.log_path.clone()),
                recent_output: first.map(|s| s.recent_output.clone()).unwrap_or_default(),
                current_file: first.and_then(|s| s.current_file.clone()),
                log_status: first.and_then(|s| s.log_status.clone()),
                alert: first.and_then(|s| s.alert.clone()),
                can_restart: session.commands.contains_key(agent_name),
                stats: session.stats.get(agent_name).cloned(),
                session_count: session_list.len(),
                session_list,
                active_session: None,
                history_sessions: None,
                freshness: domain::Freshness {
                    observed_at_ms: epoch_millis(),
                    stale: false,
                },
                usage: usage_for(agent_name),
            });
            continue;
        }

        let main = processes
            .iter()
            .max_by_key(|process| {
                let exe_matches = process
                    .exe()
                    .and_then(|path| path.file_name())
                    .map(|file| file.to_string_lossy().to_lowercase().contains(&def.keyword))
                    .unwrap_or(false);
                (exe_matches, process.cpu_usage() as u32)
            })
            .copied();

        let total_cpu: f32 = processes.iter().map(|p| p.cpu_usage()).sum();
        let total_mem_mb: f32 = processes
            .iter()
            .map(|p| p.memory() as f32 / (1024.0 * 1024.0))
            .sum();
        let uptime = processes.iter().map(|p| p.run_time()).max().unwrap_or(0);
        let log = session_list.first().map(|s| LogSnapshot {
            path: s.log_path.clone().unwrap_or_default(),
            recent: s.recent_output.clone(),
            file: s.current_file.clone(),
            cwd: s.cwd.clone(),
            log_status: s.log_status.clone(),
            alert: s.alert.clone(),
        });
        let cwd = process_cwd;

        let entry = session
            .activity
            .entry(agent_name.clone())
            .or_insert_with(|| ActivityState {
                last_cpu: total_cpu,
                last_active: now,
            });
        let cpu_delta = (total_cpu - entry.last_cpu).abs();
        let busy = total_cpu > 2.0 || cpu_delta > 1.0;
        if busy {
            entry.last_active = now;
        }
        entry.last_cpu = total_cpu;
        process_fact.activity = if busy {
            interface::snapshot::ProcessActivity::Busy {
                observed_at_ms: epoch_millis(),
            }
        } else {
            interface::snapshot::ProcessActivity::Idle {
                observed_at_ms: epoch_millis(),
            }
        };
        let idle_secs = now.duration_since(entry.last_active).as_secs();
        let log_status = log.as_ref().and_then(|l| l.log_status.clone());
        let alert = log.as_ref().and_then(|l| l.alert.clone());
        // Compatibility placeholder only. Structured events are the single source
        // of the authoritative status and `apply_snapshot_projection` overwrites
        // both fields as soon as the agent has a snapshot view. Message text never
        // decides a status here.
        let (status, placeholder_display_status) =
            legacy_process_status(total_cpu, total_mem_mb, busy);

        if let Some(main_proc) = main {
            let cmd_vec: Vec<String> = main_proc
                .cmd()
                .iter()
                .map(|s| s.to_string_lossy().into_owned())
                .collect();
            if !cmd_vec.is_empty() {
                session.commands.insert(
                    agent_name.clone(),
                    AgentCommand {
                        cwd: cwd.clone(),
                        cmd: cmd_vec,
                    },
                );
            }
        }

        let stats = session.stats.entry(agent_name.clone()).or_default();
        let today = today_key();
        let day_stats = session
            .daily
            .entry(today)
            .or_default()
            .entry(agent_name.clone())
            .or_default();
        if session.runtime_start.contains_key(agent_name) {
            stats.total_seconds += delta;
            day_stats.total_seconds += delta;
        } else {
            session.runtime_start.insert(agent_name.clone(), now);
        }
        let stats_snapshot = stats.clone();

        agents.push(AgentInfo {
            id: agent_id.to_string(),
            name: agent_name.clone(),
            status: status.to_string(),
            display_status: placeholder_display_status,
            pid: main.map(|p| p.pid().as_u32()),
            cpu: Some(total_cpu),
            memory: Some(total_mem_mb),
            uptime: Some(uptime),
            cwd,
            sessions: processes.len(),
            last_active_secs: Some(if busy { 0 } else { idle_secs }),
            log_path: log.as_ref().map(|l| l.path.clone()),
            recent_output: log.as_ref().map(|l| l.recent.clone()).unwrap_or_default(),
            current_file: log.as_ref().and_then(|l| l.file.clone()),
            log_status,
            alert,
            can_restart: session.commands.contains_key(agent_name),
            stats: Some(stats_snapshot),
            session_count: session_list.len(),
            session_list,
            active_session: None,
            history_sessions: None,
            freshness: domain::Freshness {
                observed_at_ms: epoch_millis(),
                stale: false,
            },
            usage: usage_for(agent_name),
        });
        process_facts.push(process_fact);
    }
    let generated_at_ms = epoch_millis();
    let mut snapshot = interface::snapshot::build_snapshot_with_acquisition(
        generated_at_ms,
        &process_facts,
        &session_candidates,
        &acquisition_issues,
    );
    attach_runtime_metadata_to_snapshot(&mut snapshot, &agents);
    for agent in &snapshot.agents {
        if let Some(view) = &agent.diagnostic {
            diagnostic_records.push(interface::diagnostics::DiagnosticRecord {
                view: view.clone(),
                session_id: None,
                project_path: process_facts
                    .iter()
                    .find(|fact| fact.identity.agent_id == agent.id)
                    .and_then(|fact| fact.identity.project_path.clone()),
                message_length: 0,
            });
        }
    }
    let diagnostics = interface::diagnostics::DiagnosticSnapshot {
        generated_at_ms,
        records: diagnostic_records,
    };
    apply_snapshot_projection(&mut agents, &snapshot);
    stats_changed |= update_terminal_statistics_from_snapshot(
        &mut session.stats,
        &mut session.daily,
        &today_key(),
        &snapshot,
    );
    for agent in &mut agents {
        agent.stats = session.stats.get(&agent.name).cloned();
    }
    (agents, snapshot, diagnostics, stats_changed)
}

fn attach_runtime_metadata_to_snapshot(
    snapshot: &mut interface::snapshot::AgentViewSnapshot,
    agents: &[AgentInfo],
) {
    for view in &mut snapshot.agents {
        let agent = agents
            .iter()
            .find(|agent| agent.id == view.id)
            .or_else(|| agents.iter().find(|agent| agent.name == view.name));
        view.can_restart = agent.is_some_and(|agent| agent.can_restart);
        view.usage = agent.and_then(|agent| agent.usage.clone());
    }
}

fn refresh_agent_processes(sys: &mut System) {
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, agent_process_refresh_kind());
}

fn agent_process_refresh_kind() -> ProcessRefreshKind {
    ProcessRefreshKind::nothing()
        .with_cmd(UpdateKind::Always)
        .with_cwd(UpdateKind::Always)
        .with_exe(UpdateKind::Always)
        .with_cpu()
        .with_memory()
}

fn legacy_status(display_status: domain::DisplayStatus) -> &'static str {
    match display_status {
        domain::DisplayStatus::Stopped => "stopped",
        domain::DisplayStatus::Idle => "idle",
        domain::DisplayStatus::Working => "working",
        domain::DisplayStatus::Done => "done",
        domain::DisplayStatus::Error => "error",
        domain::DisplayStatus::Waiting => "waiting",
    }
}

fn legacy_session(session: &interface::snapshot::SessionView) -> AgentSession {
    AgentSession {
        id: session.id.clone(),
        name: session.name.clone(),
        cwd: session.cwd.clone(),
        log_path: session.log_path.clone(),
        recent_output: session.recent_output.clone(),
        current_file: session.current_file.clone(),
        log_status: session.log_status.clone(),
        alert: session.alert.clone(),
    }
}

fn apply_snapshot_projection(
    agents: &mut [AgentInfo],
    snapshot: &interface::snapshot::AgentViewSnapshot,
) {
    for agent in agents {
        let Some(view) = snapshot.agents.iter().find(|view| view.id == agent.id) else {
            continue;
        };
        agent.id = view.id.clone();
        agent.status = legacy_status(view.display_status).into();
        agent.display_status = view.display_status;
        agent.active_session = view.active_session.clone();
        agent.history_sessions = Some(view.history_sessions.clone());
        agent.freshness = view.freshness;
        agent.session_list = view
            .active_session
            .as_ref()
            .map(legacy_session)
            .into_iter()
            .collect();
    }
}

fn update_terminal_statistics_from_snapshot(
    stats: &mut HashMap<String, AgentStats>,
    daily: &mut HashMap<String, HashMap<String, AgentStats>>,
    day: &str,
    snapshot: &interface::snapshot::AgentViewSnapshot,
) -> bool {
    let mut changed = false;
    for agent in &snapshot.agents {
        if agent.freshness.stale || agent.state.process != domain::ProcessState::Running {
            continue;
        }
        let (Some(session), Some(turn), Some(result_at_ms)) = (
            agent.active_session.as_ref(),
            agent.active_turn.as_ref(),
            agent.state.result_at_ms,
        ) else {
            continue;
        };
        if turn.agent_id != agent.id || turn.session_id != session.id {
            continue;
        }
        let status = match agent.state.turn {
            domain::TurnState::Succeeded => "done",
            domain::TurnState::Failed => "error",
            _ => continue,
        };
        let transition_key = format!(
            "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{result_at_ms}",
            agent.id, turn.session_id, turn.turn_id, status,
        );
        let transition_hash = terminal_key_hash(&transition_key);
        let agent_stats = stats.entry(agent.name.clone()).or_default();
        if agent_stats
            .recent_terminal_hashes
            .iter()
            .any(|hash| hash == &transition_hash)
        {
            continue;
        }
        let day_stats = daily
            .entry(day.into())
            .or_default()
            .entry(agent.name.clone())
            .or_default();
        match status {
            "done" => {
                agent_stats.done_count += 1;
                day_stats.done_count += 1;
            }
            "error" => {
                agent_stats.error_count += 1;
                day_stats.error_count += 1;
            }
            _ => unreachable!(),
        }
        agent_stats.last_status = Some(status.into());
        remember_terminal_hash(agent_stats, transition_hash);
        changed = true;
    }
    changed
}

const RECENT_TERMINAL_HASH_CAPACITY: usize = 128;

fn terminal_key_hash(transition_key: &str) -> String {
    format!("{:x}", Sha256::digest(transition_key.as_bytes()))
}

fn remember_terminal_hash(stats: &mut AgentStats, transition_hash: String) {
    stats.recent_terminal_hashes.push(transition_hash);
    if stats.recent_terminal_hashes.len() > RECENT_TERMINAL_HASH_CAPACITY {
        stats.recent_terminal_hashes.remove(0);
    }
}

fn normalize_terminal_hashes(hashes: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut normalized = Vec::new();
    for hash in hashes {
        if hash.len() == 64
            && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
            && !normalized.iter().any(|existing| existing == &hash)
        {
            normalized.push(hash);
        }
    }
    if normalized.len() > RECENT_TERMINAL_HASH_CAPACITY {
        normalized.drain(..normalized.len() - RECENT_TERMINAL_HASH_CAPACITY);
    }
    normalized
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
}

fn codex_data_root_from(configured: Option<&std::ffi::OsStr>, home: Option<&Path>) -> Option<PathBuf> {
    configured
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| home.map(|path| path.join(".codex")))
}

fn codex_data_root() -> Option<PathBuf> {
    codex_data_root_from(std::env::var_os("CODEX_HOME").as_deref(), home_dir().as_deref())
}

fn stats_path() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".agent-island")
        .join("stats.json")
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
struct StoredAgentStats {
    total_seconds: u64,
    error_count: u32,
    done_count: u32,
    last_status: Option<String>,
    recent_terminal_hashes: Vec<String>,
    last_transition_key: Option<String>,
}

impl From<StoredAgentStats> for AgentStats {
    fn from(stored: StoredAgentStats) -> Self {
        let legacy_hash = stored
            .last_transition_key
            .map(|key| terminal_key_hash(&key));
        let hashes = stored
            .recent_terminal_hashes
            .into_iter()
            .chain(legacy_hash)
            .collect::<Vec<_>>();
        Self {
            total_seconds: stored.total_seconds,
            error_count: stored.error_count,
            done_count: stored.done_count,
            last_status: stored.last_status,
            recent_terminal_hashes: normalize_terminal_hashes(hashes),
        }
    }
}

fn load_stats(path: &Path) -> HashMap<String, AgentStats> {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<HashMap<String, StoredAgentStats>>(&s).ok())
        .map(|stored| {
            stored
                .into_iter()
                .map(|(agent, stats)| (agent, stats.into()))
                .collect()
        })
        .unwrap_or_default()
}

fn save_stats(stats: &HashMap<String, AgentStats>, path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let data = serde_json::to_string_pretty(stats).map_err(|e| e.to_string())?;
    fs::write(path, data).map_err(|e| e.to_string())
}

fn daily_path() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".agent-island")
        .join("stats-daily.json")
}

fn load_daily(path: &Path) -> HashMap<String, HashMap<String, AgentStats>> {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_daily(
    daily: &HashMap<String, HashMap<String, AgentStats>>,
    path: &Path,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let data = serde_json::to_string_pretty(daily).map_err(|e| e.to_string())?;
    fs::write(path, data).map_err(|e| e.to_string())
}

fn date_key(secs: u64) -> String {
    let days = secs / 86400;
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y } as u32;
    format!("{y:04}-{m:02}-{d:02}")
}

fn today_key() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    date_key(secs)
}

fn read_tail(path: &Path, max_bytes: u64) -> String {
    let Ok(mut file) = File::open(path) else {
        return String::new();
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub(max_bytes);
    let mut buf = Vec::new();
    if file.seek(SeekFrom::Start(start)).is_err() || file.read_to_end(&mut buf).is_err() {
        return String::new();
    }
    String::from_utf8_lossy(&buf).into_owned()
}

#[allow(dead_code)]
fn clean_line(s: &str, max_chars: usize) -> String {
    let s = s.replace(['\r', '\n'], " ");
    let s = s.trim().trim_matches('"');
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max_chars).collect();
        out.push_str("...");
        out
    }
}

#[allow(dead_code)]
fn json_text(v: &Value) -> Option<String> {
    let content = v.pointer("/message/content").or_else(|| v.get("content"))?;
    let arr = content.as_array()?;
    let mut parts = Vec::new();
    for item in arr {
        let kind = item.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if kind.ends_with("text") {
            if let Some(s) = item.get("text").and_then(|t| t.as_str()) {
                if !s.trim().is_empty() {
                    parts.push(s.trim().to_string());
                }
            }
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n"))
    }
}

fn extract_paths(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in text.split(|c: char| {
        c.is_whitespace() || matches!(c, '"' | '\'' | '`' | ',' | '(' | ')' | '[' | ']' | ';')
    }) {
        let token = raw.trim_matches(|c: char| {
            c == '"' || c == '\'' || c == '`' || c == ',' || c == ')' || c == ']'
        });
        if token.is_empty() {
            continue;
        }
        let has_path_sep = token.contains('\\') || (token.contains('/') && token.contains(':'));
        let has_ext = token
            .rsplit('.')
            .nth(1)
            .map(|e| !e.is_empty() && e.len() <= 10)
            .unwrap_or(false);
        if has_path_sep && has_ext {
            out.push(token.to_string());
        }
    }
    out
}

fn list_newest_files(root: &Path, depth: usize, ext: &str, max: usize) -> Vec<PathBuf> {
    let mut found: Vec<(SystemTime, PathBuf)> = Vec::new();
    fn walk(dir: &Path, depth: usize, ext: &str, found: &mut Vec<(SystemTime, PathBuf)>) {
        if depth == 0 {
            return;
        }
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, depth - 1, ext, found);
            } else if path.extension().and_then(|e| e.to_str()) == Some(ext) {
                if let Ok(meta) = entry.metadata() {
                    if let Ok(mtime) = meta.modified() {
                        found.push((mtime, path));
                    }
                }
            }
        }
    }
    walk(root, depth, ext, &mut found);
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.into_iter().take(max).map(|(_, path)| path).collect()
}

fn session_name(cwd: Option<&str>, fallback: &str) -> String {
    if let Some(c) = cwd {
        let parts: Vec<&str> = c.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
        if let Some(last) = parts.last() {
            return last.to_string();
        }
    }
    fallback.chars().take(24).collect()
}

fn to_session_with_id(snap: LogSnapshot, id: String) -> AgentSession {
    let name = session_name(snap.cwd.as_deref(), &id);
    AgentSession {
        id,
        name,
        cwd: snap.cwd,
        log_path: Some(snap.path),
        recent_output: snap.recent,
        current_file: snap.file,
        log_status: snap.log_status,
        alert: snap.alert,
    }
}

fn session_scan_from_file(agent_id: &str, path: &Path) -> Option<SessionScan> {
    session_scan_from_file_with_budget(agent_id, path, 64 * 1024, 512 * 1024)
}

fn session_scan_from_file_with_budget(
    agent_id: &str,
    path: &Path,
    head: u64,
    tail: u64,
) -> Option<SessionScan> {
    let acquired = read_bounded_head_tail(path, head, tail);
    let mut scan = SessionScan::default();
    for issue in acquired.issues {
        scan.record_issue(agent_id, issue);
    }
    let text = acquired.text;
    if text.is_empty() {
        return Some(scan);
    }
    let (report, metadata_id, metadata_cwd) = match agent_id {
        "codex" => {
            let (metadata_id, metadata_cwd) = codex_metadata(&text);
            (
                adapters::codex::CodexAdapter.parse(&text),
                metadata_id,
                metadata_cwd,
            )
        }
        "claude" => (adapters::claude::ClaudeAdapter.parse(&text), None, None),
        "opencode" => (
            adapters::opencode::OpenCodeAdapter.parse(&text),
            Some("opencode".into()),
            None,
        ),
        _ => return None,
    };
    let event_count = report.events.len();
    let message_count = report.messages.len();
    let message_length = report
        .messages
        .iter()
        .map(|message| message.text.len())
        .sum();
    let event_type = report.events.last().map(|event| {
        match event.kind {
            domain::EventKind::TurnStarted => "turn_started",
            domain::EventKind::ToolStarted => "tool_started",
            domain::EventKind::ToolFinished { .. } => "tool_finished",
            domain::EventKind::AttentionRequested { .. } => "attention_requested",
            domain::EventKind::TurnSucceeded => "turn_succeeded",
            domain::EventKind::TurnFailed => "turn_failed",
            domain::EventKind::DiagnosticHint { .. } => "diagnostic_hint",
        }
        .to_string()
    });
    let issue = domain::parse_issue(
        event_count > 0 || message_count > 0 || !report.sessions.is_empty(),
        report.skipped_lines,
    );
    let fallback_id = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let canonical_id = metadata_id
        .or_else(|| report.events.last().map(|event| event.session_id.clone()))
        .unwrap_or(fallback_id);
    let events: Vec<domain::DomainEvent> = report
        .events
        .into_iter()
        .filter(|event| event.session_id == canonical_id)
        .collect();
    let records = chronological_records(report.messages);
    let snapshot = snapshot_from_bounded_text(path, &text, metadata_cwd, &records);
    let legacy = to_session_with_id(snapshot, canonical_id.clone());
    let candidate =
        session_candidate_from_legacy(agent_id, &legacy, events, records, acquired.modified_at_ms);
    if issue.is_some() {
        scan.acquisition = AcquisitionCompleteness::Incomplete;
    }
    let diagnostics: Vec<_> = issue
        .map(|issue| interface::diagnostics::DiagnosticRecord {
            view: interface::diagnostics::DiagnosticView::new(
                agent_id,
                issue,
                domain::Freshness {
                    observed_at_ms: candidate.identity.last_event_at_ms,
                    stale: true,
                },
                event_type,
                event_count,
                message_count,
            ),
            session_id: Some(canonical_id),
            project_path: candidate.identity.project_path.clone(),
            message_length,
        })
        .into_iter()
        .collect();
    scan.legacy_sessions.push(legacy);
    scan.candidates.push(candidate);
    scan.diagnostics.extend(diagnostics);
    Some(scan)
}

const MAX_DISPLAY_RECORDS: usize = 24;
const MAX_DISPLAY_RECORD_TEXT_CHARS: usize = 600;
const MAX_RECENT_OUTPUT_RECORDS: usize = 5;
const MAX_RECENT_OUTPUT_TEXT_CHARS: usize = 180;

fn chronological_records(
    mut messages: Vec<domain::ConversationMessage>,
) -> Vec<domain::ConversationMessage> {
    messages.sort_by_key(|message| message.at_ms);
    let first_record = messages.len().saturating_sub(MAX_DISPLAY_RECORDS);
    messages.drain(..first_record);
    for message in &mut messages {
        message.text = truncate_display_text(&message.text, MAX_DISPLAY_RECORD_TEXT_CHARS);
    }
    messages
}

fn truncate_display_text(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

fn compatibility_output(records: &[domain::ConversationMessage]) -> Vec<String> {
    records
        .iter()
        .rev()
        .take(MAX_RECENT_OUTPUT_RECORDS)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|record| truncate_display_text(&record.text, MAX_RECENT_OUTPUT_TEXT_CHARS))
        .collect()
}

fn snapshot_from_bounded_text(
    path: &Path,
    text: &str,
    metadata_cwd: Option<String>,
    records: &[domain::ConversationMessage],
) -> LogSnapshot {
    let cwd = metadata_cwd.or_else(|| {
        text.lines().find_map(|line| {
            serde_json::from_str::<Value>(line).ok().and_then(|value| {
                value
                    .get("cwd")
                    .and_then(Value::as_str)
                    .or_else(|| value.pointer("/payload/cwd").and_then(Value::as_str))
                    .map(str::to_owned)
            })
        })
    });
    let file = records
        .iter()
        .rev()
        .find_map(|record| extract_paths(&record.text).into_iter().last());
    LogSnapshot {
        path: path.display().to_string(),
        recent: compatibility_output(records),
        file,
        cwd,
        log_status: None,
        alert: None,
    }
}

fn codex_metadata(text: &str) -> (Option<String>, Option<String>) {
    for line in text.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if value.get("type").and_then(Value::as_str) != Some("session_meta") {
            continue;
        }
        let id = value
            .pointer("/payload/id")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let cwd = value
            .pointer("/payload/cwd")
            .and_then(Value::as_str)
            .map(str::to_owned);
        return (id, cwd);
    }
    (None, None)
}

fn is_codex_desktop_root(text: &str) -> bool {
    text.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()).find(|value| {
        value.get("type").and_then(Value::as_str) == Some("session_meta")
    }).is_some_and(|value| {
        let payload = &value["payload"];
        matches!(payload["originator"].as_str(), Some("Codex Desktop" | "codex_work_desktop"))
            && payload["thread_source"].as_str() == Some("user")
            && payload.get("parent_thread_id").is_none_or(Value::is_null)
            && payload["source"].as_str() == Some("vscode")
    })
}

fn session_candidate_from_legacy(
    agent_id: &str,
    session: &AgentSession,
    events: Vec<domain::DomainEvent>,
    records: Vec<domain::ConversationMessage>,
    file_time_ms: u64,
) -> interface::snapshot::SessionCandidate {
    let session_events: Vec<&domain::DomainEvent> = events
        .iter()
        .filter(|event| event.session_id == session.id)
        .collect();
    let last_event_at_ms = session_events
        .iter()
        .map(|event| event.at_ms)
        .max()
        .unwrap_or(file_time_ms);
    let started_at_ms = session_events
        .iter()
        .map(|event| event.at_ms)
        .min()
        .unwrap_or(last_event_at_ms);
    let source = session_events
        .last()
        .map(|event| event.source)
        .unwrap_or(domain::EventSource::Process);
    let confidence = session_events
        .last()
        .map(|event| event.confidence)
        .unwrap_or(domain::Confidence::Unknown);

    interface::snapshot::SessionCandidate {
        identity: domain::SessionIdentity {
            agent_id: agent_id.into(),
            session_id: session.id.clone(),
            project_path: session.cwd.clone(),
            process_ids: Vec::new(),
            started_at_ms,
            last_event_at_ms,
            source,
            confidence,
            lifecycle: domain::SessionLifecycle::Historical,
        },
        view: interface::snapshot::SessionView {
            id: session.id.clone(),
            name: session.name.clone(),
            cwd: session.cwd.clone(),
            log_path: session.log_path.clone(),
            records,
            recent_output: session.recent_output.clone(),
            current_file: session.current_file.clone(),
            log_status: session.log_status.clone(),
            alert: session.alert.clone(),
            lifecycle: domain::SessionLifecycle::Historical,
            last_active_at_ms: last_event_at_ms,
            display_status: domain::DisplayStatus::Idle,
        },
        events,
    }
}

fn scan_session_directory(
    agent_id: &str,
    root: &Path,
    process: Option<&domain::ProcessIdentity>,
) -> SessionScan {
    let mut scan = SessionScan::default();
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                scan.record_issue(agent_id, io_issue(&error));
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    scan.record_issue(agent_id, io_issue(&error));
                    continue;
                }
            };
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(error) => {
                    scan.record_issue(agent_id, io_issue(&error));
                    continue;
                }
            };
            if metadata.is_symlink() {
                scan.record_issue(agent_id, domain::DataIssue::AcquisitionIncomplete);
            } else if metadata.is_dir() {
                pending.push(entry.path());
            } else if entry.path().extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
                match metadata.modified() {
                    Ok(modified) => files.push((modified, entry.path())),
                    Err(error) => scan.record_issue(agent_id, io_issue(&error)),
                }
            }
        }
    }
    files.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    // Every file contributes identity evidence. Only selected files get expensive display parsing.
    for (_, path) in &files {
        let mut identity_scan = index_session_file(agent_id, path);
        scan.desktop_root_paths.append(&mut identity_scan.desktop_root_paths);
        scan.candidates.append(&mut identity_scan.candidates);
        scan.diagnostics.append(&mut identity_scan.diagnostics);
        if identity_scan.acquisition == AcquisitionCompleteness::Incomplete {
            scan.acquisition = AcquisitionCompleteness::Incomplete;
        }
    }
    // This changes display priority only: all identities remain available to matching.
    // Stable ordering preserves newest-first order within each group.
    scan.candidates.sort_by_key(|candidate| {
        !candidate.view.log_path.as_ref().is_some_and(|path| scan.desktop_root_paths.contains(path))
    });
    let active_id = indexed_active_session(&scan, process, epoch_millis());
    let active = scan
        .candidates
        .iter()
        .find(|candidate| Some(candidate.identity.session_id.as_str()) == active_id.as_deref());
    let history = scan
        .candidates
        .iter()
        .filter(|candidate| Some(candidate.identity.session_id.as_str()) != active_id.as_deref())
        .take(3);
    let detail_paths = active
        .into_iter()
        .chain(history)
        .filter_map(|candidate| candidate.view.log_path.clone())
        .collect::<Vec<_>>();
    for path in detail_paths {
        load_session_detail(&mut scan, agent_id, Path::new(&path));
    }
    if scan.acquisition == AcquisitionCompleteness::Complete
        && indexed_active_session(&scan, process, epoch_millis()) != active_id
    {
        // A changed selection would mix indexed and detailed evidence and could expose an
        // identity-only active row. Preserve the detail as history and gate this acquisition.
        scan.record_issue(agent_id, domain::DataIssue::AcquisitionIncomplete);
    }
    scan
}

fn indexed_active_session(
    scan: &SessionScan,
    process: Option<&domain::ProcessIdentity>,
    now_ms: u64,
) -> Option<String> {
    if scan.acquisition == AcquisitionCompleteness::Incomplete {
        return None;
    }
    let identities = scan
        .candidates
        .iter()
        .map(|candidate| candidate.identity.clone())
        .collect::<Vec<_>>();
    process.and_then(|process| {
        match application::session_registry::match_active_session(process, &identities, now_ms) {
            application::session_registry::SessionMatch::Confirmed(id)
            | application::session_registry::SessionMatch::Probable(id) => Some(id),
            _ => None,
        }
    })
}

fn load_session_detail(scan: &mut SessionScan, agent_id: &str, path: &Path) {
    if let Some(mut detail) = session_scan_from_file(agent_id, path) {
        if detail.acquisition == AcquisitionCompleteness::Incomplete {
            scan.acquisition = AcquisitionCompleteness::Incomplete;
        }
        scan.diagnostics.append(&mut detail.diagnostics);
        let indexed = scan.candidates.iter().filter(|candidate| {
            candidate.view.log_path.as_deref() == Some(path.to_string_lossy().as_ref())
        });
        let identity_disappeared = indexed.clone().any(|expected| {
            !detail.candidates.iter().any(|candidate| {
                candidate.view.log_path == expected.view.log_path
                    && candidate.identity.session_id == expected.identity.session_id
                    && candidate.identity.project_path == expected.identity.project_path
            })
        });
        let identity_replaced = detail.candidates.iter().any(|candidate| {
            !indexed.clone().any(|expected| {
                candidate.view.log_path == expected.view.log_path
                    && candidate.identity.session_id == expected.identity.session_id
                    && candidate.identity.project_path == expected.identity.project_path
            })
        });
        if identity_disappeared || identity_replaced {
            scan.record_issue(agent_id, domain::DataIssue::AcquisitionIncomplete);
            return;
        }
        for candidate in detail.candidates {
            if let Some(existing) = scan
                .candidates
                .iter_mut()
                .find(|existing| existing.view.log_path == candidate.view.log_path)
            {
                *existing = candidate;
            }
        }
        scan.legacy_sessions.append(&mut detail.legacy_sessions);
    }
}

fn index_session_file(agent_id: &str, path: &Path) -> SessionScan {
    let mut scan = SessionScan::default();
    let mut acquired = AcquiredText::default();
    let result = (|| -> std::io::Result<()> {
        let mut file = File::open(path)?;
        let metadata = file.metadata()?;
        acquired.modified_at_ms = metadata
            .modified()?
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0);
        let mut head = Vec::new();
        (&mut file).take(16 * 1024).read_to_end(&mut head)?;
        // Codex embeds instructions in session_meta: its first JSON record can
        // exceed 16 KiB. Finish that record before trimming partial tail data,
        // otherwise one ordinary long header invalidates the entire scan.
        // Keep a hard bound for malformed files without any record delimiter.
        while !head.contains(&b'\n')
            && (head.len() as u64) < metadata.len()
            && head.len() < 1024 * 1024
        {
            let previous_len = head.len();
            (&mut file).take(16 * 1024).read_to_end(&mut head)?;
            if head.len() == previous_len {
                break;
            }
        }
        if metadata.len() > head.len() as u64 {
            head.truncate(
                head.iter()
                    .rposition(|byte| *byte == b'\n')
                    .map(|index| index + 1)
                    .unwrap_or(0),
            );
        }
        acquired.text = String::from_utf8_lossy(&head).into_owned();
        Ok(())
    })();
    if let Err(error) = result {
        scan.record_issue(agent_id, io_issue(&error));
        return scan;
    }
    if agent_id == "codex" && is_codex_desktop_root(&acquired.text) {
        scan.desktop_root_paths.push(path.to_string_lossy().into_owned());
    }
    let (id, cwd) = if agent_id == "codex" {
        codex_metadata(&acquired.text)
    } else {
        acquired
            .text
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .find_map(|value| {
                let id = value
                    .get("sessionId")
                    .or_else(|| value.get("session_id"))?
                    .as_str()?
                    .to_owned();
                let cwd = value.get("cwd").and_then(Value::as_str).map(str::to_owned);
                Some((Some(id), cwd))
            })
            .unwrap_or_default()
    };
    if id.is_none() || cwd.is_none() {
        scan.record_issue(agent_id, domain::DataIssue::AcquisitionIncomplete);
    }
    let id = id.unwrap_or_else(|| path.to_string_lossy().into_owned());
    let legacy = AgentSession {
        id: id.clone(),
        name: session_name(cwd.as_deref(), &id),
        cwd,
        log_path: Some(path.to_string_lossy().into_owned()),
        recent_output: Vec::new(),
        current_file: None,
        log_status: None,
        alert: None,
    };
    scan.candidates.push(session_candidate_from_legacy(
        agent_id,
        &legacy,
        Vec::new(),
        Vec::new(),
        acquired.modified_at_ms,
    ));
    scan
}

fn opencode_session_scan() -> SessionScan {
    let Some(path) = home_dir().map(|home| {
        home.join(".local")
            .join("share")
            .join("opencode")
            .join("log")
            .join("opencode.log")
    }) else {
        return SessionScan::default();
    };
    session_scan_from_file("opencode", &path).unwrap_or_default()
}

#[cfg(test)]
fn combine_session_scans(scans: impl Iterator<Item = SessionScan>) -> SessionScan {
    let mut combined = SessionScan::default();
    for mut scan in scans {
        if scan.acquisition == AcquisitionCompleteness::Incomplete {
            combined.acquisition = AcquisitionCompleteness::Incomplete;
        }
        combined.legacy_sessions.append(&mut scan.legacy_sessions);
        combined.candidates.append(&mut scan.candidates);
        combined.diagnostics.append(&mut scan.diagnostics);
    }
    combined
}

static HERMES_CACHE: OnceLock<Mutex<Option<(Instant, SessionScan)>>> = OnceLock::new();

fn run_command_timeout(
    command: &mut std::process::Command,
    secs: u64,
) -> Result<(bool, String), domain::DataIssue> {
    static COMMAND_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let token = format!(
        "agent-island-{}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0),
        COMMAND_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    );
    let out_path = std::env::temp_dir().join(format!("{token}.out"));
    let err_path = std::env::temp_dir().join(format!("{token}.err"));
    let result = (|| {
        let out_file = File::create(&out_path).map_err(|error| io_issue(&error))?;
        let err_file = File::create(&err_path).map_err(|error| io_issue(&error))?;
        let mut child = command
            .stdout(Stdio::from(out_file))
            .stderr(Stdio::from(err_file))
            .spawn()
            .map_err(|error| io_issue(&error))?;
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if started.elapsed().as_secs() < secs => {
                    std::thread::sleep(std::time::Duration::from_millis(10))
                }
                outcome => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(outcome
                        .err()
                        .map(|error| io_issue(&error))
                        .unwrap_or(domain::DataIssue::LogUnavailable));
                }
            }
        };
        let acquired = read_bounded_head_tail(&out_path, 512 * 1024, 512 * 1024);
        if let Some(issue) = acquired.issues.into_iter().next() {
            return Err(issue);
        }
        Ok((status.success(), acquired.text))
    })();
    // Release redirection handles even when spawning fails before removing temporary files.
    command.stdout(Stdio::null()).stderr(Stdio::null());
    let _ = fs::remove_file(&out_path);
    let _ = fs::remove_file(&err_path);
    result
}

fn hermes_session_scan_from_text(text: &str) -> SessionScan {
    let report = adapters::hermes::HermesAdapter.parse(text);
    let session_count = report.sessions.len();
    let issue = domain::parse_issue(
        !report.events.is_empty() || !report.messages.is_empty() || session_count > 0,
        report.skipped_lines,
    );
    let mut scan = SessionScan::default();
    if issue.is_some() {
        scan.acquisition = AcquisitionCompleteness::Incomplete;
    }
    for identity in report.sessions {
        let name = session_name(identity.project_path.as_deref(), &identity.session_id);
        let lifecycle = identity.lifecycle;
        let last_active_at_ms = identity.last_event_at_ms;
        let legacy = AgentSession {
            id: identity.session_id.clone(),
            name,
            cwd: identity.project_path.clone(),
            log_path: None,
            recent_output: Vec::new(),
            current_file: None,
            log_status: None,
            alert: None,
        };
        scan.candidates.push(interface::snapshot::SessionCandidate {
            identity,
            view: interface::snapshot::SessionView {
                id: legacy.id.clone(),
                name: legacy.name.clone(),
                cwd: legacy.cwd.clone(),
                log_path: None,
                records: Vec::new(),
                recent_output: Vec::new(),
                current_file: None,
                log_status: None,
                alert: None,
                lifecycle,
                last_active_at_ms,
                display_status: domain::DisplayStatus::Idle,
            },
            events: Vec::new(),
        });
        scan.legacy_sessions.push(legacy);
    }
    if let Some(issue) = issue {
        scan.diagnostics
            .push(interface::diagnostics::DiagnosticRecord {
                view: interface::diagnostics::DiagnosticView::new(
                    "hermes",
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
    scan
}

fn parse_hermes_session_scan() -> SessionScan {
    let mut command = quiet_command("hermes");
    command.args(["sessions", "list"]);
    hermes_scan_from_command(&mut command, 8)
}

fn hermes_scan_from_command(command: &mut std::process::Command, timeout_secs: u64) -> SessionScan {
    let (success, text) = match run_command_timeout(command, timeout_secs) {
        Ok(output) => output,
        Err(issue) => {
            let mut scan = SessionScan::default();
            scan.record_issue("hermes", issue);
            return scan;
        }
    };
    if !success {
        let mut scan = SessionScan::default();
        scan.record_issue("hermes", domain::DataIssue::LogUnavailable);
        return scan;
    }
    hermes_session_scan_from_text(&text)
}

fn hermes_session_scan() -> SessionScan {
    let cache = HERMES_CACHE.get_or_init(|| Mutex::new(None));
    if let Ok(guard) = cache.lock() {
        if let Some((at, scan)) = guard.as_ref() {
            if at.elapsed().as_secs() < 60 {
                return scan.clone();
            }
        }
    }
    let scan = parse_hermes_session_scan();
    if let Ok(mut guard) = cache.lock() {
        *guard = Some((Instant::now(), scan.clone()));
    }
    scan
}

fn build_session_scan(name: &str) -> SessionScan {
    build_session_scan_for_process(name, None)
}

fn build_session_scan_for_process(
    name: &str,
    process: Option<&domain::ProcessIdentity>,
) -> SessionScan {
    let defs = agent_defs();
    let kind = defs
        .iter()
        .find(|d| d.name == name)
        .map(|d| d.log_kind.as_str())
        .unwrap_or("");
    match kind {
        "claude" | "codex" => {
            let root = if kind == "claude" {
                home_dir().map(|home| home.join(".claude/projects"))
            } else {
                codex_data_root().map(|root| root.join("sessions"))
            };
            let Some(root) = root else {
                let mut scan = SessionScan::default();
                scan.record_issue(kind, domain::DataIssue::LogUnavailable);
                return scan;
            };
            scan_session_directory(kind, &root, process)
        }
        "opencode" => opencode_session_scan(),
        "hermes" => hermes_session_scan(),
        _ => SessionScan::default(),
    }
}

fn build_sessions(name: &str) -> Vec<AgentSession> {
    build_session_scan(name).legacy_sessions
}

fn epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Default)]
struct AcquiredText {
    text: String,
    modified_at_ms: u64,
    issues: Vec<domain::DataIssue>,
}

fn read_bounded_head_tail(path: &Path, head_bytes: u64, tail_bytes: u64) -> AcquiredText {
    let mut acquired = AcquiredText::default();
    let result = (|| -> std::io::Result<()> {
        let mut file = File::open(path)?;
        let metadata = file.metadata()?;
        acquired.modified_at_ms = metadata
            .modified()?
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0);
        let len = metadata.len();
        read_bounded_stream(&mut file, len, head_bytes, tail_bytes, &mut acquired);
        Ok(())
    })();
    if let Err(error) = result {
        acquired.issues.push(io_issue(&error));
    }
    acquired
}

fn read_bounded_stream(
    reader: &mut (impl Read + Seek),
    len: u64,
    head_bytes: u64,
    tail_bytes: u64,
    acquired: &mut AcquiredText,
) {
    let mut bytes = Vec::new();
    let bounded = len > head_bytes.saturating_add(tail_bytes);
    let head_len = if bounded { head_bytes } else { len };
    if let Err(error) = reader.take(head_len).read_to_end(&mut bytes) {
        acquired.issues.push(io_issue(&error));
    } else if bytes.len() as u64 != head_len {
        acquired
            .issues
            .push(domain::DataIssue::AcquisitionIncomplete);
    }
    if bounded {
        // A splice drops unknown state boundaries; retain detail, but do not certify it as current.
        acquired
            .issues
            .push(domain::DataIssue::AcquisitionIncomplete);
        bytes.truncate(
            bytes
                .iter()
                .rposition(|byte| *byte == b'\n')
                .map(|index| index + 1)
                .unwrap_or(0),
        );
        let tail_result = reader.seek(SeekFrom::Start(len - tail_bytes));
        if let Err(error) = tail_result {
            acquired.issues.push(io_issue(&error));
        } else {
            let mut tail = Vec::new();
            if let Err(error) = reader.take(tail_bytes).read_to_end(&mut tail) {
                acquired.issues.push(io_issue(&error));
            } else if tail.len() as u64 != tail_bytes {
                acquired
                    .issues
                    .push(domain::DataIssue::AcquisitionIncomplete);
            }
            if let Some(first_newline) = tail.iter().position(|byte| *byte == b'\n') {
                bytes.extend_from_slice(&tail[first_newline + 1..]);
            }
        }
    }
    if std::str::from_utf8(&bytes).is_err() {
        acquired
            .issues
            .push(domain::DataIssue::AcquisitionIncomplete);
    }
    acquired.text = String::from_utf8_lossy(&bytes).into_owned();
}

fn epoch_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = m as i64 + if m > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn parse_rfc3339_secs(s: &str) -> Option<u64> {
    let (date, rest) = s.split_once('T')?;
    let mut dp = date.split('-');
    let year: i64 = dp.next()?.parse().ok()?;
    let month: i64 = dp.next()?.parse().ok()?;
    let day: i64 = dp.next()?.parse().ok()?;
    let time = rest
        .strip_suffix('Z')
        .unwrap_or_else(|| rest.split(['+', '-']).next().unwrap_or(rest));
    let mut tp = time.split(':');
    let hour: i64 = tp.next()?.parse().ok()?;
    let minute: i64 = tp.next()?.parse().ok()?;
    let second: i64 = tp.next()?.split('.').next()?.parse().ok()?;
    let days = days_from_civil(year, month as u32, day as u32);
    Some((days * 86400 + hour * 3600 + minute * 60 + second) as u64)
}

struct ClaudePrice {
    input: f64,
    output: f64,
    cache_read: f64,
    cache_write: f64,
}

fn claude_price(model: &str) -> Option<ClaudePrice> {
    let m = model.to_lowercase();
    let p = if m.contains("opus") {
        ClaudePrice {
            input: 15.0,
            output: 75.0,
            cache_read: 1.5,
            cache_write: 18.75,
        }
    } else if m.contains("sonnet") {
        ClaudePrice {
            input: 3.0,
            output: 15.0,
            cache_read: 0.3,
            cache_write: 3.75,
        }
    } else if m.contains("haiku") {
        ClaudePrice {
            input: 1.0,
            output: 5.0,
            cache_read: 0.1,
            cache_write: 1.25,
        }
    } else {
        return None;
    };
    Some(p)
}

struct ClaudeScan {
    found: bool,
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
    cost: f64,
    cost_known: bool,
}

fn scan_claude_text(text: &str, window_start: u64) -> ClaudeScan {
    let mut scan = ClaudeScan {
        found: false,
        input: 0,
        output: 0,
        cache_read: 0,
        cache_write: 0,
        cost: 0.0,
        cost_known: false,
    };
    for line in text.lines().rev() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some(ts) = v
            .get("timestamp")
            .and_then(|t| t.as_str())
            .and_then(parse_rfc3339_secs)
        {
            if ts < window_start {
                break;
            }
        }
        let Some(msg) = v.get("message") else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) != Some("assistant") {
            continue;
        }
        let Some(usage) = msg.get("usage") else {
            continue;
        };
        let input = usage
            .get("input_tokens")
            .and_then(|x| x.as_u64())
            .unwrap_or(0);
        let output = usage
            .get("output_tokens")
            .and_then(|x| x.as_u64())
            .unwrap_or(0);
        let cache_read = usage
            .get("cache_read_input_tokens")
            .and_then(|x| x.as_u64())
            .unwrap_or(0);
        let cache_write = usage
            .get("cache_creation_input_tokens")
            .and_then(|x| x.as_u64())
            .unwrap_or(0);
        scan.found = true;
        scan.input += input;
        scan.output += output;
        scan.cache_read += cache_read;
        scan.cache_write += cache_write;
        if let Some(model) = msg.get("model").and_then(|m| m.as_str()) {
            if let Some(p) = claude_price(model) {
                scan.cost += input as f64 / 1e6 * p.input
                    + output as f64 / 1e6 * p.output
                    + cache_read as f64 / 1e6 * p.cache_read
                    + cache_write as f64 / 1e6 * p.cache_write;
                scan.cost_known = true;
            }
        }
    }
    scan
}

const CLAUDE_WINDOW_SECS: u64 = 5 * 3600;

fn claude_usage() -> Option<UsageInfo> {
    let root = home_dir()?.join(".claude").join("projects");
    let files = list_newest_files(&root, 4, "jsonl", 8);
    if files.is_empty() {
        return None;
    }
    let window_start = epoch_secs().saturating_sub(CLAUDE_WINDOW_SECS);
    let mut total = ClaudeScan {
        found: false,
        input: 0,
        output: 0,
        cache_read: 0,
        cache_write: 0,
        cost: 0.0,
        cost_known: false,
    };
    for path in &files {
        let text = read_tail(path, 512 * 1024);
        let scan = scan_claude_text(&text, window_start);
        total.found |= scan.found;
        total.input += scan.input;
        total.output += scan.output;
        total.cache_read += scan.cache_read;
        total.cache_write += scan.cache_write;
        total.cost += scan.cost;
        total.cost_known |= scan.cost_known;
    }
    Some(UsageInfo {
        tokens_total: total.input + total.output + total.cache_read + total.cache_write,
        tokens_output: total.output,
        cost_usd: if total.cost_known {
            Some((total.cost * 100.0).round() / 100.0)
        } else {
            None
        },
        used_percent: None,
        window_secs: CLAUDE_WINDOW_SECS,
        resets_at_secs: None,
        credits: None,
        unlimited: None,
        stale: !total.found,
    })
}

fn scan_codex_text(text: &str, now: u64) -> Option<UsageInfo> {
    let mut best: Option<(u64, UsageInfo)> = None;
    for line in text.lines().rev() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) != Some("event_msg") {
            continue;
        }
        let Some(payload) = v.get("payload") else {
            continue;
        };
        if payload.get("type").and_then(|t| t.as_str()) != Some("token_count") {
            continue;
        }
        let Some(info) = payload.get("info") else {
            continue;
        };
        let tokens_total = info
            .pointer("/total_token_usage/total_tokens")
            .and_then(|x| x.as_u64())
            .unwrap_or(0);
        let tokens_output = info
            .pointer("/total_token_usage/output_tokens")
            .and_then(|x| x.as_u64())
            .unwrap_or(0);
        let rl = payload.get("rate_limits");
        let primary = rl.and_then(|r| r.get("primary"));
        let used_percent = primary
            .and_then(|p| p.get("used_percent"))
            .and_then(|x| x.as_f64())
            .map(|x| x as f32);
        let resets_at = primary
            .and_then(|p| p.get("resets_at"))
            .and_then(|x| x.as_u64());
        let window_minutes = primary
            .and_then(|p| p.get("window_minutes"))
            .and_then(|x| x.as_u64())
            .unwrap_or(0);
        let credits = rl.and_then(|r| r.get("credits"));
        let has_credits = credits
            .and_then(|c| c.get("has_credits"))
            .and_then(|x| x.as_bool())
            .unwrap_or(false);
        let unlimited = credits
            .and_then(|c| c.get("unlimited"))
            .and_then(|x| x.as_bool())
            .unwrap_or(false);
        let balance = credits
            .and_then(|c| c.get("balance"))
            .and_then(|x| x.as_f64());
        let mut info_out = UsageInfo {
            tokens_total,
            tokens_output,
            cost_usd: None,
            used_percent,
            window_secs: window_minutes * 60,
            resets_at_secs: resets_at,
            credits: if has_credits { balance } else { None },
            unlimited: Some(unlimited),
            stale: false,
        };
        if resets_at.map(|r| r <= now).unwrap_or(false) {
            info_out.used_percent = Some(0.0);
            info_out.stale = true;
        }
        let key = resets_at.unwrap_or(0);
        if best.as_ref().map(|(k, _)| key > *k).unwrap_or(true) {
            best = Some((key, info_out));
        }
        break;
    }
    best.map(|(_, info)| info)
}

fn codex_usage() -> Option<UsageInfo> {
    let root = codex_data_root()?.join("sessions");
    let files = list_newest_files(&root, 5, "jsonl", 6);
    if files.is_empty() {
        return None;
    }
    let now = epoch_secs();
    let mut best: Option<(u64, UsageInfo)> = None;
    for path in &files {
        let text = read_tail(path, 256 * 1024);
        if let Some(info) = scan_codex_text(&text, now) {
            let key = info.resets_at_secs.unwrap_or(0);
            if best.as_ref().map(|(k, _)| key > *k).unwrap_or(true) {
                best = Some((key, info));
            }
        }
    }
    best.map(|(_, info)| info)
}

static USAGE_CACHE: OnceLock<Mutex<HashMap<String, (Instant, Option<UsageInfo>)>>> =
    OnceLock::new();

fn usage_for(name: &str) -> Option<UsageInfo> {
    let cache = USAGE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(guard) = cache.lock() {
        if let Some((at, value)) = guard.get(name) {
            if at.elapsed().as_secs() < 30 {
                return value.clone();
            }
        }
    }
    let value = match name {
        "Claude Code" => claude_usage(),
        "Codex CLI" => codex_usage(),
        _ => None,
    };
    if let Ok(mut guard) = cache.lock() {
        guard.insert(name.to_string(), (Instant::now(), value.clone()));
    }
    value
}

const HOOK_PORT: u16 = 8799;

fn hook_dir() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".agent-island")
}

fn hook_config_path() -> PathBuf {
    hook_dir().join("hook-config.json")
}

fn hook_script_path() -> PathBuf {
    hook_dir().join("hook-bridge.mjs")
}

const HOOK_BRIDGE_SCRIPT: &str = r#"#!/usr/bin/env node
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

fn classify_command(command: &str) -> bool {
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

fn merge_hooks_settings(path: &Path, script_path: &Path) -> Result<(), String> {
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

fn remove_hooks_settings(path: &Path) -> Result<(), String> {
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

fn claude_settings_path() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".claude").join("settings.json"))
}

fn install_hooks(state: &AppState) -> Result<(), String> {
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

    *state.hook_token.lock().unwrap() = token;
    state.hook_enabled.store(true, Ordering::SeqCst);
    Ok(())
}

fn uninstall_hooks(state: &AppState) -> Result<(), String> {
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
struct HookApprovalPayload {
    id: String,
    session: String,
    tool: String,
    command: String,
    cwd: String,
}

fn new_hook_approval_id() -> String {
    format!(
        "a{:x}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    )
}

fn read_http_request(stream: &mut std::net::TcpStream) -> Option<(String, String, String)> {
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
            let token = headers
                .lines()
                .find_map(|l| {
                    l.strip_prefix("x-agent-island-token:")
                        .map(|v| v.trim().to_string())
                })
                .unwrap_or_default();
            let content_length: usize = headers
                .lines()
                .find_map(|l| {
                    l.strip_prefix("content-length:")
                        .map(|v| v.trim().parse::<usize>().ok())
                })
                .flatten()
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

fn write_http_json(stream: &mut std::net::TcpStream, status: &str, body: &str) {
    use std::io::Write;
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.flush();
}

fn handle_hook_conn(mut stream: std::net::TcpStream, app: tauri::AppHandle) {
    let Some((uri, token, body)) = read_http_request(&mut stream) else {
        return;
    };
    let state = app.state::<AppState>();
    let valid = !token.is_empty() && *state.hook_token.lock().unwrap() == token;
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
                let mut approvals = state.approvals.lock().unwrap();
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

fn start_hook_server(app: tauri::AppHandle) {
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
fn get_hook_status(state: tauri::State<AppState>) -> bool {
    state.hook_enabled.load(Ordering::SeqCst)
}

#[tauri::command]
fn set_hook_enabled(enabled: bool, state: tauri::State<AppState>) -> Result<String, String> {
    if enabled {
        install_hooks(&state)?;
        Ok("已接入 Claude hooks".to_string())
    } else {
        uninstall_hooks(&state)?;
        Ok("已断开 Claude hooks".to_string())
    }
}

#[tauri::command]
fn respond_hook_approval(
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

fn set_hook_approval_decision(
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
fn dismiss_hook_approval(id: String, state: tauri::State<AppState>) -> Result<(), String> {
    let mut approvals = state
        .approvals
        .lock()
        .map_err(|_| "state lock error".to_string())?;
    if let Some(approval) = approvals.get_mut(&id) {
        approval.decision = Some("ask".to_string());
    }
    Ok(())
}

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
fn focus_agent_terminal(name: String, state: tauri::State<AppState>) -> Result<(), String> {
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

fn collect_cached_with<Clock, Scan>(
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

fn collect_agent_data(
    state: &AppState,
) -> (Vec<AgentInfo>, interface::snapshot::AgentViewSnapshot) {
    let mut sys = state.sys.lock().unwrap();
    let mut session = state.session.lock().unwrap();
    let mut cache = session.cache.take();
    let (agents, snapshot, _diagnostics, scan_result) =
        collect_cached_with(&mut cache, Instant::now, || {
            scan_agents(&mut sys, &mut session)
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
fn get_agents(state: tauri::State<AppState>) -> Vec<AgentInfo> {
    collect_agent_data(&state).0
}

#[tauri::command]
fn get_agent_snapshot(state: tauri::State<AppState>) -> interface::snapshot::AgentViewSnapshot {
    collect_agent_data(&state).1
}

fn diagnostics_from_cache(cache: &AgentDataCache) -> interface::diagnostics::DiagnosticsResponse {
    cache
        .as_ref()
        .map(|(_, _, _, diagnostics)| diagnostics.into())
        .unwrap_or_else(|| interface::diagnostics::DiagnosticsResponse {
            generated_at_ms: 0,
            issues: Vec::new(),
        })
}

fn export_diagnostics_snapshot(
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
fn get_diagnostics(state: tauri::State<AppState>) -> interface::diagnostics::DiagnosticsResponse {
    let session = state.session.lock().unwrap();
    diagnostics_from_cache(&session.cache)
}

#[tauri::command]
fn export_diagnostics(destination: String, state: tauri::State<AppState>) -> Result<(), String> {
    let snapshot = {
        let session = state.session.lock().unwrap();
        session
            .cache
            .as_ref()
            .map(|(_, _, _, diagnostics)| diagnostics.clone())
            .unwrap_or_default()
    };
    export_diagnostics_snapshot(Path::new(&destination), &snapshot)
}

#[derive(Debug, Clone, Serialize)]
struct AgentStatsRow {
    name: String,
    total_seconds: u64,
    error_count: u32,
    done_count: u32,
}

#[derive(Debug, Clone, Serialize)]
struct StatsReportDay {
    date: String,
    agents: Vec<AgentStatsRow>,
}

#[derive(Debug, Clone, Serialize)]
struct StatsReport {
    days: Vec<StatsReportDay>,
}

#[tauri::command]
fn get_stats_report(state: tauri::State<AppState>, days: u32) -> StatsReport {
    let session = state.session.lock().unwrap();
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

fn find_agent_cwd(sys: &mut System, name: &str) -> Result<String, String> {
    let keyword = keyword_for(name).ok_or_else(|| format!("unknown agent: {name}"))?;
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let processes = matching_processes(sys, &keyword);
    processes
        .iter()
        .find_map(|p| p.cwd().map(|path| path.to_string_lossy().into_owned()))
        .ok_or_else(|| "agent working directory not found".to_string())
}

#[tauri::command]
fn open_project_dir(name: String, state: tauri::State<AppState>) -> Result<(), String> {
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
fn open_path(path: String) -> Result<(), String> {
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
fn open_terminal(name: String, state: tauri::State<AppState>) -> Result<(), String> {
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

fn find_agent_pids(sys: &mut System, name: &str) -> Result<Vec<u32>, String> {
    let keyword = keyword_for(name).ok_or_else(|| format!("unknown agent: {name}"))?;
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let pids = matching_processes(sys, &keyword)
        .iter()
        .map(|p| p.pid().as_u32())
        .collect();
    Ok(pids)
}

fn paths_match(a: &str, b: &str) -> bool {
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
fn quiet_command(program: &str) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    let mut command = std::process::Command::new(program);
    command.creation_flags(0x0800_0000);
    command
}

#[cfg(not(target_os = "windows"))]
fn quiet_command(program: &str) -> std::process::Command {
    std::process::Command::new(program)
}

fn find_session_cwd(name: &str, session_id: &str) -> Result<String, String> {
    build_sessions(name)
        .into_iter()
        .find(|s| s.id == session_id)
        .and_then(|s| s.cwd)
        .ok_or_else(|| "session not found or has no working directory".to_string())
}

fn open_terminal_in_dir(dir: &str) -> Result<(), String> {
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
fn restart_program_available(program: &str, cwd: Option<&str>) -> Result<bool, String> {
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

fn spawn_command_in_dir(cmd: &[String], cwd: Option<&str>) -> Result<(), String> {
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

fn resume_command_for(name: &str, session_id: &str, base: &[String]) -> Vec<String> {
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

fn send_command_for(
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

fn fallback_send_command(
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
        _ => unreachable!(),
    }
    let mut cmd = vec!["cmd.exe".into()];
    cmd.extend(args);
    Ok(cmd)
}

fn looks_like_shim(program: &str) -> bool {
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

fn wrap_with_cmd(cmd: &[String]) -> Vec<String> {
    let mut wrapped = vec!["cmd.exe".to_string(), "/C".to_string()];
    wrapped.extend(cmd.iter().cloned());
    wrapped
}

fn clean_agent_base(name: &str, base: &[String]) -> Vec<String> {
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
fn stop_agent(name: String, state: tauri::State<AppState>) -> Result<(), String> {
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
fn restart_agent(name: String, state: tauri::State<AppState>) -> Result<(), String> {
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
fn open_session_terminal(name: String, session_id: String) -> Result<(), String> {
    let dir = find_session_cwd(&name, &session_id)?;
    open_terminal_in_dir(&dir)
}

#[tauri::command]
fn restart_session(
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

fn interactive_resume_command(
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
fn send_to_session(
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

fn read_pipe_to_task<R: Read + Send + 'static>(reader: R, task: Arc<SendTask>) {
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
struct SendOutput {
    done: bool,
    lines: Vec<String>,
}

#[tauri::command]
fn get_send_output(task_id: String, state: tauri::State<AppState>) -> Result<SendOutput, String> {
    let mut tasks = state
        .send_tasks
        .lock()
        .map_err(|_| "state lock error".to_string())?;
    let task = tasks
        .get(&task_id)
        .ok_or_else(|| "task not found".to_string())?;
    let done = task.done.load(Ordering::SeqCst);
    let lines = task.lines.lock().unwrap().clone();
    if done {
        tasks.remove(&task_id);
    }
    Ok(SendOutput { done, lines })
}

#[tauri::command]
fn stop_session(
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

const AUTOSTART_NAME: &str = "AgentIsland";

#[cfg(target_os = "windows")]
fn autostart_enabled() -> bool {
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
fn autostart_enabled() -> bool {
    false
}

#[cfg(target_os = "windows")]
fn apply_autostart(enabled: bool) -> Result<(), String> {
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
fn apply_autostart(enabled: bool) -> Result<(), String> {
    let _ = enabled;
    Err("not supported on this platform".into())
}

#[tauri::command]
fn get_autostart() -> bool {
    autostart_enabled()
}

#[tauri::command]
fn set_autostart(enabled: bool) -> Result<(), String> {
    apply_autostart(enabled)
}

const RECORDING_KEYWORDS: &[&str] = &[
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

#[tauri::command]
fn privacy_active(state: tauri::State<AppState>) -> bool {
    let mut sys = state.sys.lock().unwrap();
    sys.refresh_processes(ProcessesToUpdate::All, false);
    sys.processes().values().any(|p| {
        let name = p.name().to_string_lossy().to_lowercase();
        RECORDING_KEYWORDS.iter().any(|k| name.contains(k))
    })
}

fn toggle_window(app: &tauri::AppHandle) {
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
fn open_overview(app: tauri::AppHandle) -> Result<(), String> {
    let window = app
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

#[cfg(target_os = "windows")]
fn start_global_hotkeys(app: tauri::AppHandle) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interactive_resume_works_without_a_recorded_process_command() {
        assert_eq!(
            interactive_resume_command("Codex CLI", "synthetic-session", None).unwrap(),
            vec!["codex", "resume", "synthetic-session"]
        );
        assert_eq!(
            interactive_resume_command("OpenCode", "synthetic-session", None).unwrap(),
            resume_command_for("OpenCode", "synthetic-session", &["opencode".into()])
        );
        assert!(interactive_resume_command("Unknown", "synthetic-session", None).is_err());
        assert!(interactive_resume_command("Codex CLI", "", None).is_err());
    }

    #[test]
    fn restart_rejects_empty_and_shell_control_arguments_before_launch() {
        assert!(spawn_command_in_dir(&[], None).is_err());
        assert!(spawn_command_in_dir(&["codex".into(), "session&other".into()], None).is_err());
        assert!(spawn_command_in_dir(&["codex".into(), "%PATH%".into()], None).is_err());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn restart_preflight_accepts_absolute_executable_paths_without_where_pattern_parsing() {
        let fixture = std::env::temp_dir().join(format!("island-启动测试 {}", std::process::id()));
        std::fs::create_dir_all(&fixture).unwrap();
        let shim = fixture.join("agent shim.cmd");
        std::fs::write(&shim, "@echo off\r\n").unwrap();
        assert!(restart_program_available(shim.to_str().unwrap(), None).unwrap());
        assert!(restart_program_available(".\\agent shim.cmd", fixture.to_str()).unwrap());
        std::fs::remove_file(shim).unwrap();
        std::fs::remove_dir(fixture).unwrap();
        let executable = std::env::current_exe().unwrap();
        assert!(restart_program_available(executable.to_str().unwrap(), None).unwrap());
        assert!(restart_program_available("cmd.exe", None).unwrap());
        assert!(
            !restart_program_available("C:\\不存在的测试目录\\Agent Tools\\missing.exe", None)
                .unwrap()
        );
        assert!(!restart_program_available("missing-agent-87a310.cmd", None).unwrap());
    }

    #[test]
    fn hook_approval_decision_rejects_unknown_id() {
        let mut approvals = HashMap::new();
        assert_eq!(
            set_hook_approval_decision(&mut approvals, "missing", "allow"),
            Err("approval_not_found".to_string())
        );
    }

    fn terminal_snapshot(
        turn_id: &str,
        turn: domain::TurnState,
        result_at_ms: u64,
        stale: bool,
        process: domain::ProcessState,
        active_turn_session_id: &str,
    ) -> interface::snapshot::AgentViewSnapshot {
        let active_session = interface::snapshot::SessionView {
            id: "matched-session".into(),
            name: "matched-session".into(),
            cwd: Some(r"D:\work\active".into()),
            log_path: None,
            records: Vec::new(),
            recent_output: Vec::new(),
            current_file: None,
            log_status: None,
            alert: None,
            lifecycle: domain::SessionLifecycle::Active,
            last_active_at_ms: result_at_ms,
            display_status: domain::DisplayStatus::Done,
        };
        interface::snapshot::AgentViewSnapshot {
            schema_version: 1,
            generated_at_ms: result_at_ms.saturating_add(1),
            agents: vec![interface::snapshot::AgentView {
                id: "codex".into(),
                name: "Codex CLI".into(),
                can_restart: false,
                state: domain::AgentState {
                    process,
                    turn,
                    attention: domain::AttentionState::None,
                    result_at_ms: Some(result_at_ms),
                },
                display_status: domain::DisplayStatus::Done,
                active_session: Some(active_session),
                active_turn: Some(interface::snapshot::ActiveTurnIdentity {
                    agent_id: "codex".into(),
                    session_id: active_turn_session_id.into(),
                    turn_id: turn_id.into(),
                }),
                history_sessions: Vec::new(),
                diagnostic: None,
                freshness: domain::Freshness {
                    observed_at_ms: result_at_ms,
                    stale,
                },
                usage: None,
            }],
        }
    }

    #[test]
    fn terminal_statistics_deduplicate_same_poll_but_count_a_new_turn() {
        let mut stats = HashMap::new();
        let mut daily = HashMap::new();
        let first = terminal_snapshot(
            "turn-1",
            domain::TurnState::Succeeded,
            1_000,
            false,
            domain::ProcessState::Running,
            "matched-session",
        );

        assert!(update_terminal_statistics_from_snapshot(
            &mut stats,
            &mut daily,
            "2026-09-06",
            &first
        ));
        assert!(!update_terminal_statistics_from_snapshot(
            &mut stats,
            &mut daily,
            "2026-09-06",
            &first
        ));
        let second = terminal_snapshot(
            "turn-2",
            domain::TurnState::Succeeded,
            2_000,
            false,
            domain::ProcessState::Running,
            "matched-session",
        );
        assert!(update_terminal_statistics_from_snapshot(
            &mut stats,
            &mut daily,
            "2026-09-06",
            &second
        ));
        assert_eq!(stats["Codex CLI"].done_count, 2);
        assert_eq!(daily["2026-09-06"]["Codex CLI"].done_count, 2);
    }

    #[test]
    fn terminal_statistics_never_recount_a_recent_terminal_after_a_session_revisit_or_reload() {
        let mut stats = HashMap::new();
        let mut daily = HashMap::new();
        let mut first = terminal_snapshot(
            "turn-a",
            domain::TurnState::Succeeded,
            1_000,
            false,
            domain::ProcessState::Running,
            "matched-session",
        );
        let mut second = terminal_snapshot(
            "turn-b",
            domain::TurnState::Succeeded,
            2_000,
            false,
            domain::ProcessState::Running,
            "matched-session",
        );
        first.agents[0].active_session.as_mut().unwrap().id = "session-a".into();
        first.agents[0].active_turn.as_mut().unwrap().session_id = "session-a".into();
        second.agents[0].active_session.as_mut().unwrap().id = "session-b".into();
        second.agents[0].active_turn.as_mut().unwrap().session_id = "session-b".into();

        assert!(update_terminal_statistics_from_snapshot(
            &mut stats,
            &mut daily,
            "2026-09-06",
            &first,
        ));
        assert!(update_terminal_statistics_from_snapshot(
            &mut stats,
            &mut daily,
            "2026-09-06",
            &second,
        ));
        assert!(!update_terminal_statistics_from_snapshot(
            &mut stats,
            &mut daily,
            "2026-09-06",
            &first,
        ));

        let path = std::env::temp_dir().join(format!(
            "agent-island-stats-revisit-{}-{}.json",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        save_stats(&stats, &path).unwrap();
        let mut reloaded = load_stats(&path);
        let _ = fs::remove_file(&path);
        assert!(!update_terminal_statistics_from_snapshot(
            &mut reloaded,
            &mut HashMap::new(),
            "2026-09-06",
            &first,
        ));
        assert_eq!(reloaded["Codex CLI"].done_count, 2);
    }

    #[test]
    fn terminal_statistics_serialize_only_opaque_hashes_and_migrate_legacy_raw_keys() {
        let mut stats = HashMap::new();
        let mut daily = HashMap::new();
        let first = terminal_snapshot(
            "turn-private",
            domain::TurnState::Succeeded,
            1_000,
            false,
            domain::ProcessState::Running,
            "matched-session",
        );
        assert!(update_terminal_statistics_from_snapshot(
            &mut stats,
            &mut daily,
            "2026-09-06",
            &first,
        ));
        let frontend_json = serde_json::to_string(&stats["Codex CLI"]).unwrap();
        assert!(frontend_json.contains("recent_terminal_hashes"));
        for identifier in ["codex", "matched-session", "turn-private"] {
            assert!(!frontend_json.contains(identifier));
        }

        let legacy_key = "codex\u{1f}matched-session\u{1f}turn-private\u{1f}done\u{1f}1000";
        let path = std::env::temp_dir().join(format!(
            "agent-island-stats-legacy-{}-{}.json",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        fs::write(
            &path,
            serde_json::json!({
                "Codex CLI": {
                    "done_count": 1,
                    "last_transition_key": legacy_key,
                }
            })
            .to_string(),
        )
        .unwrap();
        let mut loaded = load_stats(&path);
        assert!(!update_terminal_statistics_from_snapshot(
            &mut loaded,
            &mut HashMap::new(),
            "2026-09-06",
            &first,
        ));
        save_stats(&loaded, &path).unwrap();
        let persisted_json = fs::read_to_string(&path).unwrap();
        let _ = fs::remove_file(&path);
        assert!(!persisted_json.contains(legacy_key));
        assert!(persisted_json.contains("recent_terminal_hashes"));
    }

    #[test]
    fn terminal_statistics_retain_at_most_128_recent_hashes() {
        let mut stats = HashMap::new();
        let mut daily = HashMap::new();
        for index in 0..129 {
            let snapshot = terminal_snapshot(
                &format!("turn-{index}"),
                domain::TurnState::Succeeded,
                index + 1,
                false,
                domain::ProcessState::Running,
                "matched-session",
            );
            assert!(update_terminal_statistics_from_snapshot(
                &mut stats,
                &mut daily,
                "2026-09-06",
                &snapshot,
            ));
        }
        assert_eq!(stats["Codex CLI"].done_count, 129);
        assert_eq!(stats["Codex CLI"].recent_terminal_hashes.len(), 128);
    }

    #[test]
    fn terminal_statistics_ignore_stale_mismatched_and_stopped_snapshots() {
        let mut stats = HashMap::new();
        let mut daily = HashMap::new();
        for snapshot in [
            terminal_snapshot(
                "turn-1",
                domain::TurnState::Failed,
                1_000,
                true,
                domain::ProcessState::Running,
                "matched-session",
            ),
            terminal_snapshot(
                "turn-2",
                domain::TurnState::Failed,
                2_000,
                false,
                domain::ProcessState::Running,
                "unrelated-history",
            ),
            terminal_snapshot(
                "turn-3",
                domain::TurnState::Failed,
                3_000,
                false,
                domain::ProcessState::Stopped,
                "matched-session",
            ),
        ] {
            assert!(!update_terminal_statistics_from_snapshot(
                &mut stats,
                &mut daily,
                "2026-09-06",
                &snapshot,
            ));
        }
        let mut newer_history = terminal_snapshot(
            "turn-current",
            domain::TurnState::Succeeded,
            4_000,
            false,
            domain::ProcessState::Running,
            "matched-session",
        );
        newer_history.agents[0].state.turn = domain::TurnState::Executing;
        newer_history.agents[0].state.result_at_ms = None;
        let history_summary = interface::snapshot::SessionSummary::from(
            newer_history.agents[0].active_session.as_ref().unwrap(),
        );
        newer_history.agents[0]
            .history_sessions
            .push(history_summary);
        newer_history.agents[0].history_sessions[0].id = "newer-history".into();
        newer_history.agents[0].history_sessions[0].last_active_at_ms = 9_000;
        newer_history.agents[0].history_sessions[0].display_status = domain::DisplayStatus::Error;
        assert!(!update_terminal_statistics_from_snapshot(
            &mut stats,
            &mut daily,
            "2026-09-06",
            &newer_history,
        ));
        assert!(stats.is_empty());
        assert!(daily.is_empty());
    }

    #[test]
    fn diagnostics_reads_the_current_cache_generation_without_scanning() {
        let generated_at_ms = 77;
        let cache = Some((
            Instant::now(),
            Vec::new(),
            interface::snapshot::AgentViewSnapshot {
                schema_version: 1,
                generated_at_ms,
                agents: Vec::new(),
            },
            interface::diagnostics::DiagnosticSnapshot {
                generated_at_ms,
                records: Vec::new(),
            },
        ));

        let diagnostics = diagnostics_from_cache(&cache);

        assert_eq!(diagnostics.generated_at_ms, generated_at_ms);
        assert!(diagnostics.issues.is_empty());
    }

    #[test]
    fn diagnostics_export_writes_exact_caller_selected_path() {
        let destination = std::env::temp_dir().join(format!(
            "agent-island-diagnostics-{}-selected.json",
            std::process::id()
        ));
        let snapshot = interface::diagnostics::DiagnosticSnapshot {
            generated_at_ms: 77,
            records: Vec::new(),
        };

        export_diagnostics_snapshot(&destination, &snapshot).unwrap();

        let value: Value =
            serde_json::from_str(&fs::read_to_string(&destination).unwrap()).unwrap();
        assert_eq!(value["generated_at_ms"], 77);
        assert!(destination.exists());
        fs::remove_file(destination).unwrap();
    }

    #[test]
    fn file_scan_with_no_valid_events_keeps_adapter_isolated_and_reports_parse_failure() {
        let path =
            std::env::temp_dir().join(format!("agent-island-invalid-{}.jsonl", std::process::id()));
        fs::write(&path, "not-json\n{\"type\":\"unknown\"}\n").unwrap();

        let scan = session_scan_from_file("claude", &path).unwrap();

        assert_eq!(scan.legacy_sessions.len(), 1);
        assert_eq!(scan.candidates.len(), 1);
        assert_eq!(scan.diagnostics.len(), 1);
        assert_eq!(scan.diagnostics[0].view.code, "parse_failed");
        assert_eq!(scan.diagnostics[0].view.skipped_lines, Some(1));
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn claude_message_only_scan_with_malformed_neighbor_reports_skipped_lines() {
        let path = std::env::temp_dir().join(format!(
            "agent-island-claude-message-only-{}.jsonl",
            std::process::id()
        ));
        fs::write(
            &path,
            concat!(
                "{\"sessionId\":\"claude-message-only\",\"uuid\":\"message-1\",\"timestamp\":\"2026-09-04T10:00:00Z\",\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"hello\"}],\"stop_reason\":null}}\n",
                "not-json\n",
            ),
        )
        .unwrap();

        let scan = session_scan_from_file("claude", &path).unwrap();

        assert_eq!(scan.diagnostics[0].view.skipped_lines, Some(1));
        assert_eq!(scan.candidates[0].view.records[0].text, "hello");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn codex_message_only_scan_with_malformed_neighbor_reports_skipped_lines() {
        let path = std::env::temp_dir().join(format!(
            "agent-island-codex-message-only-{}.jsonl",
            std::process::id()
        ));
        fs::write(
            &path,
            concat!(
                "{\"timestamp\":\"2026-09-04T10:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"codex-message-only\",\"cwd\":\"D:\\\\work\\\\app\"}}\n",
                "{\"timestamp\":\"2026-09-04T10:00:01Z\",\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"hello\"}]}}\n",
                "not-json\n",
            ),
        )
        .unwrap();

        let scan = session_scan_from_file("codex", &path).unwrap();

        assert_eq!(scan.diagnostics[0].view.skipped_lines, Some(1));
        assert_eq!(scan.candidates[0].view.records[0].text, "hello");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn acquisition_missing_file_is_diagnostic_not_successful_empty_scan() {
        let path =
            std::env::temp_dir().join(format!("agent-island-missing-{}.jsonl", epoch_millis()));
        let scan = session_scan_from_file("codex", &path).expect("failed acquisition must survive");
        assert_eq!(scan.diagnostics[0].view.code, "log_unavailable");
        assert!(scan.diagnostics[0].view.freshness.stale);
    }

    #[test]
    fn acquisition_index_reads_long_codex_metadata_record() {
        let path = std::env::temp_dir().join(format!(
            "agent-island-long-metadata-{}.jsonl",
            epoch_millis()
        ));
        let metadata = serde_json::json!({
            "type": "session_meta",
            "payload": { "id": "long-header", "cwd": "D:/target",
                "instructions": "x".repeat(48 * 1024) }
        });
        fs::write(&path, format!("{metadata}\n{{\"type\":\"unknown\"}}\n")).unwrap();
        let scan = index_session_file("codex", &path);
        fs::remove_file(&path).unwrap();
        assert_eq!(scan.acquisition, AcquisitionCompleteness::Complete);
        assert_eq!(scan.candidates[0].identity.session_id, "long-header");
        assert_eq!(
            scan.candidates[0].identity.project_path.as_deref(),
            Some("D:/target")
        );
    }

    #[test]
    fn acquisition_indexed_identity_disappearing_in_detail_gates_ownership() {
        for (name, replacement) in [("empty", ""), ("identity_removed", "{\"type\":\"unknown\"}\n"), ("replaced", "{\"type\":\"session_meta\",\"payload\":{\"id\":\"replacement\",\"cwd\":\"D:/other\"}}\n")] {
            let path = std::env::temp_dir().join(format!("agent-island-d1-race-{name}-{}.jsonl", epoch_millis()));
            fs::write(&path, "{\"type\":\"session_meta\",\"payload\":{\"id\":\"indexed\",\"cwd\":\"D:/target\"}}\n").unwrap();
            let mut scan = index_session_file("codex", &path);
            fs::write(&path, replacement).unwrap();
            let independent_read = session_scan_from_file("codex", &path).unwrap();
            load_session_detail(&mut scan, "codex", &path);
            fs::remove_file(&path).unwrap();
            if name == "empty" {
                assert_eq!(independent_read.acquisition, AcquisitionCompleteness::Complete);
                assert!(independent_read.candidates.is_empty());
            }
            assert_eq!(scan.acquisition, AcquisitionCompleteness::Incomplete, "{name}");
            assert!(scan.diagnostics.iter().any(|record| record.view.code == "acquisition_incomplete"), "{name}");
            let process = process_fact_from_observations("codex", "Codex CLI", &[ProcessObservation { pid: 42, project_path: Some("D:/target".into()), started_at_ms: 0 }]);
            let issues = scan.diagnostics.iter().map(|record| record.view.clone()).collect::<Vec<_>>();
            let snapshot = interface::snapshot::build_snapshot_with_acquisition(epoch_millis(), &[process], &scan.candidates, &issues);
            assert!(snapshot.agents[0].active_session.is_none());
            assert!(snapshot.agents[0].freshness.stale);
            assert_eq!(snapshot.agents[0].history_sessions[0].id, "indexed");
        }
    }

    #[test]
    fn discovery_visible_history_is_hydrated_for_every_active_position() {
        for (active, expected_history) in [
            (Some(4), ["answer 3", "answer 2", "answer 1"]),
            (Some(2), ["answer 4", "answer 3", "answer 1"]),
            (Some(0), ["answer 4", "answer 3", "answer 2"]),
            (None, ["answer 4", "answer 3", "answer 2"]),
        ] {
            let root = std::env::temp_dir().join(format!(
                "agent-island-d1-visible-{active:?}-{}",
                epoch_millis()
            ));
            fs::create_dir_all(&root).unwrap();
            for index in 0..5 {
                let path = root.join(format!("{index}.jsonl"));
                fs::write(&path, format!("{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"s{index}\",\"cwd\":\"D:/project{index}\"}}}}\n{{\"timestamp\":\"2026-09-04T10:00:01Z\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{{\"type\":\"output_text\",\"text\":\"answer {index}\"}}]}}}}\n{{\"timestamp\":\"2026-09-04T10:00:02Z\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"task_complete\",\"turn_id\":\"turn\"}}}}\n")).unwrap();
                File::options()
                    .write(true)
                    .open(path)
                    .unwrap()
                    .set_modified(UNIX_EPOCH + std::time::Duration::from_secs(10 + index))
                    .unwrap();
            }
            let process = process_fact_from_observations(
                "codex",
                "Codex CLI",
                &[ProcessObservation {
                    pid: 42,
                    project_path: Some(
                        active
                            .map(|id| format!("D:/project{id}"))
                            .unwrap_or("D:/unrelated".into()),
                    ),
                    started_at_ms: 0,
                }],
            );
            let scan = scan_session_directory("codex", &root, Some(&process.identity));
            let issues = scan
                .diagnostics
                .iter()
                .map(|record| record.view.clone())
                .collect::<Vec<_>>();
            let snapshot = interface::snapshot::build_snapshot_with_acquisition(
                epoch_millis(),
                &[process],
                &scan.candidates,
                &issues,
            );
            fs::remove_dir_all(root).unwrap();
            assert_eq!(scan.acquisition, AcquisitionCompleteness::Complete);
            assert_eq!(
                scan.legacy_sessions.len(),
                if active.is_some() { 4 } else { 3 }
            );
            let serialized = serde_json::to_value(&snapshot.agents[0]).unwrap();
            for (row, expected) in serialized["history_sessions"]
                .as_array()
                .unwrap()
                .iter()
                .zip(expected_history)
            {
                assert_eq!(row["records"][0]["text"], expected, "active {active:?}");
                assert_eq!(row["display_status"], "done");
            }
            assert_eq!(
                snapshot.agents[0]
                    .active_session
                    .as_ref()
                    .map(|session| session.id.clone()),
                active.map(|id| format!("s{id}"))
            );
        }
    }

    #[test]
    fn acquisition_parse_diagnostics_are_stale_in_snapshot_and_export() {
        let path =
            std::env::temp_dir().join(format!("agent-island-d1-partial-{}.jsonl", epoch_millis()));
        fs::write(&path, "{\"type\":\"session_meta\",\"payload\":{\"id\":\"current\",\"cwd\":\"D:/target\"}}\n{\"timestamp\":\"2099-01-01T00:00:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\",\"message\":\"hello\",\"turn_id\":\"turn\"}}\nnot-json\n").unwrap();
        let partial = session_scan_from_file("codex", &path).unwrap();
        fs::remove_file(path).unwrap();
        let invalid = hermes_session_scan_from_text("invalid row\n");
        for (adapter, scan, expected_code) in [
            ("codex", partial, "partial_parse"),
            ("hermes", invalid, "parse_failed"),
        ] {
            assert_eq!(scan.diagnostics[0].view.code, expected_code);
            assert!(scan.diagnostics[0].view.freshness.stale, "{adapter}");
            let process = process_fact_from_observations(
                adapter,
                adapter,
                &[ProcessObservation {
                    pid: 42,
                    project_path: Some("D:/target".into()),
                    started_at_ms: 0,
                }],
            );
            let mut issue = scan.diagnostics[0].view.clone();
            issue.freshness.stale = false;
            let snapshot = interface::snapshot::build_snapshot_with_acquisition(
                epoch_millis(),
                &[process],
                &scan.candidates,
                &[issue],
            );
            assert!(snapshot.agents[0].freshness.stale);
            assert!(
                snapshot.agents[0]
                    .diagnostic
                    .as_ref()
                    .unwrap()
                    .freshness
                    .stale
            );
            let diagnostics = interface::diagnostics::DiagnosticSnapshot {
                generated_at_ms: epoch_millis(),
                records: scan.diagnostics,
            };
            let export = serde_json::to_value(interface::diagnostics::build_export_report(
                &diagnostics,
                "test",
                "windows",
            ))
            .unwrap();
            assert_eq!(export["entries"][0]["stale"], true);
        }
    }

    #[test]
    fn discovery_detail_changing_indexed_time_evidence_gates_ownership() {
        let root =
            std::env::temp_dir().join(format!("agent-island-d1-time-change-{}", epoch_millis()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("session.jsonl");
        fs::write(&path, "{\"type\":\"session_meta\",\"payload\":{\"id\":\"current\",\"cwd\":\"D:/target\"}}\n{\"timestamp\":\"2026-09-04T10:00:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\",\"message\":\"hello\",\"turn_id\":\"turn\"}}\n").unwrap();
        let process = process_fact_from_observations(
            "codex",
            "Codex CLI",
            &[ProcessObservation {
                pid: 42,
                project_path: Some("D:/target".into()),
                started_at_ms: epoch_millis(),
            }],
        );
        let scan = scan_session_directory("codex", &root, Some(&process.identity));
        fs::remove_dir_all(root).unwrap();
        assert_eq!(scan.acquisition, AcquisitionCompleteness::Incomplete);
        assert_eq!(scan.diagnostics[0].view.code, "acquisition_incomplete");
        assert_eq!(scan.candidates[0].view.records[0].text, "hello");
    }

    #[test]
    fn acquisition_short_stream_preserves_valid_prefix_with_incomplete_evidence() {
        let text =
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"current\",\"cwd\":\"D:/target\"}}\n";
        let mut source = std::io::Cursor::new(text.as_bytes());
        let mut acquired = AcquiredText::default();
        read_bounded_stream(
            &mut source,
            text.len() as u64 + 20,
            1024,
            1024,
            &mut acquired,
        );
        assert_eq!(acquired.text, text);
        assert_eq!(
            acquired.issues,
            vec![domain::DataIssue::AcquisitionIncomplete]
        );
    }

    #[test]
    fn discovery_four_files_keep_same_project_conflict_before_display_limit() {
        let root = std::env::temp_dir().join(format!("agent-island-discovery-{}", epoch_millis()));
        fs::create_dir_all(&root).unwrap();
        for (index, project) in ["D:/target", "D:/target", "D:/other1", "D:/other2"]
            .iter()
            .enumerate()
        {
            let path = root.join(format!("{index}.jsonl"));
            fs::write(&path, format!("{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"s{index}\",\"cwd\":\"{project}\"}}}}\n")).unwrap();
            File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_modified(UNIX_EPOCH + std::time::Duration::from_secs(10 + index as u64))
                .unwrap();
        }
        let scan = scan_session_directory("codex", &root, None);
        let process = process_fact_from_observations(
            "codex",
            "Codex CLI",
            &[ProcessObservation {
                pid: 42,
                project_path: Some("D:/target".into()),
                started_at_ms: 0,
            }],
        );
        let snapshot = interface::snapshot::build_snapshot(20_000, &[process], &scan.candidates);
        fs::remove_dir_all(root).unwrap();
        assert!(
            snapshot.agents[0].active_session.is_none(),
            "omitted fourth identity falsely proves ownership"
        );
        assert_eq!(
            snapshot.agents[0]
                .diagnostic
                .as_ref()
                .unwrap()
                .candidate_count,
            Some(2)
        );
    }

    #[cfg(windows)]
    #[test]
    fn acquisition_process_launched_after_system_has_matching_command_and_cwd() {
        let mut system = System::new_all();
        let root = std::env::temp_dir().join(format!("agent-island-child-{}", epoch_millis()));
        fs::create_dir_all(&root).unwrap();
        let mut child = quiet_command("powershell.exe")
            .args([
                "-NoProfile",
                "-Command",
                "Start-Sleep -Seconds 20 # agent-island-refresh-probe",
            ])
            .current_dir(&root)
            .spawn()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(250));
        refresh_agent_processes(&mut system);
        let discovered = matching_processes(&system, "agent-island-refresh-probe")
            .into_iter()
            .find(|process| process.pid().as_u32() == child.id());
        let cwd = discovered
            .and_then(|process| process.cwd())
            .map(Path::to_path_buf);
        let exe = discovered
            .and_then(|process| process.exe())
            .map(Path::to_path_buf);
        let _ = child.kill();
        let _ = child.wait();
        fs::remove_dir_all(&root).unwrap();
        assert!(
            discovered.is_some(),
            "new process command line missing from production refresh"
        );
        assert_eq!(cwd, Some(root));
        assert!(exe.is_some());
    }

    #[test]
    fn acquisition_incomplete_universe_rejects_even_pid_confirmed_ownership() {
        let path =
            std::env::temp_dir().join(format!("agent-island-incomplete-{}.jsonl", epoch_millis()));
        fs::write(
            &path,
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"current\",\"cwd\":\"D:/target\"}}\n",
        )
        .unwrap();
        let mut scan = session_scan_from_file("codex", &path).unwrap();
        scan.candidates[0].identity.process_ids = vec![42];
        let missing = session_scan_from_file("codex", &path.with_extension("missing")).unwrap();
        let combined = combine_session_scans([scan, missing].into_iter());
        let process = process_fact_from_observations(
            "codex",
            "Codex CLI",
            &[ProcessObservation {
                pid: 42,
                project_path: Some("D:/target".into()),
                started_at_ms: 0,
            }],
        );
        let issues = combined
            .diagnostics
            .iter()
            .map(|record| record.view.clone())
            .collect::<Vec<_>>();
        let snapshot = interface::snapshot::build_snapshot_with_acquisition(
            epoch_millis(),
            &[process],
            &combined.candidates,
            &issues,
        );
        fs::remove_file(path).unwrap();
        assert!(snapshot.agents[0].active_session.is_none());
        assert!(snapshot.agents[0].freshness.stale);
        assert_eq!(
            snapshot.agents[0].diagnostic.as_ref().unwrap().code,
            "log_unavailable"
        );
        assert_eq!(snapshot.agents[0].history_sessions.len(), 1);
    }

    #[test]
    fn acquisition_failed_hermes_command_is_diagnostic() {
        let mut command = quiet_command("agent-island-nonexistent-cli-638518");
        let scan = hermes_scan_from_command(&mut command, 1);
        assert_eq!(scan.acquisition, AcquisitionCompleteness::Incomplete);
        assert_eq!(scan.diagnostics[0].view.code, "log_unavailable");
    }

    #[cfg(windows)]
    #[test]
    fn acquisition_nonzero_hermes_command_differs_from_successful_empty() {
        let mut failed = quiet_command("cmd.exe");
        failed.args(["/D", "/C", "exit 7"]);
        let failed = hermes_scan_from_command(&mut failed, 2);
        assert_eq!(failed.acquisition, AcquisitionCompleteness::Incomplete);
        assert_eq!(failed.diagnostics[0].view.code, "log_unavailable");
        let mut empty = quiet_command("cmd.exe");
        empty.args(["/D", "/C", "exit 0"]);
        let empty = hermes_scan_from_command(&mut empty, 2);
        assert_eq!(empty.acquisition, AcquisitionCompleteness::Complete);
        assert!(empty.diagnostics.is_empty());
        assert!(empty.candidates.is_empty());
    }

    #[test]
    fn acquisition_directory_failure_is_not_successful_empty_enumeration() {
        let root = std::env::temp_dir().join(format!("agent-island-empty-{}", epoch_millis()));
        let missing = scan_session_directory("codex", &root, None);
        assert_eq!(missing.acquisition, AcquisitionCompleteness::Incomplete);
        assert_eq!(missing.diagnostics[0].view.code, "log_unavailable");
        fs::create_dir_all(&root).unwrap();
        let empty = scan_session_directory("codex", &root, None);
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(empty.acquisition, AcquisitionCompleteness::Complete);
        assert!(empty.diagnostics.is_empty());
        assert!(empty.candidates.is_empty());
    }

    #[test]
    fn discovery_oldest_matching_file_gets_detail_before_history_cap() {
        let root = std::env::temp_dir().join(format!("agent-island-oldest-{}", epoch_millis()));
        fs::create_dir_all(&root).unwrap();
        for index in 0..5 {
            let path = root.join(format!("{index}.jsonl"));
            fs::write(&path, format!("{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"s{index}\",\"cwd\":\"D:/project{index}\"}}}}\n{{\"timestamp\":\"2026-09-04T10:00:01Z\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{{\"type\":\"output_text\",\"text\":\"answer {index}\"}}]}}}}\n")).unwrap();
            File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_modified(UNIX_EPOCH + std::time::Duration::from_secs(10 + index))
                .unwrap();
        }
        let process = process_fact_from_observations(
            "codex",
            "Codex CLI",
            &[ProcessObservation {
                pid: 42,
                project_path: Some("D:/project0".into()),
                started_at_ms: 0,
            }],
        );
        let scan = scan_session_directory("codex", &root, Some(&process.identity));
        let snapshot = interface::snapshot::build_snapshot(20_000, &[process], &scan.candidates);
        fs::remove_dir_all(root).unwrap();
        assert_eq!(scan.candidates.len(), 5);
        assert_eq!(scan.legacy_sessions.len(), 4);
        assert_eq!(
            snapshot.agents[0].active_session.as_ref().unwrap().records[0].text,
            "answer 0"
        );
        assert_eq!(snapshot.agents[0].history_sessions.len(), 3);
    }

    #[test]
    fn codex_data_root_honors_override_without_mutating_environment() {
        use std::ffi::OsStr;
        let home = Path::new("C:/Users/测试 用户");
        assert_eq!(codex_data_root_from(None, Some(home)), Some(home.join(".codex")));
        assert_eq!(codex_data_root_from(Some(OsStr::new("")), Some(home)), Some(home.join(".codex")));
        let custom = OsStr::new("D:/自定义 数据/codex");
        assert_eq!(codex_data_root_from(Some(custom), Some(home)), Some(PathBuf::from(custom)));
        assert_eq!(codex_data_root_from(Some(custom), None), Some(PathBuf::from(custom)));
        assert_eq!(codex_data_root_from(None, None), None);
    }

    #[test]
    fn discovery_desktop_root_precedes_helpers_without_discarding_identity_evidence() {
        let root = std::env::temp_dir().join(format!("agent-island-desktop-{}", epoch_millis()));
        fs::create_dir_all(&root).unwrap();
        for index in 0..5 {
            let path = root.join(format!("{index}.jsonl"));
            let mut payload = serde_json::json!({"id": format!("s{index}"), "cwd":"D:/project", "originator":"Codex Desktop", "source":"vscode", "thread_source":"user"});
            if index != 0 {
                payload["source"] = serde_json::json!({"subagent":{}});
                payload["parent_thread_id"] = serde_json::json!("s0");
                payload["thread_source"] = serde_json::json!("guardian_review");
            }
            fs::write(&path, format!("{}\n", serde_json::json!({"type":"session_meta","payload":payload}))).unwrap();
            File::options().write(true).open(&path).unwrap().set_modified(UNIX_EPOCH + std::time::Duration::from_secs(10 + index)).unwrap();
        }
        let scan = scan_session_directory("codex", &root, None);
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(scan.candidates.len(), 5);
        assert_eq!(scan.candidates[0].identity.session_id, "s0");
        assert_eq!(scan.legacy_sessions.len(), 3);
        assert_eq!(scan.legacy_sessions[0].id, "s0");
        assert_eq!(indexed_active_session(&scan, None, 20_000), None);
        let process = process_fact_from_observations("codex", "Codex CLI", &[ProcessObservation { pid:42, project_path:Some("D:/project".into()), started_at_ms:0 }]);
        assert_eq!(indexed_active_session(&scan, Some(&process.identity), 20_000), None);
        let snapshot = interface::snapshot::build_snapshot(20_000, &[process], &scan.candidates);
        assert!(snapshot.agents[0].active_session.is_none());
        assert_eq!(snapshot.agents[0].history_sessions.len(), 3);
        assert_eq!(snapshot.agents[0].history_sessions[0].id, "s0");
    }

    #[test]
    fn desktop_priority_requires_explicit_root_metadata() {
        let valid = serde_json::json!({"type":"session_meta","payload":{"originator":"codex_work_desktop","source":"vscode","thread_source":"user"}});
        assert!(is_codex_desktop_root(&valid.to_string()));
        for (key, value) in [("originator", serde_json::json!("unknown")), ("source", serde_json::json!({"subagent":{}})), ("thread_source", serde_json::json!("guardian_review")), ("parent_thread_id", serde_json::json!("parent"))] {
            let mut invalid = valid.clone();
            invalid["payload"][key] = value;
            assert!(!is_codex_desktop_root(&invalid.to_string()));
        }
    }

    #[test]
    fn cache_timestamp_is_sampled_after_a_slow_scan() {
        use std::cell::Cell;
        use std::time::Duration;

        let base = Instant::now();
        let scans = Cell::new(0_u16);
        let mut cache = None;
        let mut first_clock = [base, base + Duration::from_secs(2)].into_iter();
        let (_, first_snapshot, first_diagnostics, first_scan) = collect_cached_with(
            &mut cache,
            || first_clock.next().unwrap(),
            || {
                scans.set(scans.get() + 1);
                (
                    Vec::new(),
                    interface::snapshot::AgentViewSnapshot {
                        schema_version: 1,
                        generated_at_ms: u64::from(scans.get()),
                        agents: Vec::new(),
                    },
                    interface::diagnostics::DiagnosticSnapshot {
                        generated_at_ms: u64::from(scans.get()),
                        records: Vec::new(),
                    },
                    false,
                )
            },
        );
        let (_, second_snapshot, second_diagnostics, second_scan) = collect_cached_with(
            &mut cache,
            || base + Duration::from_millis(2_100),
            || {
                scans.set(scans.get() + 1);
                (
                    Vec::new(),
                    interface::snapshot::AgentViewSnapshot {
                        schema_version: 1,
                        generated_at_ms: u64::from(scans.get()),
                        agents: Vec::new(),
                    },
                    interface::diagnostics::DiagnosticSnapshot {
                        generated_at_ms: u64::from(scans.get()),
                        records: Vec::new(),
                    },
                    false,
                )
            },
        );

        assert!(first_scan.is_some());
        assert!(second_scan.is_none());
        assert_eq!(scans.get(), 1);
        assert_eq!(
            first_snapshot.generated_at_ms,
            second_snapshot.generated_at_ms
        );
        assert_eq!(
            first_diagnostics.generated_at_ms,
            second_diagnostics.generated_at_ms
        );
    }

    #[test]
    fn process_fact_does_not_borrow_a_session_cwd_when_process_cwd_is_missing() {
        let fact = process_fact_from_observations(
            "codex",
            "Codex CLI",
            &[ProcessObservation {
                pid: 41,
                project_path: None,
                started_at_ms: 20_000,
            }],
        );

        assert_eq!(fact.identity.process_ids, vec![41]);
        assert_eq!(fact.identity.project_path, None);
        assert_eq!(fact.identity.started_at_ms, 20_000);
    }

    #[test]
    fn process_fact_keeps_all_pids_and_rejects_conflicting_process_cwds() {
        let fact = process_fact_from_observations(
            "codex",
            "Codex CLI",
            &[
                ProcessObservation {
                    pid: 41,
                    project_path: Some(r"D:\work\one".into()),
                    started_at_ms: 10_000,
                },
                ProcessObservation {
                    pid: 42,
                    project_path: Some(r"D:\work\two".into()),
                    started_at_ms: 20_000,
                },
            ],
        );

        assert_eq!(fact.identity.process_ids, vec![41, 42]);
        assert_eq!(fact.identity.project_path, None);
        assert_eq!(fact.identity.started_at_ms, 20_000);
        assert_eq!(fact.process_state, domain::ProcessState::Running);
    }

    #[test]
    fn codex_file_scan_uses_metadata_id_instead_of_filename_stem() {
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/codex/current-turn.jsonl");

        let scan = session_scan_from_file("codex", &fixture).unwrap();

        assert_eq!(scan.legacy_sessions[0].id, "codex-session-real");
        assert_eq!(scan.candidates[0].identity.session_id, "codex-session-real");
        assert_eq!(scan.candidates[0].view.id, "codex-session-real");
        assert!(scan.candidates[0]
            .events
            .iter()
            .all(|event| event.session_id == "codex-session-real"));
    }

    #[test]
    fn codex_long_file_keeps_head_metadata_and_tail_events_without_duplicates() {
        let path = std::env::temp_dir().join(format!(
            "agent-island-codex-{}-different-stem.jsonl",
            std::process::id()
        ));
        let text = format!(
            concat!(
                "{{\"timestamp\":\"2026-09-04T10:00:00Z\",\"type\":\"session_meta\",\"payload\":{{\"id\":\"canonical-long\",\"cwd\":\"D:\\\\work\\\\active\"}}}}\n",
                "{{\"timestamp\":\"2026-09-04T10:00:01Z\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"user_message\",\"message\":\"head prompt\",\"turn_id\":\"head-turn\"}}}}\n",
                "{}\n",
                "{{\"timestamp\":\"2026-09-04T10:01:00Z\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"task_started\",\"turn_id\":\"tail-turn\"}}}}\n",
                "{{\"timestamp\":\"2026-09-04T10:01:01Z\",\"type\":\"event_msg\",\"payload\":{{\"type\":\"user_message\",\"message\":\"tail prompt\",\"turn_id\":\"tail-turn\"}}}}\n",
                "{{\"timestamp\":\"2026-09-04T10:01:02Z\",\"type\":\"response_item\",\"payload\":{{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{{\"type\":\"output_text\",\"text\":\"tail answer\"}}]}}}}\n",
            ),
            "x".repeat(600 * 1024)
        );
        fs::write(&path, text).unwrap();

        let scan = session_scan_from_file("codex", &path).unwrap();

        assert_eq!(scan.candidates[0].identity.session_id, "canonical-long");
        assert_eq!(scan.candidates[0].events.len(), 3);
        assert!(scan.candidates[0]
            .events
            .iter()
            .all(|event| event.session_id == "canonical-long"));
        assert_eq!(
            scan.candidates[0]
                .view
                .records
                .iter()
                .map(|record| record.text.as_str())
                .collect::<Vec<_>>(),
            vec!["head prompt", "tail prompt", "tail answer"]
        );
        assert_eq!(
            scan.candidates[0].view.recent_output,
            vec!["head prompt", "tail prompt", "tail answer"]
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn production_codex_scan_serializes_canonical_messages_in_chronological_order() {
        let path = std::env::temp_dir().join(format!(
            "agent-island-codex-records-{}.jsonl",
            std::process::id()
        ));
        fs::write(
            &path,
            concat!(
                r#"{"timestamp":"2026-09-04T10:00:00Z","type":"session_meta","payload":{"id":"canonical-records","cwd":"D:\\work\\canonical"}}"#,
                "\n",
                r#"{"timestamp":"2026-09-04T10:00:01Z","type":"event_msg","payload":{"type":"user_message","message":"first prompt","turn_id":"turn-1"}}"#,
                "\n",
                r#"{"timestamp":"2026-09-04T10:00:02Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"first answer"}]}}"#,
                "\n",
                r#"{"timestamp":"2026-09-04T10:00:03Z","type":"event_msg","payload":{"type":"user_message","message":"second prompt","turn_id":"turn-2"}}"#,
                "\n",
            ),
        )
        .unwrap();

        let scan = session_scan_from_file("codex", &path).unwrap();
        let view = serde_json::to_value(&scan.candidates[0].view).unwrap();

        assert_eq!(
            view["records"]
                .as_array()
                .unwrap()
                .iter()
                .map(|record| record["text"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["first prompt", "first answer", "second prompt"]
        );
        assert_eq!(
            view["recent_output"],
            serde_json::json!(["first prompt", "first answer", "second prompt"])
        );
        assert_eq!(view["cwd"], r#"D:\work\canonical"#);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn production_scan_bounds_unicode_records_and_compatibility_output() {
        let path = std::env::temp_dir().join(format!(
            "agent-island-codex-record-budget-{}.jsonl",
            std::process::id()
        ));
        let mut text = String::from(
            r#"{"timestamp":"2026-09-04T10:00:00Z","type":"session_meta","payload":{"id":"bounded-records","cwd":"D:\\work\\bounded"}}"#,
        );
        text.push('\n');
        for index in 0..30 {
            text.push_str(&format!(
                r#"{{"timestamp":"2026-09-04T10:00:{index:02}Z","type":"event_msg","payload":{{"type":"user_message","message":"message-{index}-{}","turn_id":"turn-{index}"}}}}"#,
                "🦀".repeat(700),
            ));
            text.push('\n');
        }
        fs::write(&path, text).unwrap();

        let scan = session_scan_from_file("codex", &path).unwrap();
        let view = &scan.candidates[0].view;

        assert_eq!(view.records.len(), 24);
        assert!(view
            .records
            .iter()
            .all(|record| record.text.chars().count() <= 600));
        assert_eq!(view.records.first().unwrap().at_ms, view.records[0].at_ms);
        assert!(view
            .records
            .windows(2)
            .all(|pair| pair[0].at_ms <= pair[1].at_ms));
        assert_eq!(
            view.records.first().unwrap().text.starts_with("message-6-"),
            true
        );
        assert_eq!(view.recent_output.len(), 5);
        assert!(view
            .recent_output
            .iter()
            .all(|text| text.chars().count() <= 180));
        assert_eq!(
            view.recent_output,
            view.records
                .iter()
                .rev()
                .take(5)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .map(|record| record.text.chars().take(180).collect::<String>())
                .collect::<Vec<_>>()
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn claude_file_scan_uses_adapter_session_shape() {
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude/current-turn.jsonl");

        let scan = session_scan_from_file("claude", &fixture).unwrap();

        assert_eq!(scan.candidates[0].identity.agent_id, "claude");
        assert_eq!(scan.candidates[0].identity.session_id, "claude-s2");
        assert_eq!(scan.candidates[0].view.id, "claude-s2");
        assert_eq!(scan.candidates[0].events.len(), 2);
    }

    #[test]
    fn opencode_file_scan_uses_adapter_session_shape() {
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/opencode/text-error.log");

        let scan = session_scan_from_file("opencode", &fixture).unwrap();

        assert_eq!(scan.candidates[0].identity.agent_id, "opencode");
        assert_eq!(scan.candidates[0].identity.session_id, "opencode");
        assert_eq!(scan.candidates[0].view.id, "opencode");
        assert!(matches!(
            scan.candidates[0].events[0].kind,
            domain::EventKind::DiagnosticHint { .. }
        ));
    }

    #[test]
    fn hermes_raw_table_builds_a_matchable_snapshot_candidate() {
        let text = include_str!("../tests/fixtures/hermes/session-list.txt");

        let scan = hermes_session_scan_from_text(text);
        let candidate = &scan.candidates[0];
        let process = process_fact_from_observations(
            "hermes",
            "Hermes",
            &[ProcessObservation {
                pid: 77,
                project_path: Some("project-a".into()),
                started_at_ms: candidate.identity.last_event_at_ms.saturating_sub(1_000),
            }],
        );
        let snapshot = interface::snapshot::build_snapshot(
            candidate.identity.last_event_at_ms,
            &[process],
            &scan.candidates,
        );

        assert_eq!(scan.legacy_sessions[0].id, "hermes-1");
        assert_eq!(
            candidate.identity.project_path.as_deref(),
            Some("project-a")
        );
        assert_eq!(
            snapshot.agents[0].active_session.as_ref().unwrap().id,
            "hermes-1"
        );
    }

    #[test]
    fn legacy_projection_keeps_only_active_in_session_list() {
        use crate::domain::{AgentState, AttentionState, DisplayStatus, TurnState};
        use crate::interface::snapshot::{
            AgentView, AgentViewSnapshot, SessionSummary, SessionView,
        };

        let legacy_session = |id: &str| AgentSession {
            id: id.into(),
            name: id.into(),
            cwd: Some(format!(r"D:\work\{id}")),
            log_path: None,
            recent_output: Vec::new(),
            current_file: None,
            log_status: None,
            alert: None,
        };
        let mut agents = vec![AgentInfo {
            id: "codex".into(),
            name: "Codex CLI".into(),
            status: "working".into(),
            display_status: DisplayStatus::Idle,
            pid: Some(42),
            cpu: Some(1.0),
            memory: Some(2.0),
            uptime: Some(3),
            cwd: Some(r"D:\work\active".into()),
            sessions: 1,
            last_active_secs: Some(0),
            log_path: None,
            recent_output: Vec::new(),
            current_file: None,
            log_status: Some("working".into()),
            alert: None,
            can_restart: true,
            stats: None,
            session_count: 2,
            session_list: vec![legacy_session("matching-active"), legacy_session("older")],
            active_session: None,
            history_sessions: None,
            freshness: domain::Freshness {
                observed_at_ms: 10,
                stale: false,
            },
            usage: None,
        }];
        let active_session = SessionView {
            id: "matching-active".into(),
            name: "matching-active".into(),
            cwd: Some(r"D:\work\active".into()),
            log_path: None,
            records: Vec::new(),
            recent_output: Vec::new(),
            current_file: None,
            log_status: None,
            alert: None,
            lifecycle: domain::SessionLifecycle::Active,
            last_active_at_ms: 10,
            display_status: DisplayStatus::Working,
        };
        let snapshot = AgentViewSnapshot {
            schema_version: 1,
            generated_at_ms: 10,
            agents: vec![AgentView {
                id: "codex".into(),
                name: "Renamed Codex".into(),
                can_restart: false,
                state: AgentState::running(TurnState::Executing, AttentionState::None),
                display_status: DisplayStatus::Working,
                active_session: Some(active_session),
                active_turn: None,
                history_sessions: vec![SessionSummary {
                    id: "older".into(),
                    name: "older".into(),
                    cwd: Some(r"D:\work\older".into()),
                    log_path: None,
                    records: Vec::new(),
                    recent_output: Vec::new(),
                    current_file: None,
                    log_status: None,
                    alert: None,
                    lifecycle: domain::SessionLifecycle::Historical,
                    last_active_at_ms: 9,
                    display_status: DisplayStatus::Done,
                }],
                diagnostic: None,
                freshness: domain::Freshness {
                    observed_at_ms: 10,
                    stale: false,
                },
                usage: None,
            }],
        };

        apply_snapshot_projection(&mut agents, &snapshot);

        let json = serde_json::to_value(&agents[0]).unwrap();
        assert_eq!(json["id"], "codex");
        assert_eq!(json["name"], "Codex CLI");
        assert_eq!(json["status"], "working");
        assert_eq!(json["display_status"], "working");
        assert_eq!(json["session_list"].as_array().unwrap().len(), 1);
        assert_eq!(json["session_list"][0]["id"], "matching-active");
        assert_eq!(json["active_session"]["id"], "matching-active");
        assert_eq!(json["history_sessions"].as_array().unwrap().len(), 1);
        assert_eq!(json["history_sessions"][0]["id"], "older");
    }

    #[test]
    fn production_snapshot_reuses_usage_from_same_scan_by_stable_agent_id() {
        use crate::domain::{DisplayStatus, ProcessIdentity, ProcessState};
        use crate::interface::snapshot::{build_snapshot, ProcessActivity, ProcessFact};

        let usage = UsageInfo {
            tokens_total: 12_345,
            tokens_output: 2_345,
            cost_usd: Some(1.25),
            used_percent: Some(42.0),
            window_secs: 18_000,
            resets_at_secs: Some(50_000),
            credits: Some(8.75),
            unlimited: Some(false),
            stale: false,
        };
        let agents = vec![AgentInfo {
            id: "codex".into(),
            name: "Codex CLI".into(),
            status: "stopped".into(),
            display_status: DisplayStatus::Stopped,
            pid: None,
            cpu: None,
            memory: None,
            uptime: None,
            cwd: None,
            sessions: 0,
            last_active_secs: None,
            log_path: None,
            recent_output: Vec::new(),
            current_file: None,
            log_status: None,
            alert: None,
            can_restart: true,
            stats: None,
            session_count: 0,
            session_list: Vec::new(),
            active_session: None,
            history_sessions: None,
            freshness: domain::Freshness {
                observed_at_ms: 40_000,
                stale: false,
            },
            usage: Some(usage),
        }];
        let mut snapshot = build_snapshot(
            40_000,
            &[ProcessFact {
                name: "Display name may change".into(),
                identity: ProcessIdentity {
                    agent_id: "codex".into(),
                    project_path: None,
                    process_ids: Vec::new(),
                    started_at_ms: 0,
                },
                process_state: ProcessState::Stopped,
                activity: ProcessActivity::Unknown,
            }],
            &[],
        );

        attach_runtime_metadata_to_snapshot(&mut snapshot, &agents);
        let serialized = serde_json::to_value(&snapshot.agents[0]).unwrap();

        assert_eq!(serialized["usage"]["tokens_total"], 12_345);
        assert_eq!(serialized["usage"]["used_percent"], 42.0);
        assert_eq!(serialized["usage"]["stale"], false);
        assert_eq!(serialized["can_restart"], true);
    }

    #[test]
    fn compatibility_status_placeholder_uses_process_facts_only() {
        use domain::DisplayStatus;

        assert_eq!(
            legacy_process_status(0.0, 64.0, true),
            ("working", DisplayStatus::Working)
        );
        assert_eq!(
            legacy_process_status(95.0, 64.0, false),
            ("high_load", DisplayStatus::Working)
        );
        assert_eq!(
            legacy_process_status(0.0, 64.0, false),
            ("idle", DisplayStatus::Idle)
        );
    }

    #[test]
    fn stopped_production_scan_projection_serializes_typed_history_without_legacy_fallback() {
        use crate::domain::{DisplayStatus, ProcessIdentity, ProcessState};
        use crate::interface::snapshot::ProcessFact;

        let path = std::env::temp_dir().join(format!(
            "agent-island-stopped-history-{}.jsonl",
            std::process::id()
        ));
        fs::write(
            &path,
            concat!(
                r#"{"timestamp":"2026-09-04T10:00:00Z","type":"session_meta","payload":{"id":"stopped-history","cwd":"D:\\work\\history"}}"#,
                "\n",
                r#"{"timestamp":"2026-09-04T10:00:01Z","type":"event_msg","payload":{"type":"user_message","message":"saved prompt","turn_id":"turn-1"}}"#,
                "\n",
                r#"{"timestamp":"2026-09-04T10:00:02Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"saved answer"}]}}"#,
                "\n",
                r#"{"timestamp":"2026-09-04T10:00:03Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-1"}}"#,
                "\n",
            ),
        )
        .unwrap();
        let scan = session_scan_from_file("codex", &path).unwrap();
        let snapshot = interface::snapshot::build_snapshot(
            scan.candidates[0]
                .identity
                .last_event_at_ms
                .saturating_add(1),
            &[ProcessFact {
                name: "Codex CLI".into(),
                identity: ProcessIdentity {
                    agent_id: "codex".into(),
                    project_path: None,
                    process_ids: Vec::new(),
                    started_at_ms: 0,
                },
                process_state: ProcessState::Stopped,
                activity: interface::snapshot::ProcessActivity::Unknown,
            }],
            &scan.candidates,
        );
        let mut agents = vec![AgentInfo {
            id: "codex".into(),
            name: "Codex CLI".into(),
            status: "stopped".into(),
            display_status: DisplayStatus::Stopped,
            pid: None,
            cpu: None,
            memory: None,
            uptime: None,
            cwd: None,
            sessions: 0,
            last_active_secs: None,
            log_path: None,
            recent_output: Vec::new(),
            current_file: None,
            log_status: None,
            alert: None,
            can_restart: false,
            stats: None,
            session_count: 1,
            session_list: scan.legacy_sessions,
            active_session: None,
            history_sessions: None,
            freshness: domain::Freshness {
                observed_at_ms: snapshot.generated_at_ms,
                stale: false,
            },
            usage: None,
        }];

        apply_snapshot_projection(&mut agents, &snapshot);
        let agent = serde_json::to_value(&agents[0]).unwrap();

        assert!(agent["active_session"].is_null());
        assert_eq!(agent["session_list"], serde_json::json!([]));
        assert_eq!(agent["history_sessions"][0]["display_status"], "done");
        assert_eq!(
            agent["history_sessions"][0]["records"]
                .as_array()
                .unwrap()
                .iter()
                .map(|record| record["text"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["saved prompt", "saved answer"]
        );
        assert_eq!(agent["history_sessions"][0]["lifecycle"], "Historical");
        assert_eq!(
            agent["history_sessions"][0]["last_active_at_ms"],
            snapshot.generated_at_ms.saturating_sub(1)
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn default_defs_cover_core_agents() {
        let defs = default_agent_defs();
        for name in ["Claude Code", "Codex CLI", "OpenCode", "Hermes"] {
            assert!(defs.iter().any(|d| d.name == name), "missing {name}");
        }
    }

    #[test]
    fn parse_rfc3339_handles_z_and_offsets() {
        assert_eq!(
            parse_rfc3339_secs("2026-08-18T11:10:42.166Z"),
            Some(1787051442)
        );
        assert_eq!(parse_rfc3339_secs("not-a-date"), None);
    }

    #[test]
    fn claude_scan_sums_window_tokens() {
        let window_start = parse_rfc3339_secs("2026-08-18T10:00:00Z").unwrap();
        let lines = "\
{\"timestamp\":\"2026-08-18T09:00:00Z\",\"message\":{\"type\":\"message\",\"role\":\"assistant\",\"model\":\"claude-sonnet-4-5\",\"usage\":{\"input_tokens\":9999,\"cache_creation_input_tokens\":0,\"cache_read_input_tokens\":0,\"output_tokens\":9999}},\"type\":\"assistant\"}\n\
{\"timestamp\":\"2026-08-18T11:10:42.166Z\",\"message\":{\"type\":\"message\",\"role\":\"assistant\",\"model\":\"claude-sonnet-4-5\",\"usage\":{\"input_tokens\":1000,\"cache_creation_input_tokens\":0,\"cache_read_input_tokens\":2000,\"output_tokens\":500}},\"type\":\"assistant\"}\n\
{\"timestamp\":\"2026-08-18T11:20:00Z\",\"message\":{\"type\":\"message\",\"role\":\"user\"},\"type\":\"user\"}\n";
        let scan = scan_claude_text(lines, window_start);
        assert!(scan.found);
        assert_eq!(scan.input, 1000);
        assert_eq!(scan.output, 500);
        assert_eq!(scan.cache_read, 2000);
        assert!(scan.cost_known);
        assert!(scan.cost > 0.0);
    }

    #[test]
    fn claude_scan_ignores_unknown_model_cost() {
        let window_start = parse_rfc3339_secs("2026-08-18T10:00:00Z").unwrap();
        let lines = "{\"timestamp\":\"2026-08-18T11:10:42.166Z\",\"message\":{\"type\":\"message\",\"role\":\"assistant\",\"model\":\"deepseek-v4-pro\",\"usage\":{\"input_tokens\":10,\"cache_creation_input_tokens\":0,\"cache_read_input_tokens\":0,\"output_tokens\":5}},\"type\":\"assistant\"}\n";
        let scan = scan_claude_text(lines, window_start);
        assert!(scan.found);
        assert_eq!(scan.input, 10);
        assert!(!scan.cost_known);
    }

    #[test]
    fn codex_scan_reads_token_count_and_rate_limits() {
        let now = 1787056242u64;
        let line = "{\"timestamp\":\"2026-08-18T11:10:42.980Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":6313264,\"output_tokens\":24262,\"total_tokens\":6337526},\"model_context_window\":258400},\"rate_limits\":{\"limit_id\":\"codex\",\"primary\":{\"used_percent\":96.0,\"window_minutes\":43200,\"resets_at\":1789130031},\"secondary\":null,\"credits\":{\"has_credits\":false,\"unlimited\":true,\"balance\":null}}}}\n";
        let info = scan_codex_text(line, now).expect("should parse");
        assert_eq!(info.tokens_total, 6337526);
        assert!((info.used_percent.unwrap() - 96.0).abs() < 0.01);
        assert_eq!(info.resets_at_secs, Some(1789130031));
        assert_eq!(info.unlimited, Some(true));
        assert_eq!(info.credits, None);
        assert!(!info.stale);
    }

    #[test]
    fn codex_scan_marks_reset_window_as_stale() {
        let now = 1789130031u64;
        let line = "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}},\"rate_limits\":{\"primary\":{\"used_percent\":96.0,\"window_minutes\":43200,\"resets_at\":1789130000},\"credits\":{\"has_credits\":false,\"unlimited\":false,\"balance\":null}}}}\n";
        let info = scan_codex_text(line, now).expect("should parse");
        assert!(info.stale);
        assert_eq!(info.used_percent, Some(0.0));
    }

    #[test]
    fn classify_allows_read_only_commands() {
        assert!(classify_command("ls -la"));
        assert!(classify_command("Get-Content foo.txt"));
        assert!(classify_command("git status"));
        assert!(classify_command("rg -n TODO src"));
        assert!(classify_command("node --version"));
        assert!(classify_command(
            "\"C:\\Program Files\\Git\\bin\\git.exe\" status"
        ));
    }

    #[test]
    fn classify_blocks_dangerous_commands() {
        assert!(!classify_command("rm -rf build"));
        assert!(!classify_command("npm install"));
        assert!(!classify_command("git push origin main"));
        assert!(!classify_command("del /f /q secret.txt"));
        assert!(!classify_command("cd .. && del *"));
        assert!(!classify_command("ls | wc -l"));
    }

    #[test]
    fn hooks_merge_preserves_user_settings_and_secrets() {
        let dir = std::env::temp_dir().join(format!("agent-island-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let settings = dir.join("settings.json");
        let script = dir.join("hook-bridge.mjs");
        fs::write(
            &settings,
            r#"{
  "env": { "ANTHROPIC_API_KEY": "sk-secret" },
  "model": "sonnet"
}"#,
        )
        .unwrap();
        merge_hooks_settings(&settings, &script).unwrap();
        let text = fs::read_to_string(&settings).unwrap();
        assert!(text.contains("sk-secret"));
        assert!(text.contains("PreToolUse"));
        assert!(text.contains("hook-bridge.mjs"));

        remove_hooks_settings(&settings).unwrap();
        let text = fs::read_to_string(&settings).unwrap();
        assert!(text.contains("sk-secret"));
        assert!(!text.contains("PreToolUse"));
        assert!(!text.contains("hook-bridge.mjs"));
        fs::remove_dir_all(&dir).ok();
    }
}

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
                            loop {
                                std::thread::sleep(std::time::Duration::from_millis(150));
                                unsafe {
                                    let mut rc: RECT = std::mem::zeroed();
                                    if GetWindowRect(hwnd2, &mut rc).is_err() {
                                        continue;
                                    }
                                    if rc.top != 0 {
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
