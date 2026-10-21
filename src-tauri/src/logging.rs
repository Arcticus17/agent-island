//! 滚动日志：把关键事件写到 `~/.agent-island/logs/agent-island.log`，按大小轮换。
//!
//! 之前出问题只能靠用户口述——"它刚才自己抖了一下"这种描述没法复现。
//! 这里只记**事件**（启动、hook 判定、终端打开、更新探测），不记会话内容，
//! 避免把用户代码写进日志。

use crate::*;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// 单个日志文件上限：2 MB，够放几万行事件。
pub(crate) const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;
/// 保留的文件总数（当前 + 归档）。
pub(crate) const KEEP_FILES: usize = 3;

pub(crate) fn log_dir() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".agent-island")
        .join("logs")
}

pub(crate) fn log_path() -> PathBuf {
    log_dir().join("agent-island.log")
}

fn archive_path(path: &Path, index: usize) -> PathBuf {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("agent-island");
    path.with_file_name(format!("{stem}.{index}.log"))
}

/// 把 epoch 毫秒格式化成 UTC 的 `YYYY-MM-DDTHH:MM:SS.mmmZ`（不引第三方日期库）。
pub(crate) fn format_timestamp(epoch_ms: u64) -> String {
    let seconds = epoch_ms / 1_000;
    let millis = epoch_ms % 1_000;
    let days = (seconds / 86_400) as i64;
    let seconds_of_day = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        seconds_of_day / 3_600,
        (seconds_of_day % 3_600) / 60,
        seconds_of_day % 60,
    )
}

/// Howard Hinnant 的 civil_from_days：把"1970-01-01 起的天数"换算成日期。
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era = (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * mp + 2) / 5 + 1) as u32;
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

fn rotate_if_needed(path: &Path) -> std::io::Result<()> {
    let Ok(meta) = fs::metadata(path) else {
        return Ok(());
    };
    if meta.len() < MAX_LOG_BYTES {
        return Ok(());
    }
    // 归档依次后移：.2 → .3 之类，最后删掉超出保留数的那个
    for index in (1..KEEP_FILES).rev() {
        let from = archive_path(path, index);
        let to = archive_path(path, index + 1);
        if from.exists() {
            let _ = fs::rename(&from, &to);
        }
    }
    for index in KEEP_FILES..=KEEP_FILES + 1 {
        let extra = archive_path(path, index);
        if extra.exists() {
            let _ = fs::remove_file(&extra);
        }
    }
    fs::rename(path, archive_path(path, 1))
}

/// 追加一行日志。写失败不应该影响主流程，所以调用方可以忽略返回值。
pub(crate) fn write_log_line(path: &Path, level: &str, scope: &str, message: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    rotate_if_needed(path)?;
    let line = format!("{} [{level}] {scope}: {message}\n", format_timestamp(epoch_millis()));
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(line.as_bytes())
}

/// 记录一条事件；日志路径取默认位置。
pub(crate) fn log_event(level: &str, scope: &str, message: &str) {
    let _ = write_log_line(&log_path(), level, scope, message);
}

/// 读最近的日志（新的在前），给"导出诊断"和排查用。
pub(crate) fn read_recent(path: &Path, limit: usize) -> Vec<String> {
    let Ok(raw) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut lines: Vec<String> = raw.lines().map(|line| line.to_string()).collect();
    if lines.len() > limit {
        lines = lines.split_off(lines.len() - limit);
    }
    lines.reverse();
    lines
}

#[tauri::command]
pub(crate) fn read_recent_logs(limit: Option<usize>) -> Vec<String> {
    read_recent(&log_path(), limit.unwrap_or(50).clamp(1, 200))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("agent-island-log-{tag}-{}", epoch_millis()))
    }

    #[test]
    fn formats_utc_timestamps() {
        assert_eq!(format_timestamp(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(format_timestamp(1_700_000_000_000), "2023-11-14T22:13:20.000Z");
        assert_eq!(format_timestamp(1_700_000_000_123), "2023-11-14T22:13:20.123Z");
        // 闰日
        assert_eq!(format_timestamp(1_709_164_800_000), "2024-02-29T00:00:00.000Z");
    }

    #[test]
    fn appends_lines_with_level_and_scope() {
        let dir = temp_dir("append");
        let path = dir.join("agent-island.log");
        write_log_line(&path, "INFO", "hook", "allowed npm test").unwrap();
        write_log_line(&path, "WARN", "scan", "skipped unreadable session").unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = raw.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("[INFO] hook: allowed npm test"));
        assert!(lines[1].contains("[WARN] scan: skipped unreadable session"));
        let _ = fs::remove_dir_all(dir);
    }

    fn fill_to_limit(path: &Path) {
        let filler = "x".repeat(64 * 1024);
        let mut guard = 0;
        loop {
            let size = fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
            if size >= MAX_LOG_BYTES || guard > 200 {
                break;
            }
            write_log_line(path, "INFO", "bulk", &filler).unwrap();
            guard += 1;
        }
    }

    #[test]
    fn rotates_when_the_file_grows_past_the_limit() {
        let dir = temp_dir("rotate");
        let path = dir.join("agent-island.log");

        fill_to_limit(&path);
        write_log_line(&path, "INFO", "rotate", "after rotation").unwrap();
        assert!(dir.join("agent-island.1.log").exists(), "旧日志应该被归档");
        let current = fs::read_to_string(&path).unwrap();
        assert!(current.contains("after rotation"));
        assert_eq!(current.lines().count(), 1, "轮换后当前文件只应有刚写的一行");

        // 再转几次，归档数量不应无限增长
        for _ in 0..KEEP_FILES + 1 {
            fill_to_limit(&path);
            write_log_line(&path, "INFO", "rotate", "again").unwrap();
        }
        let archives = (1..=KEEP_FILES + 3)
            .filter(|index| dir.join(format!("agent-island.{index}.log")).exists())
            .count();
        assert!(dir.join("agent-island.1.log").exists());
        assert!(
            archives <= KEEP_FILES,
            "归档文件不应超过 {KEEP_FILES} 个，实际 {archives}"
        );
        let _ = fs::remove_dir_all(dir);
    }
    #[test]
    fn reads_the_most_recent_lines_first() {
        let dir = temp_dir("read");
        let path = dir.join("agent-island.log");
        for index in 0..5 {
            write_log_line(&path, "INFO", "read", &format!("line {index}")).unwrap();
        }
        let recent = read_recent(&path, 2);
        assert_eq!(recent.len(), 2);
        assert!(recent[0].contains("line 4"), "最新的在最前");
        assert!(recent[1].contains("line 3"));
        assert!(read_recent(&dir.join("missing.log"), 5).is_empty());
        let _ = fs::remove_dir_all(dir);
    }
}

