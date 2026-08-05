use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use sysinfo::{ProcessesToUpdate, System};
use tauri::menu::{CheckMenuItem, Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::Manager;
use serde_json::Value;

#[derive(Debug, Clone, Serialize)]
pub struct AgentInfo {
    pub name: String,
    pub status: String,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentStats {
    pub total_seconds: u64,
    pub error_count: u32,
    pub done_count: u32,
    pub last_status: Option<String>,
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

#[derive(Debug, Clone)]
struct AgentCommand {
    cwd: Option<String>,
    cmd: Vec<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
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

static AGENT_DEFS: OnceLock<Vec<AgentDef>> = OnceLock::new();

fn default_agent_defs() -> Vec<AgentDef> {
    vec![
        AgentDef {
            name: "Claude Code".into(),
            keyword: "claude".into(),
            log_kind: "claude".into(),
            resume_args: vec![
                "{exe}".into(), "--resume".into(), "{session}".into(), "继续".into(),
            ],
            send_args: vec![
                "{exe}".into(), "-p".into(), "{prompt}".into(),
                "--resume".into(), "{session}".into(),
            ],
        },
        AgentDef {
            name: "Codex CLI".into(),
            keyword: "codex".into(),
            log_kind: "codex".into(),
            resume_args: vec![
                "{exe}".into(), "exec".into(), "resume".into(), "{session}".into(),
            ],
            send_args: vec![
                "{exe}".into(), "exec".into(), "resume".into(),
                "{session}".into(), "{prompt}".into(),
            ],
        },
        AgentDef {
            name: "OpenCode".into(),
            keyword: "opencode".into(),
            log_kind: "opencode".into(),
            resume_args: vec![
                "{exe}".into(), "run".into(), "-s".into(), "{session}".into(),
            ],
            send_args: vec![
                "{exe}".into(), "run".into(), "-s".into(),
                "{session}".into(), "{prompt}".into(),
            ],
        },
        AgentDef {
            name: "Hermes".into(),
            keyword: "hermes".into(),
            log_kind: "hermes".into(),
            resume_args: vec![
                "{exe}".into(), "--resume".into(), "{session}".into(),
            ],
            send_args: vec![
                "{exe}".into(), "--resume".into(), "{session}".into(),
                "-z".into(), "{prompt}".into(),
            ],
        },
        AgentDef {
            name: "Copilot".into(),
            keyword: "copilot".into(),
            log_kind: "copilot".into(),
            resume_args: vec![],
            send_args: vec![],
        },
        AgentDef {
            name: "Cursor".into(),
            keyword: "cursor".into(),
            log_kind: String::new(),
            resume_args: vec![],
            send_args: vec![],
        },
    ]
}

fn agent_defs() -> &'static Vec<AgentDef> {
    AGENT_DEFS.get_or_init(|| {
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
    })
}

fn fill_template(template: &[String], exe: &str, session: &str, prompt: Option<&str>) -> Vec<String> {
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
    cache: Option<(Instant, Vec<AgentInfo>)>,
    last_save: Instant,
}

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
            let cmd = process
                .cmd()
                .iter()
                .map(|s| s.to_string_lossy().to_lowercase())
                .collect::<Vec<_>>()
                .join(" ");
            name.contains(keyword) || cmd.contains(keyword)
        })
        .map(|(_pid, process)| process)
        .collect()
}

fn scan_agents(sys: &mut System, session: &mut SessionState) -> (Vec<AgentInfo>, bool) {
    sys.refresh_processes(ProcessesToUpdate::All, true);
    let now = Instant::now();
    let delta = now.duration_since(session.last_poll).as_secs();
    session.last_poll = now;
    let mut agents: Vec<AgentInfo> = Vec::new();
    let mut stats_changed = false;

    for def in agent_defs() {
        let agent_name = &def.name;
        let processes = matching_processes(sys, &def.keyword);
        let session_list = build_sessions(agent_name);
        if processes.is_empty() {
            session.activity.remove(agent_name);
            session.runtime_start.remove(agent_name);
            let first = session_list.first();
            agents.push(AgentInfo {
                name: agent_name.clone(),
                status: "stopped".to_string(),
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
        let cwd = main
            .and_then(|p| p.cwd())
            .map(|path| path.to_string_lossy().into_owned())
            .or_else(|| {
                processes
                    .iter()
                    .find_map(|p| p.cwd().map(|path| path.to_string_lossy().into_owned()))
            })
            .or_else(|| log.as_ref().and_then(|l| l.cwd.clone()));

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
        let idle_secs = now.duration_since(entry.last_active).as_secs();
        let log_status = log.as_ref().and_then(|l| l.log_status.clone());
        let alert = log.as_ref().and_then(|l| l.alert.clone());
        let status = if log_status.as_deref() == Some("error") {
            "error"
        } else if log_status.as_deref() == Some("waiting") {
            "waiting"
        } else if busy {
            "working"
        } else if log_status.as_deref() == Some("done") {
            "done"
        } else if total_cpu > 80.0 || total_mem_mb > 1024.0 {
            "high_load"
        } else {
            "idle"
        };

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
        if status == "error" && stats.last_status.as_deref() != Some("error") {
            stats.error_count += 1;
            day_stats.error_count += 1;
            stats_changed = true;
        }
        if status == "done" && stats.last_status.as_deref() != Some("done") {
            stats.done_count += 1;
            day_stats.done_count += 1;
            stats_changed = true;
        }
        stats.last_status = Some(status.to_string());
        let stats_snapshot = stats.clone();

        agents.push(AgentInfo {
            name: agent_name.clone(),
            status: status.to_string(),
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
        });
    }
    (agents, stats_changed)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
}

fn stats_path() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".agent-island")
        .join("stats.json")
}

fn load_stats(path: &Path) -> HashMap<String, AgentStats> {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
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
        let token = raw.trim_matches(|c: char| c == '"' || c == '\'' || c == '`' || c == ',' || c == ')' || c == ']');
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

fn text_signal(text: &str) -> Option<(&'static str, &'static str)> {
    let lower = text.to_lowercase();
    const ERRORS: &[&str] = &[
        "error:", "error occurred", "failed to", "exception", "panic", "traceback", "is_error",
        "报错:", "报错：", "失败:", "失败：", "出错:", "出错：",
    ];
    const WAITING: &[&str] = &[
        "waiting for", "awaiting", "permission required", "approval", "confirm", "确认", "是否继续", "y/n", "yes/no", "需要你", "请确认",
    ];
    const DONE: &[&str] = &[
        "completed", "finished", "successfully", "success", "summary", "完成", "成功", "结束",
    ];
    if ERRORS.iter().any(|k| lower.contains(k)) {
        return Some(("error", "检测到报错"));
    }
    if WAITING.iter().any(|k| lower.contains(k)) {
        return Some(("waiting", "等待确认"));
    }
    if DONE.iter().any(|k| lower.contains(k)) && !lower.contains("not done") && !lower.contains("undone") {
        return Some(("done", "已完成"));
    }
    None
}

fn claude_snapshot(path: &Path) -> Option<LogSnapshot> {
    let text = read_tail(path, 96 * 1024);
    let mut recent = Vec::new();
    let mut file = None;
    let mut cwd = None;
    let mut log_status = None;
    let mut alert = None;

    for line in text.lines().rev() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if cwd.is_none() {
            cwd = v.get("cwd").and_then(|c| c.as_str()).map(String::from);
        }
        if log_status.is_none() && line.contains("subtype\":\"error\"") {
            log_status = Some("error".to_string());
            alert = Some("检测到报错".to_string());
            break;
        }
        if v.get("type").and_then(|t| t.as_str()) == Some("assistant") {
            if let Some(content) = json_text(&v) {
                let clean = clean_line(&content, 180);
                if !clean.is_empty() && !recent.contains(&clean) {
                    recent.push(clean);
                }
                if file.is_none() {
                    file = extract_paths(&content).into_iter().rev().next();
                }
                if log_status.is_none() {
                    let stop_reason = v
                        .pointer("/message/stop_reason")
                        .and_then(|s| s.as_str())
                        .unwrap_or("");
                    if stop_reason == "end_turn" {
                        log_status = Some("done".to_string());
                        alert = Some("已完成".to_string());
                    } else {
                        log_status = Some("working".to_string());
                        alert = Some("正在执行".to_string());
                    }
                }
            }
        }
        if recent.len() >= 5 {
            break;
        }
    }
    if recent.is_empty() {
        recent.push("暂无输出".to_string());
    }
    Some(LogSnapshot {
        path: path.display().to_string(),
        recent,
        file,
        cwd,
        log_status,
        alert,
    })
}

fn codex_snapshot(path: &Path) -> Option<LogSnapshot> {
    let text = read_tail(path, 128 * 1024);
    let mut recent = Vec::new();
    let mut file = None;
    let mut cwd = None;
    let mut log_status = None;
    let mut alert = None;

    for line in text.lines().rev() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if cwd.is_none() {
            cwd = v
                .pointer("/payload/cwd")
                .and_then(|c| c.as_str())
                .or_else(|| {
                    v.pointer("/payload/environments/environments/local/cwd")
                        .and_then(|c| c.as_str())
                })
                .map(String::from);
        }
        let payload = v.get("payload");
        let ptype = payload
            .and_then(|p| p.get("type"))
            .and_then(|t| t.as_str())
            .unwrap_or("");
        if ptype == "function_call" {
            let args = payload
                .and_then(|p| p.get("arguments"))
                .and_then(|a| a.as_str())
                .unwrap_or("");
            let command = serde_json::from_str::<Value>(args)
                .ok()
                .and_then(|a| a.get("command").and_then(|c| c.as_str()).map(String::from))
                .unwrap_or_else(|| args.to_string());
            let clean = clean_line(&format!("执行: {command}"), 180);
            if !clean.is_empty() && !recent.contains(&clean) {
                recent.push(clean);
            }
            if file.is_none() {
                file = extract_paths(&command).into_iter().rev().next();
            }
            if log_status.is_none() {
                log_status = Some("working".to_string());
                alert = Some("正在执行".to_string());
            }
        } else if ptype == "function_call_output" {
            let output = payload
                .and_then(|p| p.get("output"))
                .and_then(|o| o.as_str())
                .unwrap_or("");
            if log_status.is_none() {
                let clean = clean_line(output, 180);
                if !clean.is_empty() && !recent.contains(&clean) {
                    recent.push(clean);
                }
                log_status = Some("working".to_string());
                alert = Some("正在执行".to_string());
            }
        } else if ptype == "message" {
            let role = payload
                .and_then(|p| p.get("role"))
                .and_then(|r| r.as_str())
                .unwrap_or("");
            if role == "assistant" {
                if let Some(content) = payload.and_then(json_text) {
                    let clean = clean_line(&content, 180);
                    if !clean.is_empty() && !recent.contains(&clean) {
                        recent.push(clean);
                    }
                    if file.is_none() {
                        file = extract_paths(&content).into_iter().rev().next();
                    }
                    if log_status.is_none() {
                        log_status = Some("working".to_string());
                        alert = Some("正在执行".to_string());
                    }
                }
            }
        } else if ptype == "event_msg" {
            let inner_type = payload
                .and_then(|p| p.get("type"))
                .and_then(|t| t.as_str())
                .unwrap_or("");
            if inner_type == "user_message" {
                continue;
            }
            if log_status.is_none() {
                if inner_type.contains("error") {
                    log_status = Some("error".to_string());
                    alert = Some("检测到报错".to_string());
                    break;
                }
                if inner_type == "permission_request"
                    || inner_type.contains("permission")
                    || inner_type.contains("approval")
                {
                    log_status = Some("waiting".to_string());
                    alert = Some("等待确认".to_string());
                    break;
                }
                if inner_type == "turn_complete" || inner_type == "turn_completed" {
                    log_status = Some("done".to_string());
                    alert = Some("已完成".to_string());
                    break;
                }
            }
        }
        if recent.len() >= 5 {
            break;
        }
    }
    if recent.is_empty() {
        recent.push("暂无输出".to_string());
    }
    Some(LogSnapshot {
        path: path.display().to_string(),
        recent,
        file,
        cwd,
        log_status,
        alert,
    })
}

fn read_opencode_log() -> Option<LogSnapshot> {
    let root = home_dir()?.join(".local").join("share").join("opencode").join("log");
    let path = root.join("opencode.log");
    if !path.is_file() {
        return None;
    }
    let text = read_tail(&path, 32 * 1024);
    let mut recent = Vec::new();
    let mut file = None;
    let mut cwd = None;
    let mut log_status = None;
    let mut alert = None;

    for line in text.lines().rev() {
        if cwd.is_none() {
            if let Some(idx) = line.find("directory=") {
                let rest = line[idx + 10..].trim();
                let val = if rest.starts_with('"') {
                    rest.trim_start_matches('"').split('"').next().unwrap_or("").to_string()
                } else {
                    rest.split_whitespace().next().unwrap_or("").to_string()
                };
                if !val.is_empty() {
                    cwd = Some(val);
                }
            }
        }
        if log_status.is_none() && line.contains("level=ERROR") {
            log_status = Some("error".to_string());
            alert = Some("检测到报错".to_string());
            break;
        }
        let msg = line.rsplit("message=").next().unwrap_or(line);
        let clean = clean_line(msg, 160);
        let noisy = ["init", "cleanup", "formatter", "lsp", "watcher backend", "location services"]
            .iter()
            .any(|n| clean.to_lowercase().contains(n));
        if !clean.is_empty() && !noisy && !recent.contains(&clean) {
            recent.push(clean);
        }
        if file.is_none() {
            file = extract_paths(line).into_iter().rev().next();
        }
        if log_status.is_none() {
            if let Some((status, alert_text)) = text_signal(msg) {
                log_status = Some(status.to_string());
                alert = Some(alert_text.to_string());
            }
        }
        if recent.len() >= 5 {
            break;
        }
    }
    if recent.is_empty() {
        recent.push("暂无输出".to_string());
    }
    Some(LogSnapshot {
        path: path.display().to_string(),
        recent,
        file,
        cwd,
        log_status,
        alert,
    })
}

fn copilot_snapshot(path: &Path) -> Option<LogSnapshot> {
    let text = read_tail(path, 12 * 1024);
    let mut recent = Vec::new();
    for line in text.lines().rev() {
        let clean = clean_line(line, 160);
        if !clean.is_empty() && !recent.contains(&clean) {
            recent.push(clean);
        }
        if recent.len() >= 5 {
            break;
        }
    }
    if recent.is_empty() {
        recent.push("暂无输出".to_string());
    }
    Some(LogSnapshot {
        path: path.display().to_string(),
        recent,
        file: None,
        cwd: None,
        log_status: None,
        alert: None,
    })
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

fn to_session(path: &Path, snap: LogSnapshot) -> AgentSession {
    let id = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
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

fn claude_sessions() -> Vec<AgentSession> {
    let Some(root) = home_dir().map(|h| h.join(".claude").join("projects")) else {
        return Vec::new();
    };
    list_newest_files(&root, 4, "jsonl", 3)
        .into_iter()
        .filter_map(|path| claude_snapshot(&path).map(|snap| to_session(&path, snap)))
        .collect()
}

fn codex_sessions() -> Vec<AgentSession> {
    let Some(root) = home_dir().map(|h| h.join(".codex").join("sessions")) else {
        return Vec::new();
    };
    list_newest_files(&root, 5, "jsonl", 3)
        .into_iter()
        .filter_map(|path| codex_snapshot(&path).map(|snap| to_session(&path, snap)))
        .collect()
}

fn copilot_sessions() -> Vec<AgentSession> {
    let Some(root) = home_dir().map(|h| h.join(".copilot").join("logs")) else {
        return Vec::new();
    };
    list_newest_files(&root, 2, "log", 3)
        .into_iter()
        .filter_map(|path| copilot_snapshot(&path).map(|snap| to_session(&path, snap)))
        .collect()
}

fn opencode_sessions() -> Vec<AgentSession> {
    read_opencode_log()
        .map(|snap| {
            let path = snap.path.clone();
            to_session(Path::new(&path), snap)
        })
        .into_iter()
        .collect()
}

static HERMES_CACHE: OnceLock<Mutex<Option<(Instant, Vec<AgentSession>)>>> = OnceLock::new();

fn run_command_timeout(
    command: &mut std::process::Command,
    secs: u64,
) -> Option<std::process::Output> {
    use std::sync::mpsc;
    let child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let shared = Arc::new(Mutex::new(child));
    let worker = shared.clone();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let status = worker.lock().unwrap().wait();
        let _ = tx.send(status);
    });
    match rx.recv_timeout(std::time::Duration::from_secs(secs)) {
        Ok(Ok(status)) => {
            let mut guard = shared.lock().unwrap();
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            if let Some(mut pipe) = guard.stdout.take() {
                let _ = pipe.read_to_end(&mut stdout);
            }
            if let Some(mut pipe) = guard.stderr.take() {
                let _ = pipe.read_to_end(&mut stderr);
            }
            Some(std::process::Output {
                status,
                stdout,
                stderr,
            })
        }
        _ => {
            let _ = shared.lock().unwrap().kill();
            None
        }
    }
}

fn parse_hermes_sessions() -> Vec<AgentSession> {
    let mut command = quiet_command("hermes");
    command.args(["sessions", "list"]);
    let Some(output) = run_command_timeout(&mut command, 8) else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut sessions = Vec::new();
    for line in text.lines().skip(2) {
        let line = line.trim();
        if line.is_empty()
            || line.chars().all(|c| c == '─' || c == '-' || c == '—' || c == ' ')
        {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }
        let id = parts.last().unwrap_or(&"").to_string();
        let name = parts[0].trim_matches('"').to_string();
        if name.is_empty() || name == "—" || name == "-" {
            continue;
        }
        sessions.push(AgentSession {
            id,
            name: name.clone(),
            cwd: None,
            log_path: None,
            recent_output: vec![name.clone()],
            current_file: None,
            log_status: None,
            alert: None,
        });
        if sessions.len() >= 10 {
            break;
        }
    }
    sessions
}

fn hermes_sessions() -> Vec<AgentSession> {
    let cache = HERMES_CACHE.get_or_init(|| Mutex::new(None));
    if let Ok(guard) = cache.lock() {
        if let Some((at, sessions)) = guard.as_ref() {
            if at.elapsed().as_secs() < 60 {
                return sessions.clone();
            }
        }
    }
    let sessions = parse_hermes_sessions();
    if let Ok(mut guard) = cache.lock() {
        *guard = Some((Instant::now(), sessions.clone()));
    }
    sessions
}

fn build_sessions(name: &str) -> Vec<AgentSession> {
    let kind = agent_defs()
        .iter()
        .find(|d| d.name == name)
        .map(|d| d.log_kind.as_str())
        .unwrap_or("");
    match kind {
        "claude" => claude_sessions(),
        "codex" => codex_sessions(),
        "opencode" => opencode_sessions(),
        "copilot" => copilot_sessions(),
        "hermes" => hermes_sessions(),
        _ => Vec::new(),
    }
}

#[tauri::command]
fn get_agents(state: tauri::State<AppState>) -> Vec<AgentInfo> {
    let mut sys = state.sys.lock().unwrap();
    let mut session = state.session.lock().unwrap();
    if let Some((cached_at, cached)) = &session.cache {
        if cached_at.elapsed().as_millis() < 500 {
            return cached.clone();
        }
    }
    let now = Instant::now();
    let (agents, changed) = scan_agents(&mut sys, &mut session);
    session.cache = Some((now, agents.clone()));
    if changed || now.duration_since(session.last_save).as_secs() >= 30 {
        let _ = save_stats(&session.stats, &state.stats_path);
        let _ = save_daily(&session.daily, &state.daily_path);
        session.last_save = now;
    }
    agents
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
    let mut sys = state.sys.lock().map_err(|_| "state lock error".to_string())?;
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
    let mut sys = state.sys.lock().map_err(|_| "state lock error".to_string())?;
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

fn spawn_command_in_dir(cmd: &[String], cwd: Option<&str>) -> Result<(), String> {
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
        builder.arg("/K").arg(&cmdline).creation_flags(CREATE_NEW_CONSOLE);
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
                return Ok(fill_template(&def.send_args, &exe, session_id, Some(prompt)));
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

fn fallback_send_command(name: &str, session_id: &str, prompt: &str) -> Result<Vec<String>, String> {
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
    let mut sys = state.sys.lock().map_err(|_| "state lock error".to_string())?;
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
    let session = state.session.lock().map_err(|_| "state lock error".to_string())?;
    let command = session
        .commands
        .get(&name)
        .ok_or_else(|| "no command recorded for this agent".to_string())?;
    let base = clean_agent_base(&name, &command.cmd);
    let cmdline = base
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
        builder.arg("/K").arg(&cmdline).creation_flags(CREATE_NEW_CONSOLE);
        if let Some(cwd) = &command.cwd {
            builder.current_dir(cwd);
        }
        builder
            .spawn()
            .map_err(|e| format!("failed to restart agent: {e}"))?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = cmdline;
        return Err("not supported on this platform".into());
    }
    Ok(())
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
    let session = state.session.lock().map_err(|_| "state lock error".to_string())?;
    let command = session
        .commands
        .get(&name)
        .ok_or_else(|| "no command recorded for this agent".to_string())?;
    let base = clean_agent_base(&name, &command.cmd);
    let resume_cmd = resume_command_for(&name, &session_id, &base);
    spawn_command_in_dir(&resume_cmd, Some(&dir))
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
        let session = state.session.lock().map_err(|_| "state lock error".to_string())?;
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
        let dir = dir.or_else(|| {
            session
                .commands
                .get(&name)
                .and_then(|c| c.cwd.clone())
        });
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
    let mut sys = state.sys.lock().map_err(|_| "state lock error".to_string())?;
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
                "add", key, "/v", AUTOSTART_NAME, "/t", "REG_SZ", "/d", &value, "/f",
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
    if let Some(window) = app.get_webview_window("overview") {
        let _ = window.show();
        let _ = window.set_focus();
    }
    Ok(())
}

const REMOTE_PORT: u16 = 8765;

const REMOTE_HTML: &str = r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Agent Island Remote</title>
<style>
body{background:#121214;color:#f2f2f5;font-family:"Segoe UI","Microsoft YaHei",sans-serif;margin:0;padding:24px}
h1{font-size:18px;margin:0 0 16px}
.card{background:#1c1c1f;border:1px solid #2a2a2e;border-radius:10px;padding:12px 14px;margin-bottom:10px}
.row{display:flex;align-items:center;gap:10px}
.dot{width:9px;height:9px;border-radius:50%}
.green{background:#30d158}.yellow{background:#ffd60a}.red{background:#ff453a}.gray{background:#636366}
.name{font-weight:600}.cwd{color:#9a9aa0;font-size:12px;margin-top:4px;word-break:break-all}
.out{color:#c8c8cc;font-size:12px;margin-top:6px;white-space:pre-wrap;max-height:64px;overflow:hidden}
.meta{color:#8e8e93;font-size:11px;margin-top:4px}
</style>
</head>
<body>
<h1>Agent Island</h1>
<div id="list">加载中...</div>
<script>
async function load(){
  try{
    const r=await fetch('/api/agents');
    const data=await r.json();
    const colors={working:'green',running:'green',done:'green',idle:'yellow',waiting:'yellow',high_load:'yellow',stopped:'red',error:'red'};
    document.getElementById('list').innerHTML=data.map(a=>{
      const c=colors[a.status]||'gray';
      const out=(a.recent_output||[]).slice(-3).join('\n');
      return '<div class="card"><div class="row"><span class="dot '+c+'"></span><span class="name"></span><span class="meta"></span></div><div class="cwd"></div><div class="out"></div></div>';
    }).join('');
    const nodes=document.querySelectorAll('.card');
    nodes.forEach((n,i)=>{
      const a=data[i]; if(!a)return;
      n.querySelector('.name').textContent=a.name+' · '+(a.status||'-');
      n.querySelector('.meta').textContent='CPU '+(a.cpu!=null?a.cpu.toFixed(1)+'%':'-')+' · 内存 '+(a.memory!=null?a.memory.toFixed(0)+' MB':'-')+' · '+(a.session_count||0)+' 会话';
      n.querySelector('.cwd').textContent=a.cwd||'-';
      n.querySelector('.out').textContent=(a.recent_output||[]).slice(-3).join('\n')||'暂无日志';
    });
  }catch(e){
    document.getElementById('list').textContent='无法连接 Agent Island';
  }
}
load();
setInterval(load,3000);
</script>
</body>
</html>"#;

fn local_ipv4() -> Option<String> {
    use std::net::UdpSocket;
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let ip = socket.local_addr().ok()?.ip();
    if ip.is_unspecified() || ip.is_loopback() {
        None
    } else {
        Some(ip.to_string())
    }
}

#[tauri::command]
fn get_remote_url() -> String {
    format!(
        "http://{}:{REMOTE_PORT}",
        local_ipv4().unwrap_or_else(|| "localhost".to_string())
    )
}

fn handle_remote(mut stream: std::net::TcpStream, app: tauri::AppHandle) {
    use std::io::{Read, Write};
    let mut buf = [0u8; 8192];
    let n = match stream.read(&mut buf) {
        Ok(n) => n,
        Err(_) => return,
    };
    let req = String::from_utf8_lossy(&buf[..n]);
    let path = req
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .unwrap_or("/");
    let (status, content_type, body) = if path == "/api/agents" {
        let state = app.state::<AppState>();
        let agents = get_agents(state);
        let json = serde_json::to_string(&agents).unwrap_or_else(|_| "[]".to_string());
        ("200 OK", "application/json; charset=utf-8", json)
    } else if path == "/" || path.starts_with("/?") {
        (
            "200 OK",
            "text/html; charset=utf-8",
            REMOTE_HTML.to_string(),
        )
    } else {
        (
            "404 Not Found",
            "text/plain; charset=utf-8",
            "not found".to_string(),
        )
    };
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.flush();
}

fn start_remote_server(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let listener = match std::net::TcpListener::bind(("0.0.0.0", REMOTE_PORT)) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("remote server failed to bind: {e}");
                return;
            }
        };
        for stream in listener.incoming().flatten() {
            let app = app.clone();
            std::thread::spawn(move || handle_remote(stream, app));
        }
    });
}

pub fn run() {
    let stats_path = stats_path();
    let daily_path = daily_path();
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
        })
        .invoke_handler(tauri::generate_handler![
            get_agents,
            get_stats_report,
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
            get_remote_url,
            open_overview
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

            start_remote_server(app.handle().clone());
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
