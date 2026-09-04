use crate::adapters::{AgentAdapter, ParseReport};
use crate::domain::{Confidence, EventSource, SessionIdentity, SessionLifecycle};

/// Parses Hermes' session table. A table row identifies history, not live turn state.
pub struct HermesAdapter;

#[derive(Debug, Clone, Copy)]
struct TableHeader {
    project: usize,
    date: usize,
    session_id: usize,
    columns: usize,
}

impl AgentAdapter for HermesAdapter {
    fn parse(&self, text: &str) -> ParseReport {
        let mut report = ParseReport::default();
        let mut header = None;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || is_rule(line) {
                continue;
            }
            if header.is_none() {
                if let Some(parsed) = parse_header(line) {
                    header = Some(parsed);
                } else {
                    report.skipped_lines += 1;
                }
                continue;
            }
            let header = header.unwrap();
            let columns: Vec<&str> = line.split_whitespace().collect();
            if columns.len() != header.columns {
                report.skipped_lines += 1;
                continue;
            }
            let project = columns[header.project].trim_matches('"');
            let session_id = columns[header.session_id].trim_matches('"');
            if is_placeholder(project) || is_placeholder(session_id) {
                report.skipped_lines += 1;
                continue;
            }
            let Some(at_ms) = parse_date_ms(columns[header.date]) else {
                report.skipped_lines += 1;
                continue;
            };
            report.sessions.push(SessionIdentity {
                agent_id: "hermes".to_owned(),
                session_id: session_id.to_owned(),
                project_path: Some(project.to_owned()),
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

fn parse_header(line: &str) -> Option<TableHeader> {
    let raw: Vec<String> = line
        .split_whitespace()
        .map(normalize_header)
        .filter(|value| !value.is_empty())
        .collect();
    let mut columns = Vec::new();
    let mut index = 0;
    while index < raw.len() {
        if raw[index] == "session" && raw.get(index + 1).map(String::as_str) == Some("id") {
            columns.push("sessionid".to_owned());
            index += 2;
        } else {
            columns.push(raw[index].clone());
            index += 1;
        }
    }
    let project = columns.iter().position(|value| value == "project")?;
    let date = columns.iter().position(|value| value == "date")?;
    let session_id = columns.iter().position(|value| value == "sessionid")?;
    Some(TableHeader {
        project,
        date,
        session_id,
        columns: columns.len(),
    })
}

fn normalize_header(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn is_placeholder(value: &str) -> bool {
    value.is_empty()
        || matches!(
            value.to_ascii_lowercase().as_str(),
            "-" | "—" | "–" | "none" | "null" | "unknown"
        )
}

fn parse_date_ms(value: &str) -> Option<u64> {
    if !value.is_ascii() || value.len() != 10 {
        return None;
    }
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

    #[test]
    fn table_columns_are_header_mapped_and_malformed_rows_are_skipped() {
        let report = HermesAdapter.parse(
            "PROJECT DATE SESSION_ID\n\
             project-a 2026-09-04 hermes-1\n\
             project-b 2026-09-04 —\n\
             project-c 2026-13-99 hermes-3\n\
             project-d 2026-09-04 hermes-4 status\n",
        );

        assert_eq!(report.sessions.len(), 1);
        assert_eq!(report.sessions[0].session_id, "hermes-1");
        assert_eq!(report.skipped_lines, 3);
    }
}
