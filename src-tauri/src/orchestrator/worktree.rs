//! 每个团队任务一个 git worktree：worker 改隔离分支，主工作区不动；
//! 完成后由用户在团队页决定合并或丢弃。
//!
//! 设计边界：
//! - 只支持有至少一次提交的 git 仓库；非 git 直接拒绝，不偷偷写主工作区。
//! - worktree 从当前 HEAD 创建；主工作区未提交的改动不会进入 worktree，必须提示。
//! - worker 结束时把 worktree 里的改动提交到任务分支，便于 review / merge / discard。
//! - 合并要求主工作区干净；冲突时 abort merge，保留分支和 worktree 供人工处理。

use crate::*;
use super::team_run_dir;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub(crate) struct RepoInfo {
    pub root: PathBuf,
    pub branch: String,
    pub head: String,
    pub dirty: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct TaskWorktree {
    pub path: PathBuf,
    pub branch: String,
    pub base_commit: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct WorktreeChanges {
    pub files: Vec<String>,
    pub stat: String,
}

fn git(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let output = quiet_command("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .map_err(|error| format!("无法运行 git：{error}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if stderr.is_empty() {
            format!("git {} 失败", args.join(" "))
        } else {
            stderr
        })
    }
}

fn git_ok(cwd: &Path, args: &[&str]) -> bool {
    git(cwd, args).is_ok()
}

fn safe_component(input: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut value: String = input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    while value.contains("--") {
        value = value.replace("--", "-");
    }
    let value = value.trim_matches('-').to_string();
    if value.is_empty() {
        let digest = Sha256::digest(input.as_bytes());
        return format!("task-{:02x}{:02x}{:02x}{:02x}", digest[0], digest[1], digest[2], digest[3]);
    }
    if value.chars().count() <= 40 {
        value
    } else {
        let digest = Sha256::digest(input.as_bytes());
        format!(
            "{}-{:02x}{:02x}{:02x}",
            value.chars().take(30).collect::<String>(),
            digest[0],
            digest[1],
            digest[2]
        )
    }
}

pub(crate) fn task_branch(run_id: &str, task_id: &str) -> String {
    format!(
        "agent-island/{}/{}",
        safe_component(run_id),
        safe_component(task_id)
    )
}

fn worktree_path(run_id: &str, task_id: &str) -> PathBuf {
    team_run_dir(run_id)
        .join("worktrees")
        .join(safe_component(task_id))
}

/// 确认 workspace 是 git 仓库、已有 HEAD，并返回当前分支/提交与脏状态。
pub(crate) fn detect(workspace: &Path) -> Result<RepoInfo, String> {
    let root = git(workspace, &["rev-parse", "--show-toplevel"])
        .map_err(|_| "worktree 模式需要 git 仓库：请先 git init 并至少提交一次".to_string())?;
    let root = PathBuf::from(root);
    let head = git(&root, &["rev-parse", "HEAD"])
        .map_err(|_| "仓库还没有提交，worktree 无法创建".to_string())?;
    let branch = git(&root, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_else(|_| "HEAD".to_string());
    let dirty = !git(&root, &["status", "--porcelain"])
        .unwrap_or_default()
        .is_empty();
    Ok(RepoInfo {
        root,
        branch,
        head,
        dirty,
    })
}

fn remove_existing(repo_root: &Path, path: &Path, branch: &str) {
    let _ = git(repo_root, &["worktree", "remove", "--force", &path.to_string_lossy()]);
    let _ = git(repo_root, &["branch", "-D", branch]);
    let _ = fs::remove_dir_all(path);
}

pub(crate) fn create(
    repo: &RepoInfo,
    run_id: &str,
    task_id: &str,
) -> Result<TaskWorktree, String> {
    let path = worktree_path(run_id, task_id);
    let branch = task_branch(run_id, task_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建 worktree 目录失败：{error}"))?;
    }
    remove_existing(&repo.root, &path, &branch);
    if let Err(error) = git(
        &repo.root,
        &[
            "worktree",
            "add",
            "-b",
            &branch,
            &path.to_string_lossy(),
            &repo.head,
        ],
    ) {
        remove_existing(&repo.root, &path, &branch);
        return Err(format!("创建任务 worktree 失败：{error}"));
    }
    Ok(TaskWorktree {
        path,
        branch,
        base_commit: repo.head.clone(),
    })
}

/// 把 worker 留下的改动提交到任务分支，并返回文件清单/统计。
pub(crate) fn finalize(
    worktree: &TaskWorktree,
    title: &str,
) -> Result<WorktreeChanges, String> {
    git(&worktree.path, &["add", "-A"])?;
    let status = git(&worktree.path, &["status", "--porcelain"])?;
    if !status.is_empty() {
        let message = format!("agent-island: {}", title.trim());
        git(
            &worktree.path,
            &[
                "-c",
                "user.name=Agent Island",
                "-c",
                "user.email=agent-island@local",
                "commit",
                "-m",
                &message,
            ],
        )
        .map_err(|error| format!("提交任务改动失败：{error}"))?;
    }
    let files: Vec<String> = git(
        &worktree.path,
        &["diff", "--name-only", &format!("{}..HEAD", worktree.base_commit)],
    )
    .unwrap_or_default()
    .lines()
    .map(str::trim)
    .filter(|line| !line.is_empty())
    .map(str::to_string)
    .collect();
    let stat = git(
        &worktree.path,
        &["diff", "--stat", &format!("{}..HEAD", worktree.base_commit)],
    )
    .unwrap_or_default();
    Ok(WorktreeChanges {
        files,
        stat,
    })
}

pub(crate) fn diff(root: &Path, base_commit: &str, branch: &str) -> Result<String, String> {
    let text = git(
        root,
        &[
            "diff",
            "--no-color",
            &format!("{base_commit}..{branch}"),
        ],
    )?;
    const MAX: usize = 200_000;
    if text.len() > MAX {
        let mut capped = text.chars().take(MAX).collect::<String>();
        capped.push_str("\n…（diff 过长，已截断）");
        Ok(capped)
    } else {
        Ok(text)
    }
}

pub(crate) fn merge(repo_root: &Path, branch: &str) -> Result<(), String> {
    let dirty = git(repo_root, &["status", "--porcelain"]).unwrap_or_default();
    if !dirty.is_empty() {
        return Err("主工作区有未提交改动，先提交或暂存后再合并".to_string());
    }
    if git_ok(repo_root, &["merge-base", "--is-ancestor", branch, "HEAD"]) {
        return Ok(());
    }
    let result = git(
        repo_root,
        &[
            "-c",
            "user.name=Agent Island",
            "-c",
            "user.email=agent-island@local",
            "merge",
            "--no-ff",
            "--no-edit",
            branch,
        ],
    );
    match result {
        Ok(_) => Ok(()),
        Err(error) => {
            let _ = git(repo_root, &["merge", "--abort"]);
            Err(format!("合并失败（已 abort）：{error}"))
        }
    }
}

pub(crate) fn cleanup(repo_root: &Path, path: &Path, branch: &str) {
    remove_existing(repo_root, path, branch);
}

pub(crate) fn cleanup_after_merge(repo_root: &Path, path: &Path) {
    let _ = git(repo_root, &["worktree", "remove", "--force", &path.to_string_lossy()]);
    let _ = fs::remove_dir_all(path);
}

/// Edit/Write 只能落在任务 worktree 内。worktree 只改 cwd，绝对路径仍可能写到
/// 主工作区；这里按真实父目录做 canonicalize，挡住 `..` 和绝对路径逃逸。
pub(crate) fn path_within(root: &Path, file_path: &str) -> bool {
    let Ok(root) = root.canonicalize() else {
        return false;
    };
    let candidate = Path::new(file_path);
    let joined = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        root.join(candidate)
    };
    let resolved = if let Ok(canonical) = joined.canonicalize() {
        canonical
    } else {
        let Some(parent) = joined.parent() else {
            return false;
        };
        let Some(name) = joined.file_name() else {
            return false;
        };
        let Ok(parent) = parent.canonicalize() else {
            return false;
        };
        parent.join(name)
    };
    resolved.starts_with(&root)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_repo(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "agent-island-worktree-{tag}-{}-{}",
            std::process::id(),
            epoch_millis()
        ));
        fs::create_dir_all(&dir).unwrap();
        let run = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(args)
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?}");
        };
        run(&["init"]);
        run(&["-c", "user.name=Test", "-c", "user.email=test@local", "commit", "--allow-empty", "-m", "base"]);
        dir
    }

    #[test]
    fn creates_a_worktree_on_its_own_branch_and_merges_it() {
        let repo_dir = temp_repo("merge");
        let repo = detect(&repo_dir).unwrap();
        let worktree = create(&repo, "run-1", "task-a").unwrap();
        fs::write(worktree.path.join("hello.txt"), "hello\n").unwrap();
        let changes = finalize(&worktree, "add hello").unwrap();
        assert_eq!(changes.files, vec!["hello.txt"]);
        assert!(!repo_dir.join("hello.txt").exists(), "主工作区不应被直接修改");
        let diff = diff(&repo.root, &worktree.base_commit, &worktree.branch).unwrap();
        assert!(diff.contains("hello.txt"));

        merge(&repo.root, &worktree.branch).unwrap();
        assert_eq!(
            fs::read_to_string(repo_dir.join("hello.txt")).unwrap().trim(),
            "hello"
        );
        cleanup_after_merge(&repo.root, &worktree.path);
        let _ = fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn dirty_main_workspace_refuses_to_merge() {
        let repo_dir = temp_repo("dirty");
        let repo = detect(&repo_dir).unwrap();
        let worktree = create(&repo, "run-2", "task-b").unwrap();
        fs::write(worktree.path.join("a.txt"), "a\n").unwrap();
        finalize(&worktree, "add a").unwrap();
        fs::write(repo_dir.join("dirty.txt"), "dirty\n").unwrap();
        assert!(detect(&repo_dir).unwrap().dirty);
        let error = merge(&repo.root, &worktree.branch).unwrap_err();
        assert!(error.contains("未提交"), "{error}");
        cleanup(&repo.root, &worktree.path, &worktree.branch);
        let _ = fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn conflicting_merge_aborts_and_keeps_the_branch() {
        let repo_dir = temp_repo("conflict");
        fs::write(repo_dir.join("shared.txt"), "base\n").unwrap();
        let root = repo_dir.clone();
        git(&root, &["add", "-A"]).unwrap();
        git(
            &root,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@local",
                "commit",
                "-m",
                "shared base",
            ],
        )
        .unwrap();
        let repo = detect(&repo_dir).unwrap();
        let first = create(&repo, "run-3", "task-a").unwrap();
        let second = create(&repo, "run-3", "task-b").unwrap();
        fs::write(first.path.join("shared.txt"), "one\n").unwrap();
        finalize(&first, "one").unwrap();
        fs::write(second.path.join("shared.txt"), "two\n").unwrap();
        finalize(&second, "two").unwrap();
        merge(&repo.root, &first.branch).unwrap();

        let repo_after = detect(&repo_dir).unwrap();
        let error = merge(&repo_after.root, &second.branch).unwrap_err();
        assert!(error.contains("合并失败"), "{error}");
        assert_eq!(
            fs::read_to_string(repo_dir.join("shared.txt")).unwrap().trim(),
            "one"
        );
        assert!(git(&repo_after.root, &["branch", "--list", &second.branch]).is_ok());
        cleanup(&repo_after.root, &second.path, &second.branch);
        let _ = fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn detects_non_git_and_requires_a_commit() {
        let dir = std::env::temp_dir().join(format!("agent-island-not-git-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        assert!(detect(&dir).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn branch_names_are_safe_and_stable() {
        assert_eq!(task_branch("team-1", "t1"), "agent-island/team-1/t1");
        assert!(task_branch("team 1", "任务/一").starts_with("agent-island/team-1/task-"));
        assert!(task_branch("run", "a".repeat(80).as_str()).chars().count() < 60);
    }

    #[test]
    fn file_paths_cannot_escape_the_worktree() {
        let root = std::env::temp_dir().join(format!(
            "agent-island-path-{}-{}",
            std::process::id(),
            epoch_millis()
        ));
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("inside.txt"), "ok").unwrap();
        fs::write(root.join("sub").join("new.txt"), "ok").unwrap();
        assert!(path_within(&root, "inside.txt"));
        assert!(path_within(&root, "sub/new.txt"));
        assert!(path_within(&root, &root.join("sub").join("new.txt").to_string_lossy()));
        assert!(!path_within(&root, "../outside.txt"));
        assert!(!path_within(&root, "sub/../../outside.txt"));
        assert!(!path_within(&root, &root.parent().unwrap().join("outside.txt").to_string_lossy()));
        let _ = fs::remove_dir_all(&root);
    }
}
