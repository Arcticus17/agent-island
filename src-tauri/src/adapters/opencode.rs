use crate::adapters::{AgentAdapter, ParseReport};
use crate::domain::{
    Confidence, ConversationMessage, DomainEvent, EventKind, EventSource, MessageRole,
};

/// Normalizes OpenCode's human-readable log without treating text as a turn result.
pub struct OpenCodeAdapter;

impl AgentAdapter for OpenCodeAdapter {
    fn parse(&self, text: &str) -> ParseReport {
        let mut report = ParseReport::default();
        for (line_number, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let Some(message) = field(line, "message") else {
                report.skipped_lines += 1;
                continue;
            };
            let Some(at_ms) = timestamp(line) else {
                report.skipped_lines += 1;
                continue;
            };
            let message = message.trim().trim_matches('"');
            if message.is_empty() {
                report.skipped_lines += 1;
                continue;
            }
            report.messages.push(ConversationMessage {
                event_id: format!("opencode-line-{}", line_number + 1),
                at_ms,
                role: MessageRole::Assistant,
                text: truncate(message, 240),
            });
            // Text can explain a diagnostic, but only structured evidence may finish a turn.
            if let Some(code) = text_signal_code(message) {
                report.events.push(DomainEvent {
                    agent_id: "opencode".to_owned(),
                    session_id: "opencode".to_owned(),
                    turn_id: "unknown".to_owned(),
                    at_ms,
                    source: EventSource::Process,
                    confidence: Confidence::Unknown,
                    kind: EventKind::DiagnosticHint {
                        code: code.to_owned(),
                    },
                });
            }
        }
        report
    }
}

fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let marker = format!("{key}=");
    let start = line.find(&marker)? + marker.len();
    let value = &line[start..];
    if value.starts_with('"') {
        let end = value[1..].find('"')? + 1;
        Some(&value[1..end])
    } else if key == "message" {
        Some(value.trim())
    } else {
        Some(value.split_whitespace().next().unwrap_or_default())
    }
}

fn timestamp(line: &str) -> Option<u64> {
    field(line, "timestamp")
        .or_else(|| field(line, "time"))
        .and_then(parse_rfc3339_ms)
        .or_else(|| line.split_whitespace().find_map(parse_rfc3339_ms))
}

fn text_signal_code(text: &str) -> Option<&'static str> {
    let lower = text.to_ascii_lowercase();
    if [
        "error:",
        "error occurred",
        "failed to",
        "exception",
        "panic",
        "traceback",
        "is_error",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
    {
        return Some("error");
    }
    if [
        "waiting for",
        "awaiting",
        "permission required",
        "approval",
        "confirm",
        "y/n",
        "yes/no",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
    {
        return Some("waiting");
    }
    if ["completed", "finished", "successfully", "success"]
        .iter()
        .any(|needle| lower.contains(needle))
        && !lower.contains("not done")
        && !lower.contains("undone")
    {
        return Some("done");
    }
    None
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let mut output: String = text.chars().take(max_chars).collect();
    output.push_str("...");
    output
}

fn parse_rfc3339_ms(timestamp: &str) -> Option<u64> {
    let (date, time) = timestamp.split_once('T')?;
    if date.len() != 10 || &date[4..5] != "-" || &date[7..8] != "-" {
        return None;
    }
    let year = date[0..4].parse::<i64>().ok()?;
    let month = date[5..7].parse::<u32>().ok()?;
    let day = date[8..10].parse::<u32>().ok()?;
    if day == 0 || day > days_in_month(year, month)? {
        return None;
    }
    let (clock, offset) = if let Some(clock) = time.strip_suffix('Z') {
        (clock, 0_i64)
    } else {
        let index = time.rfind(['+', '-'])?;
        let (clock, zone) = time.split_at(index);
        (clock, parse_offset(zone)?)
    };
    let mut parts = clock.split(':');
    let hour = two_digits(parts.next()?)?;
    let minute = two_digits(parts.next()?)?;
    let seconds = parts.next()?;
    if parts.next().is_some() || hour > 23 || minute > 59 {
        return None;
    }
    let (second_text, fraction) = seconds
        .split_once('.')
        .map_or((seconds, None), |(second, fraction)| {
            (second, Some(fraction))
        });
    let second = two_digits(second_text)?;
    if second > 59 {
        return None;
    }
    let milliseconds = match fraction {
        Some(fraction) => fractional_ms(fraction)?,
        None => 0,
    };
    let local = i128::from(days_from_civil(year, month, day)) * 86_400_000
        + i128::from(hour) * 3_600_000
        + i128::from(minute) * 60_000
        + i128::from(second) * 1_000
        + i128::from(milliseconds);
    u64::try_from(local - i128::from(offset) * 60_000).ok()
}

fn two_digits(text: &str) -> Option<u32> {
    (text.len() == 2 && text.bytes().all(|b| b.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

fn fractional_ms(text: &str) -> Option<u32> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut value = 0;
    for (index, byte) in text.bytes().take(3).enumerate() {
        value += u32::from(byte - b'0') * [100, 10, 1][index];
    }
    Some(value)
}

fn parse_offset(text: &str) -> Option<i64> {
    if text.len() != 6 || &text[3..4] != ":" {
        return None;
    }
    let sign = match &text[0..1] {
        "+" => 1_i64,
        "-" => -1_i64,
        _ => return None,
    };
    let hours = two_digits(&text[1..3])?;
    let minutes = two_digits(&text[4..6])?;
    (hours <= 23 && minutes <= 59).then_some(sign * i64::from(hours * 60 + minutes))
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
    let month = i64::from(month) + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::OpenCodeAdapter;
    use crate::adapters::AgentAdapter;
    use crate::domain::EventKind;

    #[test]
    fn text_error_is_only_a_diagnostic_hint() {
        let report =
            OpenCodeAdapter.parse(include_str!("../../tests/fixtures/opencode/text-error.log"));
        assert!(report
            .events
            .iter()
            .any(|event| matches!(event.kind, EventKind::DiagnosticHint { .. })));
        assert!(!report
            .events
            .iter()
            .any(|event| matches!(event.kind, EventKind::TurnFailed | EventKind::TurnSucceeded)));
    }
}
