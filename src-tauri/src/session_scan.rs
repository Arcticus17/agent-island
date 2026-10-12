//! Session discovery: bounded reads, per-agent scans, and identity matching.

use crate::*;

pub(crate) fn read_tail(path: &Path, max_bytes: u64) -> String {
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
pub(crate) fn clean_line(s: &str, max_chars: usize) -> String {
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
pub(crate) fn json_text(v: &Value) -> Option<String> {
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

pub(crate) fn extract_paths(text: &str) -> Vec<String> {
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

pub(crate) fn list_newest_files(root: &Path, depth: usize, ext: &str, max: usize) -> Vec<PathBuf> {
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

pub(crate) fn session_name(cwd: Option<&str>, fallback: &str) -> String {
    if let Some(c) = cwd {
        let parts: Vec<&str> = c.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
        if let Some(last) = parts.last() {
            return last.to_string();
        }
    }
    fallback.chars().take(24).collect()
}

pub(crate) fn to_session_with_id(snap: LogSnapshot, id: String) -> AgentSession {
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

pub(crate) fn session_scan_from_file(agent_id: &str, path: &Path) -> Option<SessionScan> {
    session_scan_from_file_with_budget(agent_id, path, 64 * 1024, 512 * 1024)
}

pub(crate) fn session_scan_from_file_with_budget(
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

pub(crate) const MAX_DISPLAY_RECORDS: usize = 24;
pub(crate) const MAX_DISPLAY_RECORD_TEXT_CHARS: usize = 600;
pub(crate) const MAX_RECENT_OUTPUT_RECORDS: usize = 5;
pub(crate) const MAX_RECENT_OUTPUT_TEXT_CHARS: usize = 180;

pub(crate) fn chronological_records(
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

pub(crate) fn truncate_display_text(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

pub(crate) fn compatibility_output(records: &[domain::ConversationMessage]) -> Vec<String> {
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

pub(crate) fn snapshot_from_bounded_text(
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

pub(crate) fn codex_metadata(text: &str) -> (Option<String>, Option<String>) {
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

pub(crate) fn is_codex_desktop_root(text: &str) -> bool {
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|value| value.get("type").and_then(Value::as_str) == Some("session_meta"))
        .is_some_and(|value| {
            let payload = &value["payload"];
            matches!(
                payload["originator"].as_str(),
                Some("Codex Desktop" | "codex_work_desktop")
            ) && payload["thread_source"].as_str() == Some("user")
                && payload.get("parent_thread_id").is_none_or(Value::is_null)
                && payload["source"].as_str() == Some("vscode")
        })
}

pub(crate) fn session_candidate_from_legacy(
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

pub(crate) fn scan_session_directory(
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
        scan.desktop_root_paths
            .append(&mut identity_scan.desktop_root_paths);
        scan.candidates.append(&mut identity_scan.candidates);
        scan.diagnostics.append(&mut identity_scan.diagnostics);
        if identity_scan.acquisition == AcquisitionCompleteness::Incomplete {
            scan.acquisition = AcquisitionCompleteness::Incomplete;
        }
    }
    // This changes display priority only: all identities remain available to matching.
    // Stable ordering preserves newest-first order within each group.
    scan.candidates.sort_by_key(|candidate| {
        !candidate
            .view
            .log_path
            .as_ref()
            .is_some_and(|path| scan.desktop_root_paths.contains(path))
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

pub(crate) fn indexed_active_session(
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

pub(crate) fn load_session_detail(scan: &mut SessionScan, agent_id: &str, path: &Path) {
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

pub(crate) fn index_session_file(agent_id: &str, path: &Path) -> SessionScan {
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
        scan.desktop_root_paths
            .push(path.to_string_lossy().into_owned());
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

pub(crate) fn opencode_session_scan() -> SessionScan {
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
pub(crate) fn combine_session_scans(scans: impl Iterator<Item = SessionScan>) -> SessionScan {
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

pub(crate) static HERMES_CACHE: OnceLock<Mutex<Option<(Instant, SessionScan)>>> = OnceLock::new();

pub(crate) fn run_command_timeout(
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

pub(crate) fn hermes_session_scan_from_text(text: &str) -> SessionScan {
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

pub(crate) fn parse_hermes_session_scan() -> SessionScan {
    let mut command = quiet_command("hermes");
    command.args(["sessions", "list"]);
    hermes_scan_from_command(&mut command, 8)
}

pub(crate) fn hermes_scan_from_command(
    command: &mut std::process::Command,
    timeout_secs: u64,
) -> SessionScan {
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

pub(crate) fn hermes_session_scan() -> SessionScan {
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

pub(crate) fn build_session_scan(name: &str) -> SessionScan {
    build_session_scan_for_process(name, None)
}

pub(crate) fn build_session_scan_for_process(
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

pub(crate) fn build_sessions(name: &str) -> Vec<AgentSession> {
    build_session_scan(name).legacy_sessions
}

pub(crate) fn epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Default)]
pub(crate) struct AcquiredText {
    pub(crate) text: String,
    pub(crate) modified_at_ms: u64,
    pub(crate) issues: Vec<domain::DataIssue>,
}

pub(crate) fn read_bounded_head_tail(
    path: &Path,
    head_bytes: u64,
    tail_bytes: u64,
) -> AcquiredText {
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

pub(crate) fn read_bounded_stream(
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
