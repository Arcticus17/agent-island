//! Process matching, runtime facts, and status projection onto snapshots.

use crate::*;

pub(crate) fn keyword_for(name: &str) -> Option<String> {
    agent_defs()
        .iter()
        .find(|d| d.name == name)
        .map(|d| d.keyword.clone())
}

pub(crate) fn matching_processes<'a>(sys: &'a System, keyword: &str) -> Vec<&'a sysinfo::Process> {    sys.processes()
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

pub(crate) fn matches_agent_process(name: &str, args: &[String], keyword: &str) -> bool {
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
    use super::{known_program_paths, matches_agent_process, program_available};

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

    #[cfg(target_os = "windows")]
    #[test]
    fn detects_installed_cli_and_rejects_uninstalled_leftovers() {
        // 系统自带的 cmd 一定能解析到；随机名字不应被当成已安装。
        assert!(program_available("cmd"));
        assert!(!program_available("missing-agent-87a310"));
        assert!(known_program_paths("claude").iter().any(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().starts_with("claude"))
                .unwrap_or(false)
        }));
    }
}

/// CLI 是否仍装在系统上：先查 PATH（Windows 用 `where.exe`），再兜底常见安装目录。
///
/// 只认可执行文件——卸载后残留的日志/会话记录**不算**存在，所以"下载过又卸载"
/// 的 Agent 不会再出现在岛上。
pub(crate) fn program_available(program: &str) -> bool {
    let program = program.trim();
    if program.is_empty() {
        return false;
    }
    static CACHE: OnceLock<Mutex<HashMap<String, (Instant, bool)>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let key = program.to_lowercase();
    if let Ok(guard) = cache.lock() {
        if let Some((checked_at, value)) = guard.get(&key) {
            if checked_at.elapsed().as_secs() < 60 {
                return *value;
            }
        }
    }
    let value = resolve_program(program);
    if let Ok(mut guard) = cache.lock() {
        guard.insert(key, (Instant::now(), value));
    }
    value
}

#[cfg(target_os = "windows")]
fn resolve_program(program: &str) -> bool {
    // 显式路径与 PATH 都交给 where.exe 之外的同一套解析；`where.exe` 只认 PATH，所以再兜一次常见安装目录。
    if matches!(
        crate::commands::restart_program_available(program, None),
        Ok(true)
    ) {
        return true;
    }
    known_program_paths(program).iter().any(|path| path.is_file())
}

#[cfg(not(target_os = "windows"))]
fn resolve_program(program: &str) -> bool {
    if known_program_paths(program).iter().any(|path| path.is_file()) {
        return true;
    }
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths)
                .any(|directory| directory.join(program).is_file())
        })
        .unwrap_or(false)
}

/// `where.exe` 只看 PATH；这里补上各包管理器/安装器的常见落点。
fn known_program_paths(program: &str) -> Vec<PathBuf> {
    let mut bases: Vec<PathBuf> = Vec::new();
    if let Some(home) = home_dir() {
        for relative in [
            ".local/bin",
            ".cargo/bin",
            ".bun/bin",
            ".opencode/bin",
            ".claude/local",
        ] {
            bases.push(home.join(relative));
        }
    }
    for variable in ["APPDATA", "LOCALAPPDATA"] {
        if let Some(base) = std::env::var_os(variable) {
            bases.push(PathBuf::from(base).join("npm"));
        }
    }
    let mut candidates = Vec::new();
    for base in bases {
        for extension in ["", ".exe", ".cmd", ".bat", ".ps1"] {
            candidates.push(base.join(format!("{program}{extension}")));
        }
    }
    candidates
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProcessObservation {
    pub(crate) pid: u32,
    pub(crate) project_path: Option<String>,
    pub(crate) started_at_ms: u64,
}

pub(crate) fn process_fact_from_observations(
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

pub(crate) fn normalize_process_path(path: &str) -> String {
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
pub(crate) fn legacy_process_status(
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

pub(crate) fn scan_agents(
    sys: &mut System,
    session: &mut SessionState,
) -> (
    Vec<AgentInfo>,
    interface::snapshot::AgentViewSnapshot,
    interface::diagnostics::DiagnosticSnapshot,
    bool,
) {
    let _scan_started = Instant::now();
    refresh_agent_processes(sys);
    let _after_processes = Instant::now();
    let now = Instant::now();
    let delta = now.duration_since(session.last_poll).as_secs();
    session.last_poll = now;
    let mut agents: Vec<AgentInfo> = Vec::new();
    let mut process_facts = Vec::new();
    let mut session_candidates = Vec::new();
    let mut diagnostic_records = Vec::new();
    let mut acquisition_issues = Vec::new();
    let mut stats_changed = false;

    session.index_cache.begin_scan();
    for def in agent_defs() {
        let agent_name = &def.name;
        let agent_id = if def.log_kind.is_empty() {
            def.keyword.as_str()
        } else {
            def.log_kind.as_str()
        };
        let processes = matching_processes(sys, &def.keyword);
        if processes.is_empty() && !program_available(&def.keyword) {
            // 系统上已经找不到这个 CLI（可能是装了又卸载）：不再出现在岛上，
            // 也不再扫它残留的会话日志——残留日志不能证明 Agent 还在。
            session.activity.remove(agent_name);
            session.runtime_start.remove(agent_name);
            continue;
        }
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
        let mut session_scan = build_session_scan_for_process_cached(
            agent_name,
            Some(&process_fact.identity),
            &mut session.index_cache,
        );
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
    session.index_cache.finish_scan();
    if std::env::var_os("AGENT_ISLAND_SCAN_DEBUG").is_some() {
        eprintln!(
            "[scan] processes={}ms agents={}ms",
            _after_processes.duration_since(_scan_started).as_millis(),
            _after_processes.elapsed().as_millis()
        );
    }
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

pub(crate) fn attach_runtime_metadata_to_snapshot(
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

pub(crate) fn refresh_agent_processes(sys: &mut System) {
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, agent_process_refresh_kind());
}

pub(crate) fn agent_process_refresh_kind() -> ProcessRefreshKind {
    ProcessRefreshKind::nothing()
        .with_cmd(UpdateKind::Always)
        .with_cwd(UpdateKind::Always)
        .with_exe(UpdateKind::Always)
        .with_cpu()
        .with_memory()
}

pub(crate) fn legacy_status(display_status: domain::DisplayStatus) -> &'static str {
    match display_status {
        domain::DisplayStatus::Stopped => "stopped",
        domain::DisplayStatus::Idle => "idle",
        domain::DisplayStatus::Working => "working",
        domain::DisplayStatus::Done => "done",
        domain::DisplayStatus::Error => "error",
        domain::DisplayStatus::Waiting => "waiting",
    }
}

pub(crate) fn legacy_session(session: &interface::snapshot::SessionView) -> AgentSession {
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

pub(crate) fn apply_snapshot_projection(
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

pub(crate) fn update_terminal_statistics_from_snapshot(
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


