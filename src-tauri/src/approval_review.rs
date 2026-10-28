//! Local, explainable review of an approval request.
//!
//! The approval card used to show only a command and a cwd. This module adds
//! the context a human actually needs to decide:
//! - a risk level + a one-line reason (rules only, no model call),
//! - the repository/branch when git can answer quickly,
//! - for Edit/Write, a small line diff preview built from the hook payload.
//!
//! Everything here is best-effort and local. A missing git binary or an
//! unexpected tool payload must degrade to "unknown/medium", never to a false
//! "low risk" signal.

use crate::*;
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

const MAX_DIFF_INPUT_LINES: usize = 200;
const MAX_DIFF_OUTPUT_LINES: usize = 80;
const MAX_PATH_CHARS: usize = 240;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RiskLevel {
    Low,
    Medium,
    High,
}

impl RiskLevel {
    pub(crate) fn severity(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "normal",
            Self::High => "high",
        }
    }

    fn rank(self) -> u8 {
        match self {
            Self::Low => 0,
            Self::Medium => 1,
            Self::High => 2,
        }
    }

    fn max(self, other: Self) -> Self {
        if other.rank() > self.rank() {
            other
        } else {
            self
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct DiffLine {
    /// ctx | add | del
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct DiffPreview {
    pub file: String,
    pub added: u32,
    pub removed: u32,
    pub lines: Vec<DiffLine>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ApprovalReview {
    pub risk: RiskLevel,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<DiffPreview>,
}

/// Review any tool payload we know how to read. Unknown tools stay at medium:
/// we do not pretend to understand them.
pub(crate) fn review_tool(tool: &str, input: &Value, cwd: &str) -> ApprovalReview {
    let repo = repo_root(cwd);
    let branch = branch_name(cwd);
    let lower = tool.trim().to_ascii_lowercase();
    match lower.as_str() {
        "bash" => {
            let command = input
                .get("command")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            command_review(command, repo, branch)
        }
        "edit" => edit_review(input, cwd, repo, branch),
        "write" => write_review(input, cwd, repo, branch),
        _ => ApprovalReview {
            risk: RiskLevel::Medium,
            reason: format!("未识别的工具（{tool}），请确认这次调用会做什么"),
            repo,
            branch,
            diff: None,
        },
    }
}

fn command_review(command: &str, repo: Option<String>, branch: Option<String>) -> ApprovalReview {
    let (risk, reason) = assess_command(command);
    ApprovalReview {
        risk,
        reason,
        repo,
        branch,
        diff: None,
    }
}

/// Rule table for shell commands. Order matters: the first matching row wins,
/// so destructive patterns are checked before their benign prefixes.
fn assess_command(command: &str) -> (RiskLevel, String) {
    let text = command.trim().to_lowercase();
    if text.is_empty() {
        return (RiskLevel::Medium, "未提供可评估的命令".to_string());
    }

    const HIGH: &[(&str, &str)] = &[
        ("rm -rf", "递归强制删除（rm -rf）"),
        ("rm -fr", "递归强制删除（rm -fr）"),
        ("del /f", "强制删除（del /f）"),
        ("rmdir /s", "递归删除目录（rmdir /s）"),
        ("git push --force", "强制推送（可能覆盖远端历史）"),
        ("git push -f", "强制推送（可能覆盖远端历史）"),
        ("git reset --hard", "硬重置（可能丢失本地修改）"),
        ("git clean -fd", "清理未跟踪文件（可能丢失文件）"),
        ("--production", "命令包含 --production，可能作用于生产环境"),
        ("npm publish", "发布 npm 包"),
        ("drop database", "删除数据库"),
        ("drop table", "删除数据表"),
        ("truncate table", "清空数据表"),
        ("terraform apply", "应用基础设施变更"),
        ("terraform destroy", "销毁基础设施"),
        ("kubectl delete", "删除 Kubernetes 资源"),
        ("format c:", "格式化磁盘"),
        ("shutdown", "关闭系统"),
        ("reg delete", "删除注册表项"),
        ("taskkill", "强制结束进程"),
        ("--no-verify", "跳过提交/推送校验"),
        ("| sh", "把下载内容直接交给 shell 执行"),
        ("| bash", "把下载内容直接交给 shell 执行"),
        ("| iex", "把下载内容直接交给 PowerShell 执行"),
        ("invoke-expression", "执行动态 PowerShell 表达式"),
    ];
    for (needle, reason) in HIGH {
        if text.contains(needle) {
            return (RiskLevel::High, (*reason).to_string());
        }
    }

    const MEDIUM: &[(&str, &str)] = &[
        ("npm install", "安装依赖（会写入 node_modules / lockfile）"),
        ("npm i ", "安装依赖（会写入 node_modules / lockfile）"),
        ("pnpm add", "安装依赖（会写入 lockfile）"),
        ("yarn add", "安装依赖（会写入 lockfile）"),
        ("pip install", "安装 Python 依赖"),
        ("cargo install", "安装 Rust 可执行文件"),
        ("winget install", "安装系统软件"),
        ("choco install", "安装系统软件"),
        ("git push", "推送到远端仓库"),
        ("git commit", "创建提交"),
        ("git merge", "合并分支"),
        ("git rebase", "改写提交历史"),
        ("git stash", "暂存/恢复工作区"),
        ("npm run", "执行 package.json 脚本"),
        ("docker ", "执行 Docker 命令"),
        ("kubectl ", "执行 Kubernetes 命令"),
        ("curl ", "发起网络请求"),
        ("wget ", "发起网络请求"),
        ("invoke-webrequest", "发起网络请求"),
        ("iwr ", "发起网络请求"),
        ("mv ", "移动/重命名文件"),
        ("copy ", "复制文件"),
        ("robocopy", "批量复制文件"),
        ("rm ", "删除文件"),
        ("del ", "删除文件"),
        ("rmdir", "删除目录"),
        (">", "重定向写入文件"),
    ];
    for (needle, reason) in MEDIUM {
        if text.contains(needle) {
            return (RiskLevel::Medium, (*reason).to_string());
        }
    }

    if classify_command(command) {
        return (RiskLevel::Low, "只读命令（命中内置安全清单）".to_string());
    }
    (
        RiskLevel::Medium,
        "未识别为只读命令，建议确认后再允许".to_string(),
    )
}

fn edit_review(
    input: &Value,
    cwd: &str,
    repo: Option<String>,
    branch: Option<String>,
) -> ApprovalReview {
    let file = input
        .get("file_path")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let old = input
        .get("old_string")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let new = input
        .get("new_string")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let (risk, mut reasons) = path_risk(file, cwd, repo.as_deref());
    if !old.is_empty() && new.trim().is_empty() {
        reasons.push("这次编辑会清空一段内容".to_string());
    }
    if looks_like_test(file) && new.trim().is_empty() {
        reasons.push("看起来在删除测试内容".to_string());
    }
    let risk = if reasons.len() > 1 {
        risk.max(RiskLevel::Medium)
    } else {
        risk
    };
    ApprovalReview {
        risk,
        reason: join_reasons(reasons, "编辑项目内文件"),
        repo,
        branch,
        diff: if file.is_empty() {
            None
        } else {
            Some(line_diff(file, old, new))
        },
    }
}

fn write_review(
    input: &Value,
    cwd: &str,
    repo: Option<String>,
    branch: Option<String>,
) -> ApprovalReview {
    let file = input
        .get("file_path")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let content = input
        .get("content")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let (mut risk, mut reasons) = path_risk(file, cwd, repo.as_deref());
    if content.trim().is_empty() {
        reasons.push("这次写入会清空文件".to_string());
        risk = risk.max(RiskLevel::Medium);
    }
    ApprovalReview {
        risk,
        reason: join_reasons(reasons, "写入项目内文件"),
        repo,
        branch,
        diff: if file.is_empty() {
            None
        } else {
            Some(write_preview(file, content))
        },
    }
}

fn join_reasons(mut reasons: Vec<String>, fallback: &str) -> String {
    reasons.dedup();
    if reasons.is_empty() {
        fallback.to_string()
    } else {
        reasons.join("；")
    }
}

fn looks_like_test(path: &str) -> bool {
    let lower = path.to_ascii_lowercase().replace('\\', "/");
    lower.contains("/test")
        || lower.contains("/tests/")
        || lower.contains(".test.")
        || lower.contains(".spec.")
        || lower.ends_with("_test.go")
}

fn path_risk(file: &str, cwd: &str, repo: Option<&str>) -> (RiskLevel, Vec<String>) {
    let mut risk = RiskLevel::Low;
    let mut reasons = Vec::new();
    if file.trim().is_empty() {
        return (RiskLevel::Medium, vec!["未提供目标文件".to_string()]);
    }
    let path = normalize_lexically(&resolve_path(file, cwd));
    let normalized = path
        .to_string_lossy()
        .to_ascii_lowercase()
        .replace('\\', "/");
    let boundary = repo
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            if cwd.trim().is_empty() {
                None
            } else {
                Some(PathBuf::from(cwd))
            }
        })
        .map(|value| normalize_lexically(&value));
    if let Some(boundary) = boundary {
        if !path.starts_with(&boundary) {
            risk = RiskLevel::High;
            reasons.push("目标文件在项目目录之外".to_string());
        }
    }
    if normalized.len() > MAX_PATH_CHARS {
        reasons.push("路径异常长".to_string());
        risk = risk.max(RiskLevel::Medium);
    }
    const SENSITIVE: &[(&str, &str)] = &[
        ("/.env", "触碰 .env 环境变量文件"),
        ("/.env.", "触碰 .env 环境变量文件"),
        ("id_rsa", "触碰 SSH 私钥"),
        ("id_ed25519", "触碰 SSH 私钥"),
        (".pem", "触碰证书/私钥文件"),
        (".key", "触碰密钥文件"),
        ("credential", "触碰凭据文件"),
        ("secret", "触碰 secret 文件"),
    ];
    for (needle, reason) in SENSITIVE {
        if normalized.contains(needle) {
            risk = RiskLevel::High;
            reasons.push((*reason).to_string());
            break;
        }
    }
    const MEDIUM: &[(&str, &str)] = &[
        ("/migrations/", "触碰数据库迁移"),
        ("/migration/", "触碰数据库迁移"),
        ("schema.sql", "触碰数据库 schema"),
        ("/.github/workflows/", "触碰 CI 工作流"),
        ("/.gitlab-ci", "触碰 CI 配置"),
        ("jenkinsfile", "触碰 CI 配置"),
        ("/package.json", "修改项目清单"),
        ("/cargo.toml", "修改项目清单"),
        ("package-lock.json", "修改 lockfile"),
        ("pnpm-lock.yaml", "修改 lockfile"),
        ("yarn.lock", "修改 lockfile"),
        ("cargo.lock", "修改 lockfile"),
        ("/dockerfile", "修改容器构建"),
        ("/docker-compose", "修改容器编排"),
        ("/auth/", "触碰认证/登录逻辑"),
        ("/login", "触碰登录逻辑"),
        ("session.ts", "触碰会话逻辑"),
        ("session/", "触碰会话逻辑"),
        ("/payment", "触碰支付逻辑"),
        ("/billing", "触碰计费逻辑"),
        ("/security", "触碰安全配置"),
        ("/permission", "触碰权限逻辑"),
    ];
    for (needle, reason) in MEDIUM {
        if normalized.contains(needle) {
            risk = risk.max(RiskLevel::Medium);
            reasons.push((*reason).to_string());
        }
    }
    if reasons.is_empty() {
        reasons.push("编辑项目内文件".to_string());
    }
    (risk, reasons)
}

fn normalize_lexically(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(Component::RootDir.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

fn resolve_path(file: &str, cwd: &str) -> PathBuf {
    let path = Path::new(file);
    if path.is_absolute() || cwd.trim().is_empty() {
        path.to_path_buf()
    } else {
        Path::new(cwd).join(path)
    }
}

fn split_lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .split('\n')
        .map(|line| line.to_string())
        .collect()
}

fn line_diff(file: &str, old: &str, new: &str) -> DiffPreview {
    let old_lines = split_lines(old);
    let new_lines = split_lines(new);
    let old_truncated = old_lines.len() > MAX_DIFF_INPUT_LINES;
    let new_truncated = new_lines.len() > MAX_DIFF_INPUT_LINES;
    let a = &old_lines[..old_lines.len().min(MAX_DIFF_INPUT_LINES)];
    let b = &new_lines[..new_lines.len().min(MAX_DIFF_INPUT_LINES)];

    // Small LCS: 200x200 cells is cheap and gives a much more honest preview
    // than "all old lines red, all new lines green".
    let mut lcs = vec![vec![0u32; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }

    let mut ops: Vec<(&'static str, String)> = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < a.len() && j < b.len() {
        if a[i] == b[j] {
            ops.push(("ctx", a[i].clone()));
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            ops.push(("del", a[i].clone()));
            i += 1;
        } else {
            ops.push(("add", b[j].clone()));
            j += 1;
        }
    }
    while i < a.len() {
        ops.push(("del", a[i].clone()));
        i += 1;
    }
    while j < b.len() {
        ops.push(("add", b[j].clone()));
        j += 1;
    }

    let added = ops.iter().filter(|(kind, _)| *kind == "add").count() as u32;
    let removed = ops.iter().filter(|(kind, _)| *kind == "del").count() as u32;
    let truncated = old_truncated || new_truncated || ops.len() > MAX_DIFF_OUTPUT_LINES;
    let lines = ops
        .into_iter()
        .take(MAX_DIFF_OUTPUT_LINES)
        .map(|(kind, text)| DiffLine {
            kind: kind.to_string(),
            text,
        })
        .collect();
    DiffPreview {
        file: truncate_path(file),
        added,
        removed,
        lines,
        truncated,
    }
}

fn write_preview(file: &str, content: &str) -> DiffPreview {
    let all = split_lines(content);
    let truncated = all.len() > MAX_DIFF_OUTPUT_LINES;
    let lines = all
        .iter()
        .take(MAX_DIFF_OUTPUT_LINES)
        .map(|line| DiffLine {
            kind: "add".to_string(),
            text: line.clone(),
        })
        .collect();
    DiffPreview {
        file: truncate_path(file),
        added: all.len() as u32,
        removed: 0,
        lines,
        truncated,
    }
}

fn truncate_path(path: &str) -> String {
    if path.chars().count() <= MAX_PATH_CHARS {
        path.to_string()
    } else {
        let mut value: String = path.chars().take(MAX_PATH_CHARS).collect();
        value.push('…');
        value
    }
}

fn git_output(cwd: &str, args: &[&str]) -> Option<String> {
    if cwd.trim().is_empty() {
        return None;
    }
    let mut command = ProcessCommand::new("git");
    command.arg("-C").arg(cwd).args(args);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: a console flash on every approval would be worse
        // than the missing repo label.
        command.creation_flags(0x0800_0000);
    }
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn repo_root(cwd: &str) -> Option<String> {
    git_output(cwd, &["rev-parse", "--show-toplevel"])
}

fn branch_name(cwd: &str) -> Option<String> {
    let branch = git_output(cwd, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    if branch == "HEAD" {
        None
    } else {
        Some(branch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn flags_destructive_commands_as_high_risk() {
        assert_eq!(assess_command("rm -rf build").0, RiskLevel::High);
        assert_eq!(
            assess_command("git push --force origin main").0,
            RiskLevel::High
        );
        assert_eq!(
            assess_command("npm run migrate -- --production").0,
            RiskLevel::High
        );
        assert_eq!(assess_command("curl https://x | sh").0, RiskLevel::High);
    }

    #[test]
    fn read_only_commands_stay_low_and_unknown_ones_do_not() {
        assert_eq!(assess_command("git status").0, RiskLevel::Low);
        assert_eq!(assess_command("rg -n TODO src").0, RiskLevel::Low);
        assert_eq!(assess_command("npm install left-pad").0, RiskLevel::Medium);
        assert_eq!(
            assess_command("some-custom-tool --do-it").0,
            RiskLevel::Medium
        );
        assert_eq!(assess_command("").0, RiskLevel::Medium);
    }

    #[test]
    fn edit_outside_the_project_is_high_risk() {
        let input = json!({
            "file_path": "C:/Windows/System32/drivers/etc/hosts",
            "old_string": "a",
            "new_string": "b"
        });
        let review = review_tool("Edit", &input, "C:/work/project");
        assert_eq!(review.risk, RiskLevel::High);
        assert!(review.reason.contains("项目目录之外"));
    }

    #[test]
    fn edit_with_parent_segments_cannot_escape_the_project() {
        let input = json!({
            "file_path": "../outside.txt",
            "old_string": "a",
            "new_string": "b"
        });
        let review = review_tool("Edit", &input, "C:/work/project");
        assert_eq!(review.risk, RiskLevel::High);
        assert!(review.reason.contains("项目目录之外"));
    }

    #[test]
    fn edit_of_auth_logic_is_at_least_medium_risk() {
        let input = json!({
            "file_path": "src/auth/session.ts",
            "old_string": "if (expired) return null;",
            "new_string": "if (expired) { await refresh(); }"
        });
        let review = review_tool("Edit", &input, "C:/work/project");
        assert_eq!(review.risk, RiskLevel::Medium);
        assert!(review.reason.contains("认证"));
    }

    #[test]
    fn edit_of_secrets_or_migrations_is_not_low_risk() {
        let secret = json!({
            "file_path": "C:/work/project/.env",
            "old_string": "A=1",
            "new_string": "A=2"
        });
        let review = review_tool("Edit", &secret, "C:/work/project");
        assert_eq!(review.risk, RiskLevel::High);
        assert!(review.reason.contains(".env"));

        let migration = json!({
            "file_path": "C:/work/project/migrations/001.sql",
            "old_string": "select 1",
            "new_string": "select 2"
        });
        let review = review_tool("Edit", &migration, "C:/work/project");
        assert_eq!(review.risk, RiskLevel::Medium);
        assert!(review.reason.contains("迁移"));
    }

    #[test]
    fn edit_preview_reports_added_and_removed_lines() {
        let input = json!({
            "file_path": "src/session.ts",
            "old_string": "if (expired) return null;",
            "new_string": "if (expired) {\n  await refresh();\n  return get();\n}"
        });
        let review = review_tool("Edit", &input, "C:/work/project");
        let diff = review.diff.expect("edit should have a diff");
        assert_eq!(diff.removed, 1);
        assert_eq!(diff.added, 4);
        assert!(diff.lines.iter().any(|line| line.kind == "del"));
        assert!(diff.lines.iter().any(|line| line.kind == "add"));
    }

    #[test]
    fn write_preview_marks_every_line_as_added() {
        let input = json!({
            "file_path": "src/new.ts",
            "content": "export const a = 1;\nexport const b = 2;\n"
        });
        let review = review_tool("Write", &input, "C:/work/project");
        let diff = review.diff.expect("write should have a preview");
        assert_eq!(diff.added, 3);
        assert_eq!(diff.removed, 0);
        assert!(diff.lines.iter().all(|line| line.kind == "add"));
    }

    #[test]
    fn unknown_tools_are_medium_not_low() {
        let review = review_tool("SomeMcpTool", &json!({ "x": 1 }), "");
        assert_eq!(review.risk, RiskLevel::Medium);
        assert!(review.reason.contains("未识别"));
    }

    #[test]
    fn diff_is_capped_instead_of_dumping_a_whole_file() {
        let old = (0..300)
            .map(|index| format!("old-{index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let new = (0..300)
            .map(|index| format!("new-{index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let diff = line_diff("big.txt", &old, &new);
        assert!(diff.truncated);
        assert_eq!(diff.lines.len(), MAX_DIFF_OUTPUT_LINES);
    }
}
