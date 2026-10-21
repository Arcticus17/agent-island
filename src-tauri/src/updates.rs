//! Agent CLI 版本探测。
//!
//! 本地只做一件不需要联网的事：跑 `<cli> --version` 把版本号抓出来。
//! "最新版本是多少" 由前端直接 `fetch` npm registry 拿（WebView 有网，registry 也发 CORS 头），
//! 这样岛里既打通了网络，又不用在 Rust 侧背一个 HTTP 客户端。

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliVersionProbe {
    /// 被探测的命令
    pub program: String,
    /// 解析出的第一个 semver（拿不到就是 None）
    pub version: Option<String>,
    /// 命令原始输出的前几行，探测失败时给用户看
    pub raw: String,
    /// 命令是否真的跑起来了
    pub ok: bool,
}

/// 只允许探测已知 Agent CLI，避免变成任意命令执行入口。
fn probe_allowed(program: &str) -> bool {
    let lower = program.to_lowercase();
    let base = lower
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(&lower)
        .trim_end_matches(".exe")
        .trim_end_matches(".cmd")
        .trim_end_matches(".bat")
        .to_string();
    matches!(base.as_str(), "claude" | "codex" | "opencode" | "hermes")
}

/// 从任意输出里挑出第一个 `x.y.z`，允许 `v` 前缀和 `-beta.1` 这类后缀。
pub(crate) fn parse_version(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index].is_ascii_digit() {
            let start = index;
            let mut cursor = index;
            let mut parts = 0;
            loop {
                while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
                    cursor += 1;
                }
                parts += 1;
                if parts == 3 {
                    break;
                }
                if cursor < bytes.len() && bytes[cursor] == b'.' {
                    cursor += 1;
                } else {
                    parts = 0;
                    break;
                }
            }
            if parts == 3 {
                let mut end = cursor;
                // 带上 prerelease / build 后缀（-beta.3、+build）
                if end < bytes.len() && (bytes[end] == b'-' || bytes[end] == b'+') {
                    end += 1;
                    while end < bytes.len()
                        && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'.' || bytes[end] == b'-')
                    {
                        end += 1;
                    }
                }
                let candidate = &text[start..end];
                if candidate.split('.').take(3).all(|part| part.chars().any(|c| c.is_ascii_digit())) {
                    return Some(candidate.to_string());
                }
            }
            index = start + 1;
        } else {
            index += 1;
        }
    }
    None
}

fn capture(program: &str, args: &[&str], timeout: Duration) -> (bool, String) {
    // npm 装的 CLI 多数是 .cmd shim，必须过 cmd.exe 才能执行（和内置终端的处理一致）
    let mut command = Command::new("cmd");
    command
        .arg("/C")
        .arg(format!("{} {}", program, args.join(" ")))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => return (false, String::new()),
    };
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if started.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return (false, "探测超时".to_string());
                }
                std::thread::sleep(Duration::from_millis(60));
            }
            Err(_) => return (false, String::new()),
        }
    }
    let output = child.wait_with_output().ok();
    let mut text = String::new();
    if let Some(output) = output {
        text.push_str(&String::from_utf8_lossy(&output.stdout));
        text.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    let trimmed: String = text.lines().take(3).collect::<Vec<_>>().join("\n");
    (true, trimmed)
}

#[tauri::command]
pub async fn probe_cli_version(program: String) -> CliVersionProbe {
    if !probe_allowed(&program) {
        return CliVersionProbe { program, version: None, raw: "不支持的探测目标".into(), ok: false };
    }
    let target = program.clone();
    let joined = tauri::async_runtime::spawn_blocking(move || {
        // .cmd/.ps1 shim 需要过 cmd.exe 才能执行
        let (ok, raw) = capture(&target, &["--version"], Duration::from_secs(8));
        (ok, raw)
    })
    .await;
    let (ok, raw) = joined.unwrap_or((false, String::new()));
    let version = parse_version(&raw);
    CliVersionProbe { program, version, raw, ok }
}

#[cfg(test)]
mod tests {
    use super::parse_version;

    #[test]
    fn parses_plain_and_prefixed_versions() {
        assert_eq!(parse_version("0.163.0"), Some("0.163.0".into()));
        assert_eq!(parse_version("codex-cli 0.163.0"), Some("0.163.0".into()));
        assert_eq!(parse_version("v2.1.306 (Claude Code)"), Some("2.1.306".into()));
    }

    #[test]
    fn keeps_prerelease_suffix() {
        assert_eq!(parse_version("1.0.0-beta.3"), Some("1.0.0-beta.3".into()));
    }

    #[test]
    fn ignores_text_without_semver() {
        assert_eq!(parse_version("command not found"), None);
        assert_eq!(parse_version("12 apples"), None);
    }
}
