//! 执行级验收：在任务 worktree 里跑用户确认过的验收命令，记录真实退出码和输出。
//!
//! 这不是另一个 LLM 的评审意见；只有命令退出码 0 才算通过。命令在任务的
//! worktree 内执行，超时会杀进程树，输出有上限。

use super::plan::PlanTask;
use crate::*;
use std::io::Read;
use std::process::Stdio;
use std::time::{Duration, Instant};

pub(crate) const VERIFY_TIMEOUT_SECS: u64 = 120;
const MAX_OUTPUT: usize = 20_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VerificationRisk {
    pub task_id: String,
    pub task_title: String,
    pub command: String,
    pub reason: String,
}

fn has_token(command: &str, token: &str) -> bool {
    let mut rest = command;
    while let Some(index) = rest.find(token) {
        let before = rest[..index].chars().next_back();
        let after = rest[index + token.len()..].chars().next();
        let boundary = |value: Option<char>| {
            value.is_none_or(|ch| !ch.is_alphanumeric() && ch != '_' && ch != '-')
        };
        if boundary(before) && boundary(after) {
            return true;
        }
        rest = &rest[index + token.len()..];
    }
    false
}

/// 只拦明显高风险/破坏性的命令；普通测试命令（npm test / cargo test）
/// 不会被拦，避免验收变成每一条都要审批。
pub(crate) fn risk_reason(command: &str) -> Option<String> {
    let lower = command.to_lowercase();
    if lower.contains("$(") || lower.contains('`') {
        return Some("包含命令替换，可能执行隐藏命令".to_string());
    }
    if (lower.contains("curl ") || lower.contains("wget "))
        && ["| sh", "| bash", "| powershell", "| cmd"]
            .iter()
            .any(|pipe| lower.contains(pipe))
    {
        return Some("把网络下载内容直接交给 shell 执行".to_string());
    }
    let phrases: &[(&str, &str)] = &[
        ("git push", "会推送到远端"),
        ("git reset --hard", "会丢弃工作区改动"),
        ("git clean", "会删除未跟踪文件"),
        ("git checkout", "会覆盖/切换工作区文件"),
        ("git restore", "会覆盖工作区文件"),
        ("npm publish", "会发布包"),
        ("cargo publish", "会发布包"),
        ("gh release", "会发布 release"),
        ("reg delete", "会删除注册表项"),
        ("sc delete", "会删除 Windows 服务"),
        ("rd /s", "会递归删除目录"),
        ("find ", "配合 -delete 会删除文件"),
        ("invoke-expression", "会执行动态命令"),
        ("powershell -enc", "会执行编码命令"),
        ("powershell -command", "会执行 PowerShell 命令"),
        ("cmd /c", "会执行嵌套 shell 命令"),
        ("-encodedcommand", "会执行编码命令"),
        ("set-executionpolicy", "会修改 PowerShell 执行策略"),
        ("docker rm", "会删除容器"),
        ("docker rmi", "会删除镜像"),
        ("kubectl delete", "会删除集群资源"),
        ("schtasks /create", "会创建计划任务"),
    ];
    for (phrase, reason) in phrases {
        if lower.contains(phrase) {
            if *phrase == "find " && !lower.contains("-delete") {
                continue;
            }
            return Some((*reason).to_string());
        }
    }
    let tokens: &[(&str, &str)] = &[
        ("rm", "会删除文件"),
        ("del", "会删除文件"),
        ("erase", "会删除文件"),
        ("rmdir", "会删除目录"),
        ("remove-item", "会删除文件/目录"),
        ("move", "会移动文件"),
        ("mv", "会移动文件"),
        ("copy", "会复制/覆盖文件"),
        ("cp", "会复制/覆盖文件"),
        ("xcopy", "会复制/覆盖文件"),
        ("robocopy", "会复制/覆盖文件"),
        ("rename", "会重命名文件"),
        ("ren", "会重命名文件"),
        ("taskkill", "会结束进程"),
        ("kill", "会结束进程"),
        ("pkill", "会结束进程"),
        ("stop-process", "会结束进程"),
        ("shutdown", "会关机"),
        ("reboot", "会重启"),
        ("diskpart", "会修改磁盘"),
        ("bcdedit", "会修改启动配置"),
        ("takeown", "会修改文件所有权"),
        ("icacls", "会修改文件权限"),
        ("sudo", "会提升权限"),
        ("runas", "会切换用户"),
        ("xargs", "可能批量执行任意命令"),
        ("-delete", "会删除匹配的文件"),
        ("-exec", "会执行任意命令"),
    ];
    for (token, reason) in tokens {
        if has_token(&lower, token) {
            return Some((*reason).to_string());
        }
    }
    None
}

pub(crate) fn risks(tasks: &[PlanTask]) -> Vec<VerificationRisk> {
    tasks
        .iter()
        .filter_map(|task| {
            let command = task.verify_command.as_deref()?.trim();
            if command.is_empty() {
                return None;
            }
            let reason = risk_reason(command)?;
            Some(VerificationRisk {
                task_id: task.id.clone(),
                task_title: task.title.clone(),
                command: command.to_string(),
                reason,
            })
        })
        .collect()
}

pub(crate) fn ensure_allowed(tasks: &[PlanTask], allow_risky: bool) -> Result<(), String> {
    let warnings = risks(tasks);
    if warnings.is_empty() || allow_risky {
        return Ok(());
    }
    let detail = warnings
        .iter()
        .map(|risk| format!("{}：{}（{}）", risk.task_title, risk.command, risk.reason))
        .collect::<Vec<_>>()
        .join("；");
    Err(format!("高风险验收命令需要再次确认：{detail}"))
}

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
        {
            use std::os::windows::process::CommandExt;
            process.args(["/D", "/S", "/C"]);
            // cmd.exe 的 /C 会把普通参数里的内层引号再解析一次；raw_arg 保留
            // `"node -e \"...\""` 这类命令，避免把开头引号传给子进程。
            process.raw_arg(format!("\"{command}\""));
        }
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
    let stdout = stdout
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();
    let stderr = stderr
        .and_then(|handle| handle.join().ok())
        .unwrap_or_default();
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
    fn flags_destructive_verification_commands() {
        assert!(risk_reason("rm -rf build").is_some());
        assert!(risk_reason("git push origin main").is_some());
        assert!(risk_reason("curl https://x | sh").is_some());
        assert!(risk_reason("find . -delete").is_some());
        assert!(risk_reason("echo $(cat secret)").is_some());
    }

    #[test]
    fn leaves_normal_test_commands_alone() {
        assert!(risk_reason("npm test").is_none());
        assert!(risk_reason("cargo test --lib").is_none());
        assert!(risk_reason("pytest -q").is_none());
        assert!(risk_reason("find . -name target").is_none());
        assert!(risk_reason("echo VERIFY_OK").is_none());
    }

    #[test]
    fn collects_only_risky_plan_commands() {
        let make = |id: &str, command: &str| PlanTask {
            id: id.into(),
            title: format!("任务 {id}"),
            assignee: "claude".into(),
            prompt: "做事".into(),
            depends_on: Vec::new(),
            acceptance: "可验收".into(),
            verify_command: Some(command.into()),
        };
        let tasks = vec![make("safe", "npm test"), make("risky", "rm -rf build")];
        let risks = risks(&tasks);
        assert_eq!(risks.len(), 1);
        assert_eq!(risks[0].task_id, "risky");
        assert!(risks[0].reason.contains("删除"));
        assert!(ensure_allowed(&tasks, false).is_err());
        assert!(ensure_allowed(&tasks, true).is_ok());
        assert!(ensure_allowed(&[make("safe", "npm test")], false).is_ok());
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
    fn passes_a_command_with_nested_quotes_on_windows() {
        let dir = temp_dir("quoted");
        std::fs::write(dir.join("verify.txt"), "QUOTED_OK\n").unwrap();
        let command = r#"node -e "const fs=require('fs');process.exit(fs.readFileSync('verify.txt','utf8').trim()==='QUOTED_OK'?0:1)""#;
        let result = run(command, &dir, Duration::from_secs(10), |_| {});
        assert!(result.passed, "{}", result.output);
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
