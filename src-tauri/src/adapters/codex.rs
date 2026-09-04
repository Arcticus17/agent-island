use serde_json::Value;

use crate::adapters::{AgentAdapter, ParseReport};
use crate::domain::{
    Confidence, ConversationMessage, DomainEvent, EventKind, EventSource, MessageRole,
};

/// Normalizes the event stream written by Codex CLI into the shared domain model.
pub struct CodexAdapter;

impl AgentAdapter for CodexAdapter {
    fn parse(&self, text: &str) -> ParseReport {
        let mut report = ParseReport::default();
        let mut session_id = String::from("codex");
        let mut turn_id = String::from("turn-1");
        let mut turn_number = 1_u64;

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
            let payload = record.get("payload").unwrap_or(&record);
            let Some(payload_type) = payload.get("type").and_then(Value::as_str) else {
                // Usage and session metadata records do not participate in the domain stream.
                continue;
            };

            if record_type == "session_meta" || payload_type == "session_meta" {
                if let Some(id) = string_field(payload, &["id", "session_id", "sessionId"]) {
                    session_id = id;
                }
                continue;
            }
            if record_type == "turn_context" || payload_type == "turn_context" {
                if let Some(id) = turn_id_field(&record, payload) {
                    turn_id = id;
                }
                continue;
            }
            if payload_type == "token_count" {
                continue;
            }

            let is_user_message = record_type == "event_msg" && payload_type == "user_message";
            let is_response_message = record_type == "response_item" && payload_type == "message";
            let is_function_call =
                record_type == "response_item" && payload_type == "function_call";
            let is_function_output =
                record_type == "response_item" && payload_type == "function_call_output";
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
                "turn_complete"
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
                let Some(message_text) = explicit_output_text(content) else {
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

            let kind = if is_function_call {
                EventKind::ToolStarted
            } else if is_function_output {
                EventKind::ToolFinished {
                    success: function_output_succeeded(payload),
                }
            } else if is_attention {
                EventKind::AttentionRequested {
                    approval: !matches!(payload_type, "request_user_input" | "input_request"),
                }
            } else if matches!(
                payload_type,
                "turn_complete" | "turn_completed" | "turn_succeeded"
            ) {
                EventKind::TurnSucceeded
            } else {
                EventKind::TurnFailed
            };
            report
                .events
                .push(event(&session_id, &turn_id, at_ms, kind));
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

fn explicit_output_text(blocks: &[Value]) -> Option<String> {
    let text = blocks
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("output_text"))
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then_some(text)
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
}
