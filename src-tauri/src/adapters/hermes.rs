use crate::adapters::{AgentAdapter, ParseReport};
use crate::domain::{Confidence, EventSource, SessionIdentity, SessionLifecycle};

/// Parses Hermes' session table. A table row identifies history, not live turn state.
pub struct HermesAdapter;

impl AgentAdapter for HermesAdapter {
    fn parse(&self, text: &str) -> ParseReport {
        let mut report = ParseReport::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || is_rule(line) {
                continue;
            }
            let columns: Vec<&str> = line.split_whitespace().collect();
            let Some(date_index) = columns.iter().position(|value| is_date(value)) else {
                continue;
            };
            if date_index == 0 || columns.len() <= date_index + 1 {
                report.skipped_lines += 1;
                continue;
            }
            let project = columns[..date_index].join(" ");
            let Some(session_id) = columns.last().map(|value| value.trim_matches('"')) else {
                report.skipped_lines += 1;
                continue;
            };
            let session_id = session_id.to_owned();
            if project.is_empty() || session_id.is_empty() {
                report.skipped_lines += 1;
                continue;
            }
            let Some(at_ms) = parse_date_ms(columns[date_index]) else {
                report.skipped_lines += 1;
                continue;
            };
            report.sessions.push(SessionIdentity {
                agent_id: "hermes".to_owned(),
                session_id,
                project_path: Some(project.trim_matches('"').to_owned()),
                process_ids: Vec::new(),
                started_at_ms: at_ms,
                last_event_at_ms: at_ms,
                source: EventSource::Process,
                confidence: Confidence::Unknown,
                lifecycle: SessionLifecycle::Historical,
            });
        }
        report
    }
}

fn is_rule(line: &str) -> bool {
    line.chars().all(|character| {
        character == '-' || character == '—' || character == '─' || character.is_whitespace()
    })
}

fn is_date(value: &str) -> bool {
    value.len() == 10
        && value.as_bytes()[4] == b'-'
        && value.as_bytes()[7] == b'-'
        && parse_date_ms(value).is_some()
}

fn parse_date_ms(value: &str) -> Option<u64> {
    let year = value[0..4].parse::<i64>().ok()?;
    let month = value[5..7].parse::<u32>().ok()?;
    let day = value[8..10].parse::<u32>().ok()?;
    if day == 0 || day > days_in_month(year, month)? {
        return None;
    }
    u64::try_from(i128::from(days_from_civil(year, month, day)) * 86_400_000).ok()
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
    use super::HermesAdapter;
    use crate::adapters::AgentAdapter;
    use crate::domain::EventKind;

    #[test]
    fn session_table_yields_historical_identity_without_terminal_event() {
        let report =
            HermesAdapter.parse(include_str!("../../tests/fixtures/hermes/session-list.txt"));
        assert_eq!(report.sessions.len(), 1);
        assert_eq!(report.sessions[0].session_id, "hermes-1");
        assert_eq!(
            report.sessions[0].lifecycle,
            crate::domain::SessionLifecycle::Historical
        );
        assert!(!report
            .events
            .iter()
            .any(|event| matches!(event.kind, EventKind::TurnFailed | EventKind::TurnSucceeded)));
    }
}
