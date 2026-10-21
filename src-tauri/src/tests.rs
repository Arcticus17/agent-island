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
        !restart_program_available("C:\\不存在的测试目录\\Agent Tools\\missing.exe", None).unwrap()
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

    let value: Value = serde_json::from_str(&fs::read_to_string(&destination).unwrap()).unwrap();
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
    let path = std::env::temp_dir().join(format!("agent-island-missing-{}.jsonl", epoch_millis()));
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
    let root = std::env::temp_dir().join(format!("agent-island-d1-time-change-{}", epoch_millis()));
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
    assert_eq!(
        codex_data_root_from(None, Some(home)),
        Some(home.join(".codex"))
    );
    assert_eq!(
        codex_data_root_from(Some(OsStr::new("")), Some(home)),
        Some(home.join(".codex"))
    );
    let custom = OsStr::new("D:/自定义 数据/codex");
    assert_eq!(
        codex_data_root_from(Some(custom), Some(home)),
        Some(PathBuf::from(custom))
    );
    assert_eq!(
        codex_data_root_from(Some(custom), None),
        Some(PathBuf::from(custom))
    );
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
        fs::write(
            &path,
            format!(
                "{}\n",
                serde_json::json!({"type":"session_meta","payload":payload})
            ),
        )
        .unwrap();
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(UNIX_EPOCH + std::time::Duration::from_secs(10 + index))
            .unwrap();
    }
    let scan = scan_session_directory("codex", &root, None);
    fs::remove_dir_all(&root).unwrap();
    assert_eq!(scan.candidates.len(), 5);
    assert_eq!(scan.candidates[0].identity.session_id, "s0");
    assert_eq!(scan.legacy_sessions.len(), 3);
    assert_eq!(scan.legacy_sessions[0].id, "s0");
    assert_eq!(indexed_active_session(&scan, None, 20_000), None);
    let process = process_fact_from_observations(
        "codex",
        "Codex CLI",
        &[ProcessObservation {
            pid: 42,
            project_path: Some("D:/project".into()),
            started_at_ms: 0,
        }],
    );
    assert_eq!(
        indexed_active_session(&scan, Some(&process.identity), 20_000),
        None
    );
    let snapshot = interface::snapshot::build_snapshot(20_000, &[process], &scan.candidates);
    assert!(snapshot.agents[0].active_session.is_none());
    assert_eq!(snapshot.agents[0].history_sessions.len(), 3);
    assert_eq!(snapshot.agents[0].history_sessions[0].id, "s0");
}

#[test]
fn desktop_priority_requires_explicit_root_metadata() {
    let valid = serde_json::json!({"type":"session_meta","payload":{"originator":"codex_work_desktop","source":"vscode","thread_source":"user"}});
    assert!(is_codex_desktop_root(&valid.to_string()));
    for (key, value) in [
        ("originator", serde_json::json!("unknown")),
        ("source", serde_json::json!({"subagent":{}})),
        ("thread_source", serde_json::json!("guardian_review")),
        ("parent_thread_id", serde_json::json!("parent")),
    ] {
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
    use crate::interface::snapshot::{AgentView, AgentViewSnapshot, SessionSummary, SessionView};

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
fn locks_recover_after_a_panicking_holder() {
    let state = Mutex::new(7_u32);
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = state.lock().unwrap();
        panic!("holder failed");
    }));

    assert!(panicked.is_err(), "the holder must have panicked");
    assert!(state.is_poisoned(), "the mutex must actually be poisoned");
    assert_eq!(*state.lock_recover(), 7);
}

#[test]
fn fallback_send_command_accepts_known_agents_and_rejects_unknown_ones() {
    for name in ["Claude Code", "Codex CLI", "OpenCode", "Hermes"] {
        let command = fallback_send_command(name, "session-1", "你好")
            .unwrap_or_else(|error| panic!("{name} must build a command: {error}"));
        assert_eq!(command[0], "cmd.exe");
        assert!(command.iter().any(|part| part == "你好"));
    }

    assert!(fallback_send_command("自定义 Agent", "session-1", "hi").is_err());
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

/// HTTP 头名大小写不敏感：Node/undici 发小写，curl 等客户端发 `Content-Length`。
/// 只认小写会让别的客户端的 body 被当成空，审批事件永远弹不出来。
#[test]
fn hook_http_request_reads_canonical_case_headers() {
    use std::io::Write;
    use std::net::{TcpListener, TcpStream};

    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let address = listener.local_addr().unwrap();
    let body = r#"{"name":"PreToolUse","session":"s1","tool":"Bash","command":"rm -rf /tmp/x"}"#;
    let request = format!(
        "POST /event HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Agent-Island-Token: tok-1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let writer = std::thread::spawn(move || {
        let mut stream = TcpStream::connect(address).unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        stream.flush().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(80));
    });
    let (mut stream, _) = listener.accept().unwrap();
    let (uri, token, parsed_body) = hooks::read_http_request(&mut stream).expect("request parsed");
    assert_eq!(uri, "/event");
    assert_eq!(token, "tok-1");
    assert_eq!(parsed_body, body);
    writer.join().unwrap();
}

#[test]
fn incremental_index_reuses_unchanged_session_files() {
    let root = std::env::temp_dir().join(format!("agent-island-index-cache-{}", epoch_millis()));
    fs::create_dir_all(&root).unwrap();
    for index in 0..5 {
        fs::write(
            root.join(format!("session-{index}.jsonl")),
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"current\",\"cwd\":\"D:/target\"}}\n",
        )
        .unwrap();
    }
    let process = process_fact_from_observations(
        "codex",
        "Codex CLI",
        &[ProcessObservation {
            pid: 7,
            project_path: Some("D:/target".into()),
            started_at_ms: epoch_millis(),
        }],
    );
    let mut cache = SessionIndexCache::default();

    cache.begin_scan();
    let first = scan_session_directory_with("codex", &root, Some(&process.identity), &mut cache);
    cache.finish_scan();
    assert_eq!(cache.index_len(), 5, "cold scan indexes every file once");
    // 冷扫还会额外解析一次活跃会话详情，所以 miss 至少是 5
    assert!(cache.misses() >= 5, "cold scan misses: {}", cache.misses());
    assert_eq!(cache.hits(), 0);
    assert_eq!(first.candidates.len(), 5);

    cache.begin_scan();
    let second = scan_session_directory_with("codex", &root, Some(&process.identity), &mut cache);
    cache.finish_scan();
    assert_eq!(cache.misses(), 0, "unchanged files must not be re-read");
    assert!(cache.hits() >= 5, "expected cache hits, got {}", cache.hits());
    assert_eq!(second.candidates.len(), 5);
    assert_eq!(second.candidates[0].identity.session_id, first.candidates[0].identity.session_id);

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn incremental_index_reindexes_a_changed_file_and_prunes_deleted_ones() {
    let root = std::env::temp_dir().join(format!("agent-island-index-change-{}", epoch_millis()));
    fs::create_dir_all(&root).unwrap();
    let kept = root.join("kept.jsonl");
    let changed = root.join("changed.jsonl");
    let removed = root.join("removed.jsonl");
    let meta = "{\"type\":\"session_meta\",\"payload\":{\"id\":\"current\",\"cwd\":\"D:/target\"}}\n";
    fs::write(&kept, meta).unwrap();
    fs::write(&changed, meta).unwrap();
    fs::write(&removed, meta).unwrap();
    let process = process_fact_from_observations(
        "codex",
        "Codex CLI",
        &[ProcessObservation {
            pid: 9,
            project_path: Some("D:/target".into()),
            started_at_ms: epoch_millis(),
        }],
    );
    let mut cache = SessionIndexCache::default();
    cache.begin_scan();
    scan_session_directory_with("codex", &root, Some(&process.identity), &mut cache);
    cache.finish_scan();
    assert_eq!(cache.index_len(), 3);

    // 改一个、删一个，再扫一遍：只有改过的文件被重新解析，删掉的条目会被清掉。
    let mut longer = meta.to_string();
    longer.push_str("{\"timestamp\":\"2026-09-04T10:00:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\",\"message\":\"changed\"}}\n");
    fs::write(&changed, longer).unwrap();
    fs::remove_file(&removed).unwrap();

    cache.begin_scan();
    let scan = scan_session_directory_with("codex", &root, Some(&process.identity), &mut cache);
    cache.finish_scan();
    assert!(cache.misses() >= 1, "changed file must be re-indexed");
    assert_eq!(cache.index_len(), 2, "deleted file is pruned");
    assert_eq!(scan.candidates.len(), 2);
    let changed_session = scan
        .candidates
        .iter()
        .find(|candidate| candidate.view.log_path.as_deref() == Some(changed.to_string_lossy().as_ref()))
        .expect("changed file still indexed");
    assert!(changed_session
        .view
        .records
        .iter()
        .any(|record| record.text.contains("changed")));

    fs::remove_dir_all(root).unwrap();
}
