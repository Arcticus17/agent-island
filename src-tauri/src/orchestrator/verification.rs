//! 执行级验收：在任务 worktree 里跑用户确认过的验收命令，记录真实退出码和输出。
//!
//! 这不是另一个 LLM 的评审意见；只有命令退出码 0 才算通过。命令在任务的
//! worktree 内执行，超时会杀进程树，输出有上限。

use crate::*;
use std::io::Read;
use std::process::Stdio;
use std::time::{Duration, Instant};

pub(crate) const VERIFY_TIMEOUT_SECS: u64 = 120;
const MAX_OUTPUT: usize = 20_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TaskVerification {
    pub command: String,
    pub passed: bool,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub duration_ms: u64,
    pub output: String,
}

fn read_limited(mut reader: impl Read, limit: usize) -> String {
    let mut collected = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(count) => {
                if collected.len() < limit {
                    let remaining = limit - collected.len();
                    collected.extend_from_slice(&buffer[..count.min(remaining)]);
                }
            }
        }
    }
    String::from_utf8_lossy(&collected).trim().to_string()
}

fn cap_output(text: &str) -> String {
    if text.chars().count() <= MAX_OUTPUT {
        return text.trim().to_string();
    }
    let mut value: String = text.chars().take(MAX_OUTPUT).collect();
    value.push_str("\n…（验收输出已截断）");
    value
}

pub(crate) fn run(
    command: &str,
    cwd: &Path,
    timeout: Duration,
    on_spawn: impl FnOnce(u32),
) -> TaskVerification {
    let started = Instant::now();
    let mut child = {
        #[cfg(target_os = "windows")]
        let mut process = quiet_command("cmd.exe");
        #[cfg(not(target_os = "windows"))]
        let mut process = quiet_command("sh");
        #[cfg(target_os = "windows")]
        process.args(["/C", command]);
        #[cfg(not(target_os = "windows"))]
        process.args(["-c", command]);
        process
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        match process.spawn() {
            Ok(child) => child,
            Err(error) => {
                return TaskVerification {
                    command: command.to_string(),
                    passed: false,
                    exit_code: None,
                    timed_out: false,
                    duration_ms: started.elapsed().as_millis() as u64,
                    output: format!("无法启动验收命令：{error}"),
                };
            }
        }
    };
    let pid = child.id();
    on_spawn(pid);
    let stdout = child
        .stdout
        .take()
        .map(|reader| std::thread::spawn(move || read_limited(reader, MAX_OUTPUT)));
    let stderr = child
        .stderr
        .take()
        .map(|reader| std::thread::spawn(move || read_limited(reader, MAX_OUTPUT)));

    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if started.elapsed() >= timeout => {
                timed_out = true;
                runner::kill_process_tree(pid);
                let _ = child.kill();
                break child.wait().ok();
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => {
                let _ = child.kill();
                break child.wait().ok();
            }
        }
    };
    let stdout = stdout.and_then(|handle| handle.join().ok()).unwrap_or_default();
    let stderr = stderr.and_then(|handle| handle.join().ok()).unwrap_or_default();
    let mut output = String::new();
    if !stdout.is_empty() {
        output.push_str(&stdout);
    }
    if !stderr.is_empty() {
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str("[stderr]\n");
        output.push_str(&stderr);
    }
    let passed = !timed_out && status.as_ref().is_some_and(|status| status.success());
    let exit_code = status.and_then(|status| status.code());
    TaskVerification {
        command: command.to_string(),
        passed,
        exit_code,
        timed_out,
        duration_ms: started.elapsed().as_millis() as u64,
        output: cap_output(&output),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "agent-island-verify-{name}-{}-{}",
            std::process::id(),
            epoch_millis()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn passes_and_reports_output() {
        let dir = temp_dir("pass");
        let result = run("echo VERIFY_OK", &dir, Duration::from_secs(10), |_| {});
        assert!(result.passed, "{}", result.output);
        assert_eq!(result.exit_code, Some(0));
        assert!(result.output.contains("VERIFY_OK"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fails_on_non_zero_exit() {
        let dir = temp_dir("fail");
        let result = run("echo NOPE && exit 3", &dir, Duration::from_secs(10), |_| {});
        assert!(!result.passed);
        assert_eq!(result.exit_code, Some(3));
        assert!(result.output.contains("NOPE"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn times_out_and_kills_the_process() {
        let dir = temp_dir("timeout");
        let command = if cfg!(target_os = "windows") {
            "ping -n 10 127.0.0.1 > nul"
        } else {
            "sleep 5"
        };
        let result = run(command, &dir, Duration::from_millis(300), |_| {});
        assert!(!result.passed);
        assert!(result.timed_out);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
