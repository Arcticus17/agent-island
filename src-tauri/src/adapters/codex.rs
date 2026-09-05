use serde_json::Value;
use std::collections::HashMap;

use crate::adapters::{AgentAdapter, ParseReport};
use crate::domain::{
    Confidence, ConversationMessage, DomainEvent, EventKind, EventSource, MessageRole,
};

/// Normalizes the event stream written by Codex CLI into the shared domain model.
pub struct CodexAdapter;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolCallKind {
    Function,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ToolCallIdentity {
    session_id: String,
    turn_id: String,
    call_id: String,
}

impl AgentAdapter for CodexAdapter {
    fn parse(&self, text: &str) -> ParseReport {
        let mut report = ParseReport::default();
        let mut session_id = String::from("codex");
        let mut turn_id = String::from("turn-1");
        let mut turn_number = 1_u64;
        let mut outstanding_calls = HashMap::<ToolCallIdentity, ToolCallKind>::new();

        for (line_number, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }

            let Ok(record) = serde_json::from_str::<Value>(line) else {
                report.skipped_lines += 1;
                continue;
            };
            let Some(record_type) = record.get("type").and_then(Value::as_str) else {
                report.skipped_lines += 1;
                continue;
            };
            // These top-level envelopes intentionally have metadata payloads without a `type`.
            // Dispatch on the envelope before looking for an inner event type.
            if record_type == "session_meta" {
                let Some(payload) = record.get("payload").filter(|value| value.is_object()) else {
                    report.skipped_lines += 1;
                    continue;
                };
                let Some(id) = string_field(payload, &["id", "session_id", "sessionId"]) else {
                    report.skipped_lines += 1;
                    continue;
                };
                session_id = id;
                continue;
            }
            if record_type == "turn_context" {
                let Some(payload) = record.get("payload").filter(|value| value.is_object()) else {
                    report.skipped_lines += 1;
                    continue;
                };
                let Some(id) = turn_id_field(&record, payload) else {
                    report.skipped_lines += 1;
                    continue;
                };
                turn_id = id;
                continue;
            }

            let Some(payload) = record.get("payload").filter(|value| value.is_object()) else {
                // event_msg and response_item are recognized envelopes and malformed payloads
                // must be visible in the parse report. Unknown top-level records remain ignored.
                if matches!(record_type, "event_msg" | "response_item") {
                    report.skipped_lines += 1;
                }
                continue;
            };
            let Some(payload_type) = payload.get("type").and_then(Value::as_str) else {
                if matches!(record_type, "event_msg" | "response_item") {
                    report.skipped_lines += 1;
                }
                continue;
            };
            if payload_type == "token_count" {
                continue;
            }

            let is_user_message = record_type == "event_msg" && payload_type == "user_message";
            let is_response_message = record_type == "response_item" && payload_type == "message";
            let is_function_call =
                record_type == "response_item" && payload_type == "function_call";
            let is_function_output =
                record_type == "response_item" && payload_type == "function_call_output";
            let is_custom_call =
                record_type == "response_item" && payload_type == "custom_tool_call";
            let is_custom_output =
                record_type == "response_item" && payload_type == "custom_tool_call_output";
            let is_attention = matches!(
                payload_type,
                "permission_request"
                    | "approval_request"
                    | "exec_approval_request"
                    | "apply_patch_approval_request"
                    | "approval"
                    | "request_user_input"
                    | "input_request"
            );
            let is_terminal = matches!(
                payload_type,
                "task_started"
                    | "task_complete"
                    | "turn_complete"
                    | "turn_completed"
                    | "turn_succeeded"
                    | "turn_failed"
                    | "turn_aborted"
                    | "error"
            );
            if !(is_user_message
                || is_response_message
                || is_function_call
                || is_function_output
                || is_custom_call
                || is_custom_output
                || is_attention
                || is_terminal)
            {
                continue;
            }

            let Some(at_ms) = record
                .get("timestamp")
                .and_then(Value::as_str)
                .and_then(parse_rfc3339_ms)
            else {
                report.skipped_lines += 1;
                continue;
            };

            if is_user_message {
                let Some(message) = payload.get("message").and_then(Value::as_str) else {
                    report.skipped_lines += 1;
                    continue;
                };
                let message = message.trim();
                if message.is_empty() {
                    report.skipped_lines += 1;
                    continue;
                }
                if let Some(explicit_turn_id) = turn_id_field(&record, payload) {
                    turn_id = explicit_turn_id;
                } else if has_started_turn(&report, &session_id, &turn_id) {
                    turn_number += 1;
                    turn_id = format!("turn-{turn_number}");
                }
                report
                    .events
                    .push(event(&session_id, &turn_id, at_ms, EventKind::TurnStarted));
                report.messages.push(ConversationMessage {
                    event_id: event_id(&record, payload, line_number),
                    at_ms,
                    role: MessageRole::User,
                    text: message.to_owned(),
                });
                continue;
            }

            if matches!(payload_type, "task_started") {
                if let Some(explicit_turn_id) = turn_id_field(&record, payload) {
                    turn_id = explicit_turn_id;
                }
                report
                    .events
                    .push(event(&session_id, &turn_id, at_ms, EventKind::TurnStarted));
                continue;
            }
            let event_turn_id = turn_id_field(&record, payload).unwrap_or_else(|| turn_id.clone());

            if is_response_message {
                let Some(role) = payload.get("role").and_then(Value::as_str) else {
                    report.skipped_lines += 1;
                    continue;
                };
                let message_role = match role {
                    "user" => MessageRole::User,
                    "assistant" => MessageRole::Assistant,
                    _ => continue,
                };
                let Some(content) = payload.get("content").and_then(Value::as_array) else {
                    report.skipped_lines += 1;
                    continue;
                };
                let Some(message_text) = explicit_message_text(content, message_role) else {
                    continue;
                };
                report.messages.push(ConversationMessage {
                    event_id: event_id(&record, payload, line_number),
                    at_ms,
                    role: message_role,
                    text: message_text,
                });
                continue;
            }

            if is_function_call || is_custom_call {
                let Some(call_id) = non_empty_string(payload, "call_id") else {
                    report.skipped_lines += 1;
                    continue;
                };
                let call_kind = if is_function_call {
                    ToolCallKind::Function
                } else {
                    ToolCallKind::Custom
                };
                let required_field = if is_function_call {
                    "arguments"
                } else {
                    "input"
                };
                if non_empty_string(payload, "name").is_none()
                    || payload
                        .get(required_field)
                        .map(Value::is_null)
                        .unwrap_or(true)
                {
                    report.skipped_lines += 1;
                    continue;
                }
                let identity = ToolCallIdentity {
                    session_id: session_id.clone(),
                    turn_id: event_turn_id.clone(),
                    call_id: call_id.to_owned(),
                };
                if outstanding_calls.contains_key(&identity) {
                    report.skipped_lines += 1;
                    continue;
                }
                outstanding_calls.insert(identity, call_kind);
            } else if is_function_output || is_custom_output {
                let Some(call_id) = non_empty_string(payload, "call_id") else {
                    report.skipped_lines += 1;
                    continue;
                };
                let call_kind = if is_function_output {
                    ToolCallKind::Function
                } else {
                    ToolCallKind::Custom
                };
                let identity = ToolCallIdentity {
                    session_id: session_id.clone(),
                    turn_id: event_turn_id.clone(),
                    call_id: call_id.to_owned(),
                };
                if payload.get("output").is_none()
                    || outstanding_calls.get(&identity) != Some(&call_kind)
                {
                    report.skipped_lines += 1;
                    continue;
                }
                outstanding_calls.remove(&identity);
            }

            let kind = if is_function_call || is_custom_call {
                EventKind::ToolStarted
            } else if is_function_output || is_custom_output {
                EventKind::ToolFinished {
                    success: function_output_succeeded(payload),
                }
            } else if is_attention {
                EventKind::AttentionRequested {
                    approval: !matches!(payload_type, "request_user_input" | "input_request"),
                }
            } else if matches!(
                payload_type,
                "task_complete" | "turn_complete" | "turn_completed" | "turn_succeeded"
            ) {
                EventKind::TurnSucceeded
            } else {
                EventKind::TurnFailed
            };
            report
                .events
                .push(event(&session_id, &event_turn_id, at_ms, kind));
        }

        report
    }
}

fn has_started_turn(report: &ParseReport, session_id: &str, turn_id: &str) -> bool {
    report.events.iter().any(|event| {
        event.session_id == session_id
            && event.turn_id == turn_id
            && matches!(event.kind, EventKind::TurnStarted)
    })
}

fn function_output_succeeded(payload: &Value) -> bool {
    if let Some(success) = payload.get("success").and_then(Value::as_bool) {
        return success;
    }
    if let Some(status) = payload.get("status").and_then(Value::as_str) {
        if matches!(status, "failed" | "error" | "cancelled" | "canceled") {
            return false;
        }
        if matches!(status, "success" | "succeeded" | "completed") {
            return true;
        }
    }
    let Some(output) = payload.get("output").and_then(Value::as_str) else {
        return true;
    };
    exit_code(output).map(|code| code == 0).unwrap_or(true)
}

fn exit_code(output: &str) -> Option<i64> {
    for marker in [
        "Process exited with code ",
        "process exited with code ",
        "exit code: ",
    ] {
        if let Some(rest) = output.split_once(marker).map(|(_, rest)| rest) {
            let digits = rest
                .trim_start()
                .strip_prefix('-')
                .map_or(rest.trim_start(), |positive| positive);
            let digits = digits
                .chars()
                .take_while(|character| character.is_ascii_digit())
                .collect::<String>();
            if !digits.is_empty() {
                let mut value = digits.parse::<i64>().ok()?;
                if rest.trim_start().starts_with('-') {
                    value = -value;
                }
                return Some(value);
            }
        }
    }
    None
}

fn explicit_message_text(blocks: &[Value], role: MessageRole) -> Option<String> {
    let expected_type = match role {
        MessageRole::User => "input_text",
        MessageRole::Assistant => "output_text",
        MessageRole::Tool => return None,
    };
    let text = blocks
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some(expected_type))
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then_some(text)
}

fn non_empty_string<'a>(record: &'a Value, name: &str) -> Option<&'a str> {
    let value = record.get(name).and_then(Value::as_str)?.trim();
    (!value.is_empty()).then_some(value)
}

fn turn_id_field(record: &Value, payload: &Value) -> Option<String> {
    string_field(payload, &["turn_id", "turnId"])
        .or_else(|| string_field(record, &["turn_id", "turnId"]))
}

fn event_id(record: &Value, payload: &Value, line_number: usize) -> String {
    string_field(payload, &["id", "event_id", "eventId", "uuid"])
        .or_else(|| string_field(record, &["id", "event_id", "eventId", "uuid"]))
        .unwrap_or_else(|| format!("line-{}", line_number + 1))
}

fn string_field(record: &Value, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| record.get(*name).and_then(Value::as_str))
        .map(str::to_owned)
}

fn event(session_id: &str, turn_id: &str, at_ms: u64, kind: EventKind) -> DomainEvent {
    DomainEvent {
        agent_id: "codex".to_owned(),
        session_id: session_id.to_owned(),
        turn_id: turn_id.to_owned(),
        at_ms,
        source: EventSource::CodexLog,
        confidence: Confidence::Confirmed,
        kind,
    }
}

fn parse_rfc3339_ms(timestamp: &str) -> Option<u64> {
    if !timestamp.is_ascii() {
        return None;
    }
    let (date, time_with_zone) = timestamp.split_once('T')?;
    if date.len() != 10 || &date[4..5] != "-" || &date[7..8] != "-" {
        return None;
    }
    let year = date[0..4].parse::<i64>().ok()?;
    let month = date[5..7].parse::<u32>().ok()?;
    let day = date[8..10].parse::<u32>().ok()?;
    if day == 0 || day > days_in_month(year, month)? {
        return None;
    }

    let (clock, offset_minutes) = if let Some(clock) = time_with_zone
        .strip_suffix('Z')
        .or_else(|| time_with_zone.strip_suffix('z'))
    {
        (clock, 0_i64)
    } else {
        let offset_start = time_with_zone.rfind(['+', '-'])?;
        let (clock, offset) = time_with_zone.split_at(offset_start);
        (clock, parse_offset_minutes(offset)?)
    };

    let mut clock_parts = clock.split(':');
    let hour = parse_two_digits(clock_parts.next()?)?;
    let minute = parse_two_digits(clock_parts.next()?)?;
    let second_and_fraction = clock_parts.next()?;
    if clock_parts.next().is_some() || hour > 23 || minute > 59 {
        return None;
    }
    let (second_text, fraction) = second_and_fraction
        .split_once('.')
        .map_or((second_and_fraction, None), |(second, fraction)| {
            (second, Some(fraction))
        });
    let second = parse_two_digits(second_text)?;
    if second > 59 {
        return None;
    }
    let fractional_ms = match fraction {
        Some(fraction) => parse_fractional_ms(fraction)?,
        None => 0,
    };

    let local_ms = i128::from(days_from_civil(year, month, day)) * 86_400_000
        + i128::from(hour) * 3_600_000
        + i128::from(minute) * 60_000
        + i128::from(second) * 1_000
        + i128::from(fractional_ms);
    let utc_ms = local_ms - i128::from(offset_minutes) * 60_000;
    u64::try_from(utc_ms).ok()
}

fn parse_two_digits(text: &str) -> Option<u32> {
    if text.len() != 2 || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

fn parse_fractional_ms(fraction: &str) -> Option<u32> {
    if fraction.is_empty() || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let mut milliseconds = 0;
    for (index, byte) in fraction.bytes().take(3).enumerate() {
        milliseconds += u32::from(byte - b'0') * [100, 10, 1][index];
    }
    Some(milliseconds)
}

fn parse_offset_minutes(offset: &str) -> Option<i64> {
    if offset.len() != 6 || &offset[3..4] != ":" {
        return None;
    }
    let sign = match &offset[0..1] {
        "+" => 1_i64,
        "-" => -1_i64,
        _ => return None,
    };
    let hours = parse_two_digits(&offset[1..3])?;
    let minutes = parse_two_digits(&offset[4..6])?;
    if hours > 23 || minutes > 59 {
        return None;
    }
    Some(sign * i64::from(hours * 60 + minutes))
}

fn days_in_month(year: i64, month: u32) -> Option<u32> {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => Some(31),
        4 | 6 | 9 | 11 => Some(30),
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => Some(29),
        2 => Some(28),
        _ => None,
    }
}

fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let adjusted_month = i64::from(month) + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * adjusted_month + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::CodexAdapter;
    use crate::adapters::AgentAdapter;
    use crate::domain::{EventKind, MessageRole};

    #[test]
    fn recovered_command_failure_finishes_successfully() {
        let report = CodexAdapter.parse(include_str!(
            "../../tests/fixtures/codex/recovered-command.jsonl"
        ));

        assert!(report
            .events
            .iter()
            .any(|event| matches!(event.kind, EventKind::ToolFinished { success: false })));
        assert!(matches!(
            report.events.last().map(|event| &event.kind),
            Some(EventKind::TurnSucceeded)
        ));
    }

    #[test]
    fn current_turn_preserves_file_order_and_explicit_messages() {
        let report = CodexAdapter.parse(include_str!(
            "../../tests/fixtures/codex/current-turn.jsonl"
        ));

        assert_eq!(
            report
                .messages
                .iter()
                .map(|message| (message.role, message.text.as_str()))
                .collect::<Vec<_>>(),
            [
                (MessageRole::User, "run checks"),
                (MessageRole::Assistant, "checks passed")
            ]
        );
        assert_eq!(report.events[0].session_id, "codex-session-real");
        assert_eq!(report.events[0].turn_id, "turn-real-1");
        assert_eq!(
            report
                .events
                .iter()
                .map(|event| &event.kind)
                .collect::<Vec<_>>(),
            [
                &EventKind::TurnStarted,
                &EventKind::ToolStarted,
                &EventKind::ToolFinished { success: true },
                &EventKind::TurnSucceeded,
            ]
        );
    }

    #[test]
    fn approvals_and_explicit_errors_are_structured_without_text_heuristics() {
        let input = concat!(
            r#"{"timestamp":"2026-09-04T10:00:00Z","type":"session_meta","payload":{"id":"s"}}"#,
            "\n",
            r#"{"timestamp":"2026-09-04T10:00:00Z","type":"event_msg","payload":{"type":"task_started","turn_id":"t"}}"#,
            "\n",
            r#"{"timestamp":"2026-09-04T10:00:01Z","type":"event_msg","payload":{"type":"exec_approval_request","turn_id":"t"}}"#,
            "\n",
            r#"{"timestamp":"2026-09-04T10:00:02Z","type":"event_msg","payload":{"type":"request_user_input","turn_id":"t","message":"confirm"}}"#,
            "\n",
            r#"{"timestamp":"2026-09-04T10:00:03Z","type":"event_msg","payload":{"type":"error","message":"failed text is not inferred"}}"#,
        );

        let report = CodexAdapter.parse(input);

        assert_eq!(
            report
                .events
                .iter()
                .map(|event| &event.kind)
                .collect::<Vec<_>>(),
            [
                &EventKind::TurnStarted,
                &EventKind::AttentionRequested { approval: true },
                &EventKind::AttentionRequested { approval: false },
                &EventKind::TurnFailed,
            ]
        );
    }

    #[test]
    fn explicit_old_task_complete_does_not_rewind_active_turn_context() {
        let input = concat!(
            r#"{"timestamp":"2026-09-04T10:00:00Z","type":"session_meta","payload":{"id":"s"}}"#,
            "\n",
            r#"{"timestamp":"2026-09-04T10:00:00Z","type":"event_msg","payload":{"type":"task_started","turn_id":"turn-a"}}"#,
            "\n",
            r#"{"timestamp":"2026-09-04T10:00:01Z","type":"event_msg","payload":{"type":"task_started","turn_id":"turn-b"}}"#,
            "\n",
            r#"{"timestamp":"2026-09-04T10:00:02Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-a"}}"#,
            "\n",
            r#"{"timestamp":"2026-09-04T10:00:03Z","type":"response_item","payload":{"type":"function_call","call_id":"b-call","name":"tool","arguments":"{}"}}"#,
            "\n",
            r#"{"timestamp":"2026-09-04T10:00:04Z","type":"response_item","payload":{"type":"function_call_output","call_id":"b-call","output":"Process exited with code 0"}}"#,
        );

        let report = CodexAdapter.parse(input);

        assert_eq!(
            report
                .events
                .iter()
                .map(|event| event.turn_id.as_str())
                .collect::<Vec<_>>(),
            ["turn-a", "turn-b", "turn-a", "turn-b", "turn-b"]
        );
    }

    #[test]
    fn unmatched_tool_outputs_are_skipped_and_valid_calls_are_paired() {
        let input = concat!(
            r#"{"timestamp":"2026-09-04T10:00:00Z","type":"event_msg","payload":{"type":"task_started","turn_id":"t"}}"#,
            "\n",
            r#"{"timestamp":"2026-09-04T10:00:01Z","type":"response_item","payload":{"type":"custom_tool_call_output","call_id":"missing","output":"Process exited with code 0"}}"#,
            "\n",
            r#"{"timestamp":"2026-09-04T10:00:02Z","type":"response_item","payload":{"type":"custom_tool_call","call_id":"c1","name":"tool","input":{}}}"#,
            "\n",
            r#"{"timestamp":"2026-09-04T10:00:03Z","type":"response_item","payload":{"type":"custom_tool_call_output","call_id":"wrong","output":"Process exited with code 0"}}"#,
            "\n",
            r#"{"timestamp":"2026-09-04T10:00:04Z","type":"response_item","payload":{"type":"custom_tool_call_output","call_id":"c1","output":"Process exited with code 0"}}"#,
        );

        let report = CodexAdapter.parse(input);

        assert_eq!(
            report
                .events
                .iter()
                .map(|event| &event.kind)
                .collect::<Vec<_>>(),
            [
                &EventKind::TurnStarted,
                &EventKind::ToolStarted,
                &EventKind::ToolFinished { success: true },
            ]
        );
        assert_eq!(report.skipped_lines, 2);
    }

    #[test]
    fn malformed_recognized_envelopes_count_once_and_token_counts_are_ignored() {
        let input = concat!(
            r#"{"timestamp":"2026-09-04T10:00:00Z","type":"event_msg","payload":{"type":"task_started","turn_id":"t"}}"#,
            "\n",
            r#"{"timestamp":"2026-09-04T10:00:01Z","type":"response_item","payload":{"type":"function_call","name":"tool"}}"#,
            "\n",
            r#"{"timestamp":"2026-09-04T10:00:02Z","type":"response_item","payload":{"type":"message","role":"user","content":{}}}"#,
            "\n",
            r#"{"timestamp":"2026-09-04T10:00:03Z","type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"total_tokens":1}}}}"#,
            "\n",
            r#"{"timestamp":"2026-09-04T10:00:04Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"t"}}"#,
        );

        let report = CodexAdapter.parse(input);

        assert_eq!(report.skipped_lines, 2);
        assert!(matches!(
            report.events.last().map(|event| &event.kind),
            Some(EventKind::TurnSucceeded)
        ));
    }

    #[test]
    fn active_tool_identity_rejects_duplicate_kind_and_scope_mismatches() {
        let input = concat!(
            r#"{"timestamp":"2026-09-04T10:00:00Z","type":"session_meta","payload":{"id":"s1"}}"#,
            "\n",
            r#"{"timestamp":"2026-09-04T10:00:00Z","type":"event_msg","payload":{"type":"task_started","turn_id":"t1"}}"#,
            "\n",
            r#"{"timestamp":"2026-09-04T10:00:01Z","type":"response_item","payload":{"type":"function_call","call_id":"c1","name":"tool","arguments":"{}"}}"#,
            "\n",
            r#"{"timestamp":"2026-09-04T10:00:02Z","type":"response_item","payload":{"type":"function_call","call_id":"c1","name":"tool","arguments":"{}"}}"#,
            "\n",
            r#"{"timestamp":"2026-09-04T10:00:03Z","type":"response_item","payload":{"type":"custom_tool_call_output","call_id":"c1","output":"ok"}}"#,
            "\n",
            r#"{"timestamp":"2026-09-04T10:00:04Z","type":"response_item","payload":{"type":"function_call_output","call_id":"c1","output":"ok"}}"#,
            "\n",
            r#"{"timestamp":"2026-09-04T10:00:05Z","type":"event_msg","payload":{"type":"task_started","turn_id":"t2"}}"#,
            "\n",
            r#"{"timestamp":"2026-09-04T10:00:06Z","type":"response_item","payload":{"type":"function_call","call_id":"c2","name":"tool","arguments":"{}"}}"#,
            "\n",
            r#"{"timestamp":"2026-09-04T10:00:07Z","type":"event_msg","payload":{"type":"task_started","turn_id":"t3"}}"#,
            "\n",
            r#"{"timestamp":"2026-09-04T10:00:08Z","type":"response_item","payload":{"type":"function_call_output","call_id":"c2","output":"ok"}}"#,
            "\n",
            r#"{"timestamp":"2026-09-04T10:00:09Z","type":"session_meta","payload":{"id":"s2"}}"#,
            "\n",
            r#"{"timestamp":"2026-09-04T10:00:10Z","type":"response_item","payload":{"type":"function_call_output","call_id":"c2","output":"ok"}}"#,
        );

        let report = CodexAdapter.parse(input);

        assert_eq!(
            report
                .events
                .iter()
                .map(|event| &event.kind)
                .collect::<Vec<_>>(),
            [
                &EventKind::TurnStarted,
                &EventKind::ToolStarted,
                &EventKind::ToolFinished { success: true },
                &EventKind::TurnStarted,
                &EventKind::ToolStarted,
                &EventKind::TurnStarted,
            ]
        );
        assert_eq!(report.skipped_lines, 4);
    }

    #[test]
    fn function_and_custom_calls_require_their_structured_fields() {
        let input = concat!(
            r#"{"timestamp":"2026-09-04T10:00:00Z","type":"response_item","payload":{"type":"function_call","call_id":"f1"}}"#,
            "\n",
            r#"{"timestamp":"2026-09-04T10:00:01Z","type":"response_item","payload":{"type":"custom_tool_call","call_id":"c1","name":"tool"}}"#,
        );

        let report = CodexAdapter.parse(input);

        assert!(report.events.is_empty());
        assert_eq!(report.skipped_lines, 2);
    }
}
