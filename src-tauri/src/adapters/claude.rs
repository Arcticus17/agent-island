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
                .and_then(crate::parse_rfc3339_secs)
                .map(|seconds| seconds.saturating_mul(1_000))
            else {
                report.skipped_lines += 1;
                continue;
            };

            let message = record.get("message");
            let role = message
                .and_then(|message| message.get("role"))
                .and_then(Value::as_str);
            let blocks = message
                .and_then(|message| message.get("content"))
                .and_then(Value::as_array);
            let message_text = blocks.and_then(|blocks| explicit_text(blocks));

            if record_type == "user" && role == Some("user") && message_text.is_some() {
                current_turns.insert(session_id.clone(), event_id.clone());
                report
                    .events
                    .push(event(&session_id, &event_id, at_ms, EventKind::TurnStarted));
            }

            let turn_id = string_field(&record, &["turnId", "turn_id"])
                .or_else(|| current_turns.get(&session_id).cloned())
                .unwrap_or_else(|| event_id.clone());

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

            let stop_reason = message
                .and_then(|message| message.get("stop_reason"))
                .and_then(Value::as_str);
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

fn string_field(record: &Value, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| record.get(*name).and_then(Value::as_str))
        .map(str::to_owned)
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
}
