use std::collections::HashMap;

use serde_json::Value;

use crate::adapters::{AgentAdapter, ParseReport};
use crate::domain::{
    Confidence, ConversationMessage, DomainEvent, EventKind, EventSource, MessageRole,
};

pub struct ClaudeAdapter;

impl AgentAdapter for ClaudeAdapter {
    fn parse(&self, text: &str) -> ParseReport {
        let mut report = ParseReport::default();
        let mut current_turns = HashMap::<String, String>::new();

        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            let Ok(record) = serde_json::from_str::<Value>(line) else {
                report.skipped_lines += 1;
                continue;
            };
            let Some(record_type) = record.get("type").and_then(Value::as_str) else {
                report.skipped_lines += 1;
                continue;
            };
            if !matches!(
                record_type,
                "user"
                    | "assistant"
                    | "permission_request"
                    | "approval_request"
                    | "input_request"
                    | "result"
                    | "error"
            ) {
                continue;
            }

            let Some(session_id) = string_field(&record, &["sessionId", "session_id"]) else {
                report.skipped_lines += 1;
                continue;
            };
            let Some(event_id) = string_field(&record, &["uuid", "event_id"]) else {
                report.skipped_lines += 1;
                continue;
            };
            let Some(at_ms) = record
                .get("timestamp")
                .and_then(Value::as_str)
                .and_then(parse_rfc3339_ms)
            else {
                report.skipped_lines += 1;
                continue;
            };

            let mut role = None;
            let mut blocks = None;
            let mut message_text = None;
            let mut stop_reason = None;
            if matches!(record_type, "user" | "assistant") {
                let expected_role = record_type;
                let Some(message) = record.get("message").filter(|message| message.is_object())
                else {
                    report.skipped_lines += 1;
                    continue;
                };
                role = message.get("role").and_then(Value::as_str);
                if role != Some(expected_role) {
                    report.skipped_lines += 1;
                    continue;
                }
                let Some(content) = message.get("content").and_then(Value::as_array) else {
                    report.skipped_lines += 1;
                    continue;
                };
                if !content_blocks_are_valid(content) {
                    report.skipped_lines += 1;
                    continue;
                }
                stop_reason = match message.get("stop_reason") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(reason)) => Some(reason.as_str()),
                    Some(_) => {
                        report.skipped_lines += 1;
                        continue;
                    }
                };
                blocks = Some(content.as_slice());
                message_text = explicit_text(content);
            } else if record_type == "result"
                && record.get("is_error").and_then(Value::as_bool).is_none()
            {
                report.skipped_lines += 1;
                continue;
            }

            let explicit_turn_id = string_field(&record, &["turnId", "turn_id"]);
            let starts_turn =
                record_type == "user" && role == Some("user") && message_text.is_some();
            let turn_id = if starts_turn {
                explicit_turn_id.unwrap_or_else(|| event_id.clone())
            } else {
                explicit_turn_id
                    .or_else(|| current_turns.get(&session_id).cloned())
                    .unwrap_or_else(|| event_id.clone())
            };

            if starts_turn {
                current_turns.insert(session_id.clone(), turn_id.clone());
                report
                    .events
                    .push(event(&session_id, &turn_id, at_ms, EventKind::TurnStarted));
            }

            match record_type {
                "permission_request" | "approval_request" => report.events.push(event(
                    &session_id,
                    &turn_id,
                    at_ms,
                    EventKind::AttentionRequested { approval: true },
                )),
                "input_request" => report.events.push(event(
                    &session_id,
                    &turn_id,
                    at_ms,
                    EventKind::AttentionRequested { approval: false },
                )),
                "result" => {
                    if let Some(is_error) = record.get("is_error").and_then(Value::as_bool) {
                        report.events.push(event(
                            &session_id,
                            &turn_id,
                            at_ms,
                            if is_error {
                                EventKind::TurnFailed
                            } else {
                                EventKind::TurnSucceeded
                            },
                        ));
                    }
                }
                "error" => {
                    report
                        .events
                        .push(event(&session_id, &turn_id, at_ms, EventKind::TurnFailed))
                }
                _ => {}
            }

            if let Some(blocks) = blocks {
                for block in blocks {
                    match block.get("type").and_then(Value::as_str) {
                        Some("tool_use") | Some("server_tool_use") => report.events.push(event(
                            &session_id,
                            &turn_id,
                            at_ms,
                            EventKind::ToolStarted,
                        )),
                        Some("tool_result") => {
                            let is_error = block
                                .get("is_error")
                                .and_then(Value::as_bool)
                                .unwrap_or(false);
                            report.events.push(event(
                                &session_id,
                                &turn_id,
                                at_ms,
                                EventKind::ToolFinished { success: !is_error },
                            ));
                        }
                        _ => {}
                    }
                }
            }

            let message_role = match (record_type, role) {
                ("user", Some("user")) => Some(MessageRole::User),
                ("assistant", Some("assistant")) => Some(MessageRole::Assistant),
                _ => None,
            };
            if let (Some(role), Some(text)) = (message_role, message_text) {
                report.messages.push(ConversationMessage {
                    event_id,
                    at_ms,
                    role,
                    text,
                });
            }

            if record_type == "assistant" && stop_reason == Some("end_turn") {
                report.events.push(event(
                    &session_id,
                    &turn_id,
                    at_ms,
                    EventKind::TurnSucceeded,
                ));
            }
        }

        report
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

fn string_field(record: &Value, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| record.get(*name).and_then(Value::as_str))
        .map(str::to_owned)
}

fn content_blocks_are_valid(blocks: &[Value]) -> bool {
    blocks.iter().all(|block| {
        let Some(block_type) = block.get("type").and_then(Value::as_str) else {
            return false;
        };
        match block_type {
            "text" => block.get("text").and_then(Value::as_str).is_some(),
            "tool_result" => block
                .get("is_error")
                .map(|is_error| is_error.is_boolean())
                .unwrap_or(true),
            _ => true,
        }
    })
}

fn explicit_text(blocks: &[Value]) -> Option<String> {
    let text = blocks
        .iter()
        .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then_some(text)
}

fn event(session_id: &str, turn_id: &str, at_ms: u64, kind: EventKind) -> DomainEvent {
    DomainEvent {
        agent_id: "claude".to_owned(),
        session_id: session_id.to_owned(),
        turn_id: turn_id.to_owned(),
        at_ms,
        source: EventSource::ClaudeLog,
        confidence: Confidence::Confirmed,
        kind,
    }
}

#[cfg(test)]
mod tests {
    use super::ClaudeAdapter;
    use crate::adapters::AgentAdapter;
    use crate::domain::{EventKind, MessageRole};

    #[test]
    fn historical_tool_error_does_not_fail_new_turn() {
        let report = ClaudeAdapter.parse(include_str!(
            "../../tests/fixtures/claude/historical-tool-error.jsonl"
        ));

        assert!(matches!(
            report.events.last().unwrap().kind,
            EventKind::TurnSucceeded
        ));
        assert_eq!(
            report
                .messages
                .iter()
                .map(|message| message.role)
                .collect::<Vec<_>>(),
            [MessageRole::User, MessageRole::Assistant]
        );
    }

    #[test]
    fn current_turn_preserves_message_order_and_structured_success() {
        let report = ClaudeAdapter.parse(include_str!(
            "../../tests/fixtures/claude/current-turn.jsonl"
        ));

        assert_eq!(
            report
                .messages
                .iter()
                .map(|message| message.role)
                .collect::<Vec<_>>(),
            [MessageRole::User, MessageRole::Assistant]
        );
        assert!(matches!(
            report.events.last().unwrap().kind,
            EventKind::TurnSucceeded
        ));
    }

    #[test]
    fn terminal_words_in_message_text_do_not_create_terminal_events() {
        let input = concat!(
            r#"{"type":"user","sessionId":"claude-s1","uuid":"u1","timestamp":"2026-09-04T10:00:00Z","message":{"role":"user","content":[{"type":"text","text":"the previous run failed"}]}}"#,
            "\n",
            r#"{"type":"assistant","sessionId":"claude-s1","uuid":"a1","timestamp":"2026-09-04T10:00:01Z","message":{"role":"assistant","content":[{"type":"text","text":"error fixed, done and successful"}],"stop_reason":null}}"#,
        );

        let report = ClaudeAdapter.parse(input);

        assert!(!report
            .events
            .iter()
            .any(|event| matches!(event.kind, EventKind::TurnSucceeded | EventKind::TurnFailed)));
    }

    #[test]
    fn malformed_lines_are_skipped_without_reordering_valid_records() {
        let input = concat!(
            r#"{"type":"user","sessionId":"claude-s1","uuid":"u1","timestamp":"2026-09-04T10:00:00Z","message":{"role":"user","content":[{"type":"text","text":"first"}]}}"#,
            "\nnot-json\n",
            r#"{"type":"assistant","sessionId":"claude-s1","uuid":"a1","timestamp":"2026-09-04T10:00:01Z","message":{"role":"assistant","content":[{"type":"text","text":"second"}],"stop_reason":"end_turn"}}"#,
        );

        let report = ClaudeAdapter.parse(input);

        assert_eq!(report.skipped_lines, 1);
        assert_eq!(
            report
                .messages
                .iter()
                .map(|message| message.text.as_str())
                .collect::<Vec<_>>(),
            ["first", "second"]
        );
    }

    #[test]
    fn structured_fields_emit_tool_attention_and_terminal_failure_events() {
        let input = concat!(
            r#"{"type":"user","sessionId":"claude-s1","uuid":"u1","timestamp":"2026-09-04T10:00:00Z","message":{"role":"user","content":[{"type":"text","text":"run command"}]}}"#,
            "\n",
            r#"{"type":"assistant","sessionId":"claude-s1","uuid":"a1","timestamp":"2026-09-04T10:00:01Z","message":{"role":"assistant","content":[{"type":"tool_use","name":"Bash","input":{}}],"stop_reason":"tool_use"}}"#,
            "\n",
            r#"{"type":"permission_request","sessionId":"claude-s1","uuid":"p1","timestamp":"2026-09-04T10:00:02Z","approval":true}"#,
            "\n",
            r#"{"type":"result","sessionId":"claude-s1","uuid":"r1","timestamp":"2026-09-04T10:00:03Z","is_error":true}"#,
        );

        let report = ClaudeAdapter.parse(input);

        assert_eq!(
            report
                .events
                .iter()
                .map(|event| &event.kind)
                .collect::<Vec<_>>(),
            [
                &EventKind::TurnStarted,
                &EventKind::ToolStarted,
                &EventKind::AttentionRequested { approval: true },
                &EventKind::TurnFailed,
            ]
        );
    }

    #[test]
    fn explicit_turn_id_is_stable_across_start_tool_and_terminal_events() {
        let input = concat!(
            r#"{"type":"user","sessionId":"claude-s1","turnId":"turn-1","uuid":"u1","timestamp":"2026-09-04T10:00:00Z","message":{"role":"user","content":[{"type":"text","text":"run checks"}]}}"#,
            "\n",
            r#"{"type":"assistant","sessionId":"claude-s1","turnId":"turn-1","uuid":"a1","timestamp":"2026-09-04T10:00:01Z","message":{"role":"assistant","content":[{"type":"tool_result","is_error":false}],"stop_reason":"tool_use"}}"#,
            "\n",
            r#"{"type":"assistant","sessionId":"claude-s1","turnId":"turn-1","uuid":"a2","timestamp":"2026-09-04T10:00:02Z","message":{"role":"assistant","content":[{"type":"text","text":"done"}],"stop_reason":"end_turn"}}"#,
        );

        let report = ClaudeAdapter.parse(input);

        assert_eq!(report.events.len(), 3);
        assert!(report.events.iter().all(|event| event.turn_id == "turn-1"));
    }

    #[test]
    fn rfc3339_timestamps_preserve_milliseconds_and_apply_offsets() {
        let inputs = [
            (
                r#"{"type":"user","sessionId":"z","uuid":"u1","timestamp":"1970-01-01T00:00:00.980Z","message":{"role":"user","content":[{"type":"text","text":"z"}]}}"#,
                980,
            ),
            (
                r#"{"type":"user","sessionId":"plus","uuid":"u2","timestamp":"1970-01-01T08:00:00.250+08:00","message":{"role":"user","content":[{"type":"text","text":"plus"}]}}"#,
                250,
            ),
            (
                r#"{"type":"user","sessionId":"minus","uuid":"u3","timestamp":"1970-01-01T00:00:00.500-01:00","message":{"role":"user","content":[{"type":"text","text":"minus"}]}}"#,
                3_600_500,
            ),
        ];

        for (input, expected_ms) in inputs {
            let report = ClaudeAdapter.parse(input);

            assert_eq!(report.events[0].at_ms, expected_ms);
            assert_eq!(report.messages[0].at_ms, expected_ms);
        }
    }

    #[test]
    fn recognized_malformed_records_are_counted_once_and_later_lines_recover() {
        let malformed_records = [
            (
                "missing message",
                r#"{"type":"user","sessionId":"claude-s1","uuid":"bad","timestamp":"2026-09-04T10:00:00Z"}"#,
            ),
            (
                "wrong role",
                r#"{"type":"assistant","sessionId":"claude-s1","uuid":"bad","timestamp":"2026-09-04T10:00:00Z","message":{"role":"user","content":[]}}"#,
            ),
            (
                "non-array content",
                r#"{"type":"assistant","sessionId":"claude-s1","uuid":"bad","timestamp":"2026-09-04T10:00:00Z","message":{"role":"assistant","content":{}}}"#,
            ),
            (
                "text block without text",
                r#"{"type":"assistant","sessionId":"claude-s1","uuid":"bad","timestamp":"2026-09-04T10:00:00Z","message":{"role":"assistant","content":[{"type":"text"}]}}"#,
            ),
            (
                "tool result with non-boolean error flag",
                r#"{"type":"assistant","sessionId":"claude-s1","uuid":"bad","timestamp":"2026-09-04T10:00:00Z","message":{"role":"assistant","content":[{"type":"tool_result","is_error":"false"}]}}"#,
            ),
            (
                "result missing error flag",
                r#"{"type":"result","sessionId":"claude-s1","uuid":"bad","timestamp":"2026-09-04T10:00:00Z"}"#,
            ),
            (
                "result with non-boolean error flag",
                r#"{"type":"result","sessionId":"claude-s1","uuid":"bad","timestamp":"2026-09-04T10:00:00Z","is_error":"true"}"#,
            ),
            (
                "non-ascii malformed timestamp",
                r#"{"type":"result","sessionId":"claude-s1","uuid":"bad","timestamp":"1234é6-78T10:00:00Z","is_error":true}"#,
            ),
        ];
        let recovered = r#"{"type":"assistant","sessionId":"claude-s1","uuid":"ok","timestamp":"2026-09-04T10:00:01Z","message":{"role":"assistant","content":[{"type":"text","text":"recovered"}],"stop_reason":"end_turn"}}"#;

        for (case, malformed) in malformed_records {
            let report = ClaudeAdapter.parse(&format!("{malformed}\n{recovered}"));

            assert_eq!(report.skipped_lines, 1, "case: {case}");
            assert_eq!(report.messages.len(), 1, "case: {case}");
            assert_eq!(report.messages[0].text, "recovered", "case: {case}");
            assert!(matches!(
                report.events.last().map(|event| &event.kind),
                Some(EventKind::TurnSucceeded)
            ));
        }
    }
}
