//! 团队任务：主 Agent 规划、岛派发、串行执行、汇总与持久化。
//!
//! 设计边界（骨架阶段）：
//! - 主 Agent 只在"规划"和"汇总"两次被调用；中间过程由岛掌控。
//! - 同一工作区默认串行写入；任务按依赖 DAG 取第一个就绪项。
//! - 成功 = 进程退出 0 且拿到结构化结果；否则失败/未验证，不猜。
//! - 团队任务的子进程 pid 报告给 AppState，由主岛快照过滤掉。

pub(crate) mod capability;
pub(crate) mod plan;
pub(crate) mod runner;
pub(crate) mod scheduler;
pub(crate) mod verification;
pub(crate) mod worktree;

use crate::*;
use plan::{PlanTask, TeamPlan};
use runner::{ProcessOutcome, RunnerConfig};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

const MAX_PERSISTED_RUNS: usize = 10;
const MAX_RUNS_BYTES: usize = 2 * 1024 * 1024;
const TASK_TIMEOUT_SECS: u64 = 600;
const PLANNER_TIMEOUT_SECS: u64 = 240;
const SYNTHESIS_TIMEOUT_SECS: u64 = 300;

static ID_COUNTER: AtomicU64 = AtomicU64::new(1);

fn next_id(prefix: &str) -> String {
    let counter = ID_COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("{prefix}-{}-{counter}", epoch_millis())
}

fn cap_text(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        text.to_string()
    } else {
        let mut value: String = text.chars().take(limit).collect();
        value.push('…');
        value
    }
}

fn new_uuid() -> String {    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(epoch_millis().to_le_bytes());
    hasher.update(std::process::id().to_le_bytes());
    hasher.update(ID_COUNTER.fetch_add(1, Ordering::SeqCst).to_le_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RunPhase {
    Planning,
    AwaitingPlan,
    Running,
    Summarizing,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TaskState {
    Queued,
    Running,
    WaitingApproval,
    Succeeded,
    Failed,
    Cancelled,
    Skipped,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MergeState {
    #[default]
    None,
    Pending,
    Merged,
    Discarded,
    Conflict,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TeamTaskView {
    pub id: String,
    pub title: String,
    pub assignee: String,
    pub assignee_name: String,
    pub prompt: String,
    pub depends_on: Vec<String>,
    pub acceptance: String,
    /// 计划里声明的验收命令；None 表示不跑执行级验收。
    #[serde(default)]
    pub verify_command: Option<String>,
    /// 最近一次验收结果；旧记录/未配置时 None。
    #[serde(default)]
    pub verification: Option<verification::TaskVerification>,
    pub state: TaskState,
    pub started_at_ms: Option<u64>,
    pub ended_at_ms: Option<u64>,
    pub exit_code: Option<i32>,
    pub result: Option<String>,
    pub error: Option<String>,
    pub cost_usd: Option<f64>,
    pub log: Vec<String>,
    pub session_id: Option<String>,
    pub pending_approval: Option<String>,
    pub approval_id: Option<String>,
    #[serde(default)]
    pub worktree_path: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub base_commit: Option<String>,
    #[serde(default)]
    pub changed_files: Vec<String>,
    #[serde(default)]
    pub diff_stat: Option<String>,
    #[serde(default)]
    pub merge_state: MergeState,
    #[serde(default)]
    pub merge_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TeamRunView {
    pub id: String,
    pub goal: String,
    pub workspace: String,
    pub main_agent: String,
    pub main_agent_name: String,
    pub phase: RunPhase,
    pub plan_summary: Option<String>,
    pub summary: Option<String>,
    pub tasks: Vec<TeamTaskView>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub error: Option<String>,
    pub planner_session_id: Option<String>,
    /// 本轮 USD 预算；None 表示不设上限（旧记录默认 None）。
    #[serde(default)]
    pub budget_usd: Option<f64>,
    /// 已经花掉的 USD（规划 + worker + 汇总）。
    #[serde(default)]
    pub spent_usd: f64,
    /// git worktree 模式的仓库根、基分支与基提交。
    #[serde(default)]
    pub repo_root: Option<String>,
    #[serde(default)]
    pub base_branch: Option<String>,
    #[serde(default)]
    pub base_commit: Option<String>,
    #[serde(default)]
    pub worktree_mode: bool,
    /// 主工作区脏等需要用户知道、但不阻塞启动的提醒。
    #[serde(default)]
    pub workspace_warning: Option<String>,
    /// 同时执行的独立任务数（旧记录默认 1）。
    #[serde(default = "default_concurrency")]
    pub concurrency: u32,
    /// 多轮规划：上一轮的 run id；首轮为 None。
    #[serde(default)]
    pub parent_run_id: Option<String>,
    #[serde(default = "default_round")]
    pub round: u32,
}

fn default_round() -> u32 {
    1
}

fn default_concurrency() -> u32 {
    1
}

pub(crate) struct TeamState {
    pub runs: Mutex<HashMap<String, TeamRunView>>,
    /// task_key（run_id:task_id）→ 子进程 pid，用于取消与主岛过滤。
    pub active: Mutex<HashMap<String, u32>>,
    /// 团队任务正在使用的 Claude session → 所属任务。
    pub managed_sessions: Mutex<HashMap<String, ManagedSession>>,
}

#[derive(Debug, Clone)]
pub(crate) struct ManagedSession {
    pub run_id: String,
    pub task_id: String,
    pub worktree_root: PathBuf,
}

fn team_runs_path() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".agent-island")
        .join("team-runs.json")
}

fn team_run_dir(run_id: &str) -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".agent-island")
        .join("team-runs")
        .join(run_id)
}

fn active_key(run_id: &str, task_id: &str) -> String {
    format!("{run_id}:{task_id}")
}

impl TeamState {
    pub(crate) fn load() -> Self {
        let mut runs = HashMap::new();
        if let Ok(text) = fs::read_to_string(team_runs_path()) {
            if let Ok(list) = serde_json::from_str::<Vec<TeamRunView>>(&text) {
                for run in list {
                    runs.insert(run.id.clone(), run);
                }
            }
        }
        let mut changed = false;
        for run in runs.values_mut() {
            if matches!(
                run.phase,
                RunPhase::Planning | RunPhase::Running | RunPhase::Summarizing
            ) {
                run.phase = RunPhase::Failed;
                run.error = Some("岛已退出或重启，任务中断；结果未知".to_string());
                for task in &mut run.tasks {
                    if matches!(
                        task.state,
                        TaskState::Running | TaskState::Queued | TaskState::WaitingApproval
                    ) {
                        task.state = TaskState::Failed;
                        task.error = Some("岛已退出或重启，结果未知".to_string());
                    }
                }
                changed = true;
            }
        }
        let state = Self {
            runs: Mutex::new(runs),
            active: Mutex::new(HashMap::new()),
            managed_sessions: Mutex::new(HashMap::new()),
        };
        if changed {
            state.save();
        }
        state
    }

    pub(crate) fn save(&self) {
        let mut list: Vec<TeamRunView> = self.runs.lock_recover().values().cloned().collect();
        list.sort_by(|left, right| right.created_at_ms.cmp(&left.created_at_ms));
        list.truncate(MAX_PERSISTED_RUNS);
        let Ok(json) = serde_json::to_string_pretty(&list) else {
            return;
        };
        if json.len() > MAX_RUNS_BYTES {
            return;
        }
        let path = team_runs_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let temp = path.with_extension("json.tmp");
        if fs::write(&temp, json).is_ok() {
            let _ = fs::rename(&temp, &path);
        }
    }

    pub(crate) fn active_pids(&self) -> HashSet<u32> {
        self.active.lock_recover().values().copied().collect()
    }

    pub(crate) fn register_session(
        &self,
        session_id: &str,
        run_id: &str,
        task_id: &str,
        worktree_root: &Path,
    ) {
        self.managed_sessions.lock_recover().insert(
            session_id.to_string(),
            ManagedSession {
                run_id: run_id.to_string(),
                task_id: task_id.to_string(),
                worktree_root: worktree_root.to_path_buf(),
            },
        );
    }

    pub(crate) fn unregister_session(&self, session_id: &str) {
        self.managed_sessions.lock_recover().remove(session_id);
    }

    pub(crate) fn task_for_session(&self, session_id: &str) -> Option<(String, String)> {
        self.managed_sessions
            .lock_recover()
            .get(session_id)
            .map(|session| (session.run_id.clone(), session.task_id.clone()))
    }

    pub(crate) fn session_worktree(&self, session_id: &str) -> Option<PathBuf> {
        self.managed_sessions
            .lock_recover()
            .get(session_id)
            .map(|session| session.worktree_root.clone())
    }

    fn update_task(
        &self,
        run_id: &str,
        task_id: &str,
        change: impl FnOnce(&mut TeamTaskView),
    ) -> Option<TeamRunView> {
        self.update(run_id, |run| {
            if let Some(task) = run.tasks.iter_mut().find(|task| task.id == task_id) {
                change(task);
            }
        })
    }

    /// worker 的 Bash 需要人工确认：任务进入"等待审批"，由小岛上的审批卡处理。
    pub(crate) fn begin_task_approval(
        &self,
        run_id: &str,
        task_id: &str,
        approval_id: &str,
        command: &str,
    ) -> Option<TeamRunView> {
        self.update_task(run_id, task_id, |task| {
            task.state = TaskState::WaitingApproval;
            task.pending_approval = Some(cap_text(command, 400));
            task.approval_id = Some(approval_id.to_string());
        })
    }

    pub(crate) fn finish_task_approval(
        &self,
        run_id: &str,
        task_id: &str,
    ) -> Option<TeamRunView> {
        self.update_task(run_id, task_id, |task| {
            if task.state == TaskState::WaitingApproval {
                task.state = TaskState::Running;
            }
            task.pending_approval = None;
            task.approval_id = None;
        })
    }

    /// 审批超时：立即按拒绝处理，避免 worker 在 print 模式里无限等待。
    pub(crate) fn fail_task_approval(
        &self,
        run_id: &str,
        task_id: &str,
        reason: &str,
    ) -> Option<TeamRunView> {
        self.update_task(run_id, task_id, |task| {
            task.state = TaskState::Failed;
            task.error = Some(reason.to_string());
            task.pending_approval = None;
            task.approval_id = None;
            task.ended_at_ms = Some(epoch_millis());
        })
    }

    pub(crate) fn insert(&self, run: TeamRunView) {
        self.runs.lock_recover().insert(run.id.clone(), run);
        self.save();
    }

    pub(crate) fn get(&self, id: &str) -> Option<TeamRunView> {
        self.runs.lock_recover().get(id).cloned()
    }

    pub(crate) fn list(&self) -> Vec<TeamRunView> {
        let mut list: Vec<TeamRunView> = self.runs.lock_recover().values().cloned().collect();
        list.sort_by(|left, right| right.updated_at_ms.cmp(&left.updated_at_ms));
        list
    }

    pub(crate) fn update(
        &self,
        id: &str,
        change: impl FnOnce(&mut TeamRunView),
    ) -> Option<TeamRunView> {
        let view = {
            let mut runs = self.runs.lock_recover();
            let run = runs.get_mut(id)?;
            change(run);
            run.updated_at_ms = epoch_millis();
            run.clone()
        };
        self.save();
        Some(view)
    }
}

fn emit_run(app: &tauri::AppHandle, view: &TeamRunView) {
    use tauri::Emitter;
    let _ = app.emit("team-run", view);
}

/// 应用正常退出时终止团队任务，避免留下仍在改文件的孤儿进程。
pub(crate) fn shutdown(state: &AppState) {
    let pids = state.team.active_pids();
    for pid in pids {
        runner::kill_process_tree(pid);
    }
    let running: Vec<String> = state
        .team
        .runs
        .lock_recover()
        .values()
        .filter(|run| {
            matches!(
                run.phase,
                RunPhase::Planning | RunPhase::Running | RunPhase::Summarizing
            )
        })
        .map(|run| run.id.clone())
        .collect();
    for id in running {
        let _ = state.team.update(&id, |run| {
            run.phase = RunPhase::Failed;
            run.error = Some("岛已退出，任务已中止；结果未知".to_string());
            for task in &mut run.tasks {
                if matches!(
                    task.state,
                    TaskState::Running | TaskState::Queued | TaskState::WaitingApproval
                ) {
                    task.state = TaskState::Cancelled;
                    task.error = Some("岛已退出，任务已中止".to_string());
                    task.ended_at_ms = Some(epoch_millis());
                }
            }
        });
    }
}

fn normalize_budget(value: Option<f64>) -> Result<Option<f64>, String> {
    match value {
        Some(value) if value.is_finite() && value > 0.0 && value <= 100.0 => {
            Ok(Some((value * 100.0).round() / 100.0))
        }
        Some(_) => Err("预算必须是 0.01–100 USD 之间的数字".to_string()),
        None => Ok(None),
    }
}

fn normalize_workspace(input: &str) -> Result<PathBuf, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("请选择工作区目录".to_string());
    }
    let path = PathBuf::from(trimmed);
    let canonical = path
        .canonicalize()
        .map_err(|_| format!("工作区不存在：{}", path.display()))?;
    if !canonical.is_dir() {
        return Err("工作区不是目录".to_string());
    }
    if canonical.parent().is_none() {
        return Err("不能把磁盘根目录作为工作区".to_string());
    }
    // Windows canonicalize 会加 `\\?\` 前缀；界面上显示它很吵，且 Claude 的
    // cwd 用普通路径就够。UNC 路径要保留 `\\server\share` 形式。
    let text = canonical.to_string_lossy();
    let cleaned = if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        canonical
    };
    Ok(cleaned)
}

fn from_plan_task(task: &PlanTask) -> TeamTaskView {
    TeamTaskView {
        id: task.id.trim().to_string(),
        title: task.title.trim().to_string(),
        assignee: task.assignee.trim().to_string(),
        assignee_name: "Claude Code".to_string(),
        prompt: cap_text(task.prompt.trim(), 8_000),
        depends_on: task.depends_on.iter().map(|value| value.trim().to_string()).collect(),
        acceptance: cap_text(task.acceptance.trim(), 4_000),
        verify_command: task
            .verify_command
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| cap_text(value, 1_000)),
        verification: None,
        state: TaskState::Queued,
        started_at_ms: None,
        ended_at_ms: None,
        exit_code: None,
        result: None,
        error: None,
        cost_usd: None,
        log: Vec::new(),
        session_id: None,
        pending_approval: None,
        approval_id: None,
        worktree_path: None,
        branch: None,
        base_commit: None,
        changed_files: Vec::new(),
        diff_stat: None,
        merge_state: MergeState::None,
        merge_error: None,
    }
}

fn planner_call(
    prompt: &str,
    workspace: &Path,
    max_budget_usd: Option<f64>,
    on_spawn: impl FnOnce(u32),
    on_cost: impl FnOnce(Option<f64>),
) -> Result<(TeamPlan, String, Option<f64>), String> {
    let program = capability::claude_program()
        .ok_or_else(|| "找不到 Claude Code 可执行文件".to_string())?;
    let allowed = capability::available_agent_ids();
    if allowed.is_empty() {
        return Err(
            capability::capabilities()
                .into_iter()
                .find(|item| item.id == "claude")
                .map(|item| item.detail)
                .unwrap_or_else(|| "没有可用的 Agent".to_string()),
        );
    }
    let schema = plan::plan_schema(&allowed);
    let schema_text = schema.to_string();
    let env = capability::claude_settings_env();
    let session_id = new_uuid();
    let config = RunnerConfig {
        program: &program,
        cwd: workspace,
        prompt,
        settings: None,
        session_id: &session_id,
        permission_mode: "plan",
        max_turns: 3,
        timeout: Duration::from_secs(PLANNER_TIMEOUT_SECS),
        output_format: "json",
        json_schema: Some(&schema_text),
        max_budget_usd,
        env: &env,
    };
    let outcome = runner::run_claude(&config, on_spawn);
    on_cost(outcome.cost_usd);
    if !outcome.ok {
        return Err(outcome
            .error
            .unwrap_or_else(|| "主 Agent 规划失败".to_string()));
    }
    let value = outcome
        .structured
        .clone()
        .or_else(|| {
            outcome
                .result
                .as_deref()
                .and_then(|text| serde_json::from_str::<Value>(text).ok())
        })
        .ok_or_else(|| "主 Agent 没有返回结构化计划".to_string())?;
    let team_plan: TeamPlan =
        serde_json::from_value(value).map_err(|error| format!("计划格式不合法：{error}"))?;
    plan::validate_plan(&team_plan, &allowed).map_err(|errors| errors.join("；"))?;
    Ok((
        team_plan,
        outcome.session_id,
        outcome.cost_usd,
    ))
}

fn synthesis_call(
    goal: &str,
    workspace: &Path,
    tasks: &[TeamTaskView],
    max_budget_usd: Option<f64>,
    on_spawn: impl FnOnce(u32),
) -> Result<(String, Option<f64>), String> {
    let program = capability::claude_program()
        .ok_or_else(|| "找不到 Claude Code 可执行文件".to_string())?;
    let env = capability::claude_settings_env();
    let plan_tasks: Vec<PlanTask> = tasks
        .iter()
        .map(|task| PlanTask {
            id: task.id.clone(),
            title: task.title.clone(),
            assignee: task.assignee.clone(),
            prompt: task.prompt.clone(),
            depends_on: task.depends_on.clone(),
            acceptance: task.acceptance.clone(),
            verify_command: task.verify_command.clone(),
        })
        .collect();
    let results: Vec<String> = tasks
        .iter()
        .map(|task| {
            task.result
                .clone()
                .or_else(|| task.error.clone())
                .unwrap_or_else(|| "（没有返回结果）".to_string())
        })
        .collect();
    let prompt = plan::synthesis_prompt(goal, workspace, &plan_tasks, &results);
    let session_id = new_uuid();
    let config = RunnerConfig {
        program: &program,
        cwd: workspace,
        prompt: &prompt,
        settings: None,
        session_id: &session_id,
        permission_mode: "plan",
        max_turns: 2,
        timeout: Duration::from_secs(SYNTHESIS_TIMEOUT_SECS),
        output_format: "json",
        json_schema: None,
        max_budget_usd,
        env: &env,
    };
    let outcome = runner::run_claude(&config, on_spawn);
    let cost = outcome.cost_usd;
    let error = outcome.error;
    match (outcome.ok, outcome.result) {
        (true, Some(text)) if !text.trim().is_empty() => Ok((text, cost)),
        (_, _) => Err(error.unwrap_or_else(|| "主 Agent 汇总失败".to_string())),
    }
}

fn worker_settings_path(run_id: &str) -> Result<PathBuf, String> {
    let script = hook_script_path();
    if !script.is_file() {
        return Err("Claude 事件桥接脚本不存在；请先在设置里开启 Claude 事件".to_string());
    }
    let dir = team_run_dir(run_id);
    fs::create_dir_all(&dir).map_err(|error| format!("创建团队运行目录失败：{error}"))?;
    let hook = format!("node \"{}\" --timeout=310000", script.display());
    let settings = serde_json::json!({
        "hooks": {
            // Bash 用来审批危险命令；文件工具用来阻止 worker 写到 worktree
            // 之外（worktree 只改 cwd，绝对路径仍然可能命中主工作区）。
            "PreToolUse": [
                {
                    "matcher": "Bash",
                    "hooks": [{ "type": "command", "command": hook.clone() }]
                },
                {
                    "matcher": "Edit|Write|MultiEdit|NotebookEdit",
                    "hooks": [{ "type": "command", "command": hook.clone() }]
                }
            ]
        }
    });
    let path = dir.join("claude-settings.json");
    fs::write(
        &path,
        serde_json::to_string_pretty(&settings).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("写入团队审批设置失败：{error}"))?;
    Ok(path)
}

fn execute_task(
    app: &tauri::AppHandle,
    run_id: &str,
    task: &TeamTaskView,
    workspace: &Path,
    worktree_root: &Path,
    goal: &str,
    max_budget_usd: Option<f64>,
) -> ProcessOutcome {
    let state = app.state::<AppState>();
    let Some(program) = capability::claude_program() else {
        return ProcessOutcome {
            error: Some("找不到 Claude Code 可执行文件".to_string()),
            session_id: String::new(),
            ..ProcessOutcome::default()
        };
    };
    let settings = match worker_settings_path(run_id) {
        Ok(path) => path,
        Err(error) => {
            return ProcessOutcome {
                error: Some(error),
                session_id: String::new(),
                ..ProcessOutcome::default()
            }
        }
    };
    let plan_task = PlanTask {
        id: task.id.clone(),
        title: task.title.clone(),
        assignee: task.assignee.clone(),
        prompt: task.prompt.clone(),
        depends_on: task.depends_on.clone(),
        acceptance: task.acceptance.clone(),
        verify_command: task.verify_command.clone(),
    };
    let prompt = plan::worker_prompt(goal, workspace, &plan_task);
    let session_id = task.session_id.clone().unwrap_or_else(new_uuid);
    let env = capability::claude_settings_env();
    let schema = plan::worker_schema().to_string();
    let config = RunnerConfig {
        program: &program,
        cwd: workspace,
        prompt: &prompt,
        settings: Some(&settings),
        session_id: &session_id,
        permission_mode: "acceptEdits",
        max_turns: 16,
        timeout: Duration::from_secs(TASK_TIMEOUT_SECS),
        output_format: "stream-json",
        json_schema: Some(&schema),
        max_budget_usd,
        env: &env,
    };
    let key = active_key(run_id, &task.id);
    let team = &state.team;
    team.register_session(&session_id, run_id, &task.id, worktree_root);
    let outcome = runner::run_claude(&config, |pid| {
        team.active.lock_recover().insert(key.clone(), pid);
    });
    team.unregister_session(&session_id);
    team.active.lock_recover().remove(&key);
    outcome
}

fn worker_report(outcome: &ProcessOutcome) -> (TaskState, Option<String>, Option<String>) {
    let structured = outcome.structured.as_ref();
    let status = structured
        .and_then(|value| value.get("status"))
        .and_then(Value::as_str);
    let summary = structured
        .and_then(|value| value.get("summary"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let files: Vec<String> = structured
        .and_then(|value| value.get("files_changed"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let verification = structured
        .and_then(|value| value.get("verification"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let mut parts = Vec::new();
    if let Some(summary) = &summary {
        parts.push(summary.clone());
    }
    if !files.is_empty() {
        parts.push(format!("文件：{}", files.join("、")));
    }
    if let Some(verification) = verification {
        parts.push(format!("验证：{verification}"));
    }
    let display = if parts.is_empty() {
        outcome.result.clone()
    } else {
        Some(parts.join("\n"))
    };

    if !outcome.ok {
        return (
            TaskState::Failed,
            display,
            outcome
                .error
                .clone()
                .or_else(|| Some("worker 进程失败".to_string())),
        );
    }

    match status {
        Some("completed") => (TaskState::Succeeded, display, None),
        Some("blocked") => (
            TaskState::Failed,
            display,
            Some(summary.unwrap_or_else(|| "任务报告 blocked".to_string())),
        ),
        Some("failed") => (
            TaskState::Failed,
            display,
            Some(summary.unwrap_or_else(|| "任务报告 failed".to_string())),
        ),
        _ => (
            TaskState::Failed,
            display,
            Some("worker 没有返回结构化任务结果，不能算完成".to_string()),
        ),
    }
}

fn run_task_verification(
    state: &AppState,
    run_id: &str,
    task: &TeamTaskView,
    workspace: &Path,
) -> Option<verification::TaskVerification> {
    let command = task.verify_command.as_deref()?.trim();
    if command.is_empty() {
        return None;
    }
    let key = active_key(run_id, &format!("verify:{}", task.id));
    let pid_key = key.clone();
    let result = verification::run(
        command,
        workspace,
        Duration::from_secs(verification::VERIFY_TIMEOUT_SECS),
        |pid| {
            state.team.active.lock_recover().insert(pid_key.clone(), pid);
        },
    );
    state.team.active.lock_recover().remove(&key);
    Some(result)
}

fn apply_task_completion(
    app: &tauri::AppHandle,
    state: &AppState,
    run_id: &str,
    task: &TeamTaskView,
    outcome: &ProcessOutcome,
    finalized: &Result<worktree::WorktreeChanges, String>,
    verification: Option<verification::TaskVerification>,
    ended: u64,
) -> Option<TeamRunView> {
    let (mut next_state, display, report_error) = worker_report(outcome);
    let mut final_error = report_error;
    let mut verification_blocked = false;
    if let Some(result) = verification.as_ref() {
        if next_state == TaskState::Succeeded && !result.passed {
            next_state = TaskState::Failed;
            verification_blocked = true;
            final_error = Some(if result.timed_out {
                format!(
                    "验收命令超时（{}s）：{}",
                    verification::VERIFY_TIMEOUT_SECS,
                    cap_text(&result.output, 800)
                )
            } else {
                format!(
                    "验收命令失败（退出码 {}）：{}",
                    result
                        .exit_code
                        .map(|code| code.to_string())
                        .unwrap_or_else(|| "未知".to_string()),
                    cap_text(&result.output, 800)
                )
            });
        }
    }
    if outcome.timed_out && final_error.is_none() {
        final_error = Some("任务超时，已终止进程树".to_string());
    }
    let view = state.team.update(run_id, |run| {
        if let Some(task) = run.tasks.iter_mut().find(|item| item.id == task.id) {
            task.state = next_state;
            task.ended_at_ms = Some(ended);
            task.exit_code = outcome.exit_code;
            task.cost_usd = outcome.cost_usd;
            task.result = display.as_deref().map(|text| cap_text(text, 8_000));
            task.error = final_error
                .as_deref()
                .map(|text| cap_text(text, 2_000));
            task.log = outcome.log.clone();
            task.verification = verification;
            match finalized {
                Ok(changes) if changes.files.is_empty() => {
                    task.changed_files.clear();
                    task.diff_stat = None;
                    task.merge_state = MergeState::None;
                    task.merge_error = None;
                }
                Ok(changes) => {
                    task.changed_files = changes.files.clone();
                    task.diff_stat = Some(changes.stat.clone());
                    task.merge_state = if next_state == TaskState::Succeeded {
                        MergeState::Pending
                    } else {
                        MergeState::None
                    };
                    task.merge_error = verification_blocked
                        .then(|| "验收未通过，禁止合并".to_string());
                }
                Err(error) => {
                    task.changed_files.clear();
                    task.diff_stat = None;
                    task.merge_state = MergeState::None;
                    task.merge_error = Some(format!("收集任务改动失败：{error}"));
                }
            }
        }
        run.spent_usd += outcome.cost_usd.unwrap_or(0.0);
    });
    if let Some(view) = &view {
        emit_run(app, view);
    }
    view
}

fn pick_task(tasks: &[TeamTaskView]) -> Pick {
    for (index, task) in tasks.iter().enumerate() {
        if task.state != TaskState::Queued {
            continue;
        }
        let mut ready = true;
        for dependency in &task.depends_on {
            match tasks
                .iter()
                .find(|candidate| &candidate.id == dependency)
                .map(|candidate| candidate.state)
            {
                Some(TaskState::Succeeded) => {}
                Some(TaskState::Failed | TaskState::Cancelled | TaskState::Skipped) => {
                    return Pick::Blocked(index)
                }
                _ => ready = false,
            }
        }
        if ready {
            return Pick::Ready(index);
        }
    }
    Pick::None
}

enum Pick {
    Ready(usize),
    Blocked(usize),
    None,
}

/// 无依赖任务从本轮 base 创建；有依赖任务从依赖分支的 tip 创建，这样
/// 能看到前一个任务的改动，同时仍是独立 worktree。
fn task_base_commit(
    run: &TeamRunView,
    task: &TeamTaskView,
    repo_root: &Path,
) -> Result<String, String> {
    if task.depends_on.len() > 1 {
        return Err("任务有多个依赖；当前只支持单个依赖的线性链".to_string());
    }
    let Some(dependency_id) = task.depends_on.first() else {
        return run
            .base_commit
            .clone()
            .ok_or_else(|| "这一轮缺少基准 commit，请新建一轮".to_string());
    };
    let dependency = run
        .tasks
        .iter()
        .find(|candidate| &candidate.id == dependency_id)
        .ok_or_else(|| format!("依赖任务不存在：{dependency_id}"))?;
    if dependency.state != TaskState::Succeeded {
        return Err(format!("依赖任务还没成功：{}", dependency.title));
    }
    let branch = dependency
        .branch
        .as_deref()
        .ok_or_else(|| format!("依赖任务缺少分支：{}", dependency.title))?;
    worktree::branch_tip(repo_root, branch)
}

fn dependent_ids(tasks: &[TeamTaskView], root: &str) -> HashSet<String> {
    let mut ids: HashSet<String> = HashSet::from([root.to_string()]);
    loop {
        let mut added = false;
        for task in tasks {
            if ids.contains(&task.id) {
                continue;
            }
            if task
                .depends_on
                .iter()
                .any(|dependency| ids.contains(dependency))
                && ids.insert(task.id.clone())
            {
                added = true;
            }
        }
        if !added {
            return ids;
        }
    }
}

/// 依赖任务有文件改动时，必须先合并进主工作区，后续任务才能合并。
fn dependency_blocker(tasks: &[TeamTaskView], task: &TeamTaskView) -> Option<String> {
    for dependency_id in &task.depends_on {
        let Some(dependency) = tasks
            .iter()
            .find(|candidate| &candidate.id == dependency_id)
        else {
            return Some(format!("依赖任务不存在：{dependency_id}"));
        };
        if dependency.state != TaskState::Succeeded {
            return Some(format!("依赖任务还没成功：{}", dependency.title));
        }
        if dependency.merge_state == MergeState::Discarded {
            return Some(format!("依赖任务已丢弃，无法合并：{}", dependency.title));
        }
        if !dependency.changed_files.is_empty() && dependency.merge_state != MergeState::Merged {
            return Some(format!("先合并依赖任务：{}", dependency.title));
        }
    }
    None
}

fn fail_task_setup(
    app: &tauri::AppHandle,
    state: &AppState,
    run_id: &str,
    task_id: &str,
    message: String,
) {
    let message = cap_text(&message, 2_000);
    if let Some(view) = state.team.update(run_id, |run| {
        if let Some(task) = run.tasks.iter_mut().find(|task| task.id == task_id) {
            task.state = TaskState::Failed;
            task.error = Some(message.clone());
            task.ended_at_ms = Some(epoch_millis());
        }
        run.phase = RunPhase::Failed;
        run.error = Some(format!("准备任务 worktree 失败：{message}"));
        for task in &mut run.tasks {
            if task.state == TaskState::Queued {
                task.state = TaskState::Skipped;
                task.error = Some("worktree 准备失败，未派发".to_string());
                task.ended_at_ms = Some(epoch_millis());
            }
        }
    }) {
        emit_run(app, &view);
    }
}

fn run_scheduler(app: tauri::AppHandle, run_id: String) {
    let concurrency = app
        .state::<AppState>()
        .team
        .get(&run_id)
        .map(|run| run.concurrency)
        .unwrap_or(1);
    if concurrency <= 1 {
        run_scheduler_serial(app, run_id);
    } else {
        scheduler::run_parallel(app, run_id);
    }
}

fn run_scheduler_serial(app: tauri::AppHandle, run_id: String) {
    loop {
        let state = app.state::<AppState>();
        let Some(run) = state.team.get(&run_id) else {
            return;
        };
        if matches!(
            run.phase,
            RunPhase::Cancelled | RunPhase::Failed | RunPhase::Succeeded
        ) {
            return;
        }
        let goal = run.goal.clone();
        let remaining = run
            .budget_usd
            .map(|budget| (budget - run.spent_usd).max(0.0));
        if remaining.is_some_and(|value| value <= 0.005) {
            let budget = run.budget_usd.unwrap_or(0.0);
            let spent = run.spent_usd;
            if let Some(view) = state.team.update(&run_id, |run| {
                run.phase = RunPhase::Failed;
                run.error = Some(format!(
                    "预算已用完：已花 ${spent:.2} / 预算 ${budget:.2}；未派发的任务已跳过"
                ));
                for task in &mut run.tasks {
                    if task.state == TaskState::Queued {
                        task.state = TaskState::Skipped;
                        task.error = Some("预算已用完，未派发".to_string());
                        task.ended_at_ms = Some(epoch_millis());
                    }
                }
            }) {
                emit_run(&app, &view);
            }
            return;
        }
        let index = match pick_task(&run.tasks) {
            Pick::Ready(index) => index,
            Pick::Blocked(index) => {
                let task_id = run.tasks[index].id.clone();
                if let Some(view) = state.team.update(&run_id, |run| {
                    if let Some(task) = run.tasks.iter_mut().find(|task| task.id == task_id) {
                        task.state = TaskState::Skipped;
                        task.error = Some("依赖的任务没有成功，已跳过".to_string());
                        task.ended_at_ms = Some(epoch_millis());
                    }
                }) {
                    emit_run(&app, &view);
                }
                continue;
            }
            Pick::None => {
                let remaining = run
                    .tasks
                    .iter()
                    .filter(|task| task.state == TaskState::Queued)
                    .count();
                if remaining > 0 {
                    if let Some(view) = state.team.update(&run_id, |run| {
                        for task in &mut run.tasks {
                            if task.state == TaskState::Queued {
                                task.state = TaskState::Skipped;
                                task.error = Some("依赖无法满足，已跳过".to_string());
                                task.ended_at_ms = Some(epoch_millis());
                            }
                        }
                    }) {
                        emit_run(&app, &view);
                    }
                }
                break;
            }
        };

        let task = run.tasks[index].clone();
        let Some(repo_root) = run.repo_root.clone() else {
            fail_task_setup(
                &app,
                &state,
                &run_id,
                &task.id,
                "这一轮不是 worktree 模式（旧记录），请新建一轮".to_string(),
            );
            return;
        };
        let repo_path = PathBuf::from(&repo_root);
        let base_commit = match task_base_commit(&run, &task, &repo_path) {
            Ok(commit) => commit,
            Err(error) => {
                fail_task_setup(&app, &state, &run_id, &task.id, error);
                return;
            }
        };
        let mut repo = match worktree::detect(&repo_path) {
            Ok(repo) => repo,
            Err(error) => {
                fail_task_setup(&app, &state, &run_id, &task.id, error);
                return;
            }
        };
        repo.head = base_commit;
        let task_worktree = match worktree::create(&repo, &run_id, &task.id) {
            Ok(worktree) => worktree,
            Err(error) => {
                fail_task_setup(&app, &state, &run_id, &task.id, error);
                return;
            }
        };
        let relative = Path::new(&run.workspace)
            .strip_prefix(&repo.root)
            .unwrap_or(Path::new(""));
        let task_workspace = if relative.as_os_str().is_empty() {
            task_worktree.path.clone()
        } else {
            task_worktree.path.join(relative)
        };
        if !task_workspace.is_dir() {
            fail_task_setup(
                &app,
                &state,
                &run_id,
                &task.id,
                format!("worktree 里找不到工作子目录：{}", task_workspace.display()),
            );
            return;
        }

        let session_id = new_uuid();
        let Some(started_view) = state.team.update(&run_id, |run| {
            if let Some(task) = run.tasks.iter_mut().find(|item| item.id == task.id) {
                task.state = TaskState::Running;
                task.started_at_ms = Some(epoch_millis());
                task.session_id = Some(session_id.clone());
                task.error = None;
                task.result = None;
                task.log.clear();
                task.pending_approval = None;
                task.approval_id = None;
                task.worktree_path = Some(task_worktree.path.to_string_lossy().into_owned());
                task.branch = Some(task_worktree.branch.clone());
                task.base_commit = Some(task_worktree.base_commit.clone());
                task.changed_files.clear();
                task.diff_stat = None;
                task.merge_state = MergeState::None;
                task.merge_error = None;
            }
        }) else {
            return;
        };
        emit_run(&app, &started_view);

        let outcome = execute_task(
            &app,
            &run_id,
            &task,
            &task_workspace,
            &task_worktree.path,
            &goal,
            remaining,
        );
        let current = state.team.get(&run_id);
        if current.as_ref().is_some_and(|run| run.phase == RunPhase::Cancelled) {
            return;
        }
        let finalized = worktree::finalize(&task_worktree, &task.title);
        let verification = if worker_report(&outcome).0 == TaskState::Succeeded {
            run_task_verification(&state, &run_id, &task, &task_workspace)
        } else {
            None
        };
        if state
            .team
            .get(&run_id)
            .is_some_and(|run| run.phase == RunPhase::Cancelled)
        {
            return;
        }
        let ended = epoch_millis();
        if apply_task_completion(
            &app,
            &state,
            &run_id,
            &task,
            &outcome,
            &finalized,
            verification,
            ended,
        )
        .is_none()
        {
            return;
        }
    }

    // 所有任务结束：有失败就不汇总，明确报告失败；全成功才让主 Agent 汇总。
    let state = app.state::<AppState>();
    let Some(run) = state.team.get(&run_id) else {
        return;
    };
    if run.phase == RunPhase::Cancelled {
        return;
    }
    let failed = run
        .tasks
        .iter()
        .any(|task| matches!(task.state, TaskState::Failed | TaskState::Cancelled | TaskState::Skipped));
    if failed {
        if let Some(view) = state.team.update(&run_id, |run| {
            run.phase = RunPhase::Failed;
            run.error = Some("有任务失败或被跳过，未完成汇总".to_string());
        }) {
            emit_run(&app, &view);
        }
        return;
    }

    if let Some(view) = state.team.update(&run_id, |run| {
        run.phase = RunPhase::Summarizing;
    }) {
        emit_run(&app, &view);
    }
    let synthesis_key = active_key(&run_id, "synthesis");
    let synthesis_pid_key = synthesis_key.clone();
    let synthesis_budget = run
        .budget_usd
        .map(|budget| (budget - run.spent_usd).max(0.0));
    let synthesis = if synthesis_budget.is_some_and(|value| value <= 0.005) {
        Err("预算已用完，跳过汇总".to_string())
    } else {
        synthesis_call(
            &run.goal,
            &PathBuf::from(&run.workspace),
            &run.tasks,
            synthesis_budget,
            |pid| {
                state
                    .team
                    .active
                    .lock_recover()
                    .insert(synthesis_pid_key.clone(), pid);
            },
        )
    };
    state.team.active.lock_recover().remove(&synthesis_key);
    if state
        .team
        .get(&run_id)
        .is_some_and(|run| run.phase == RunPhase::Cancelled)
    {
        return;
    }
    let fallback: String = run
        .tasks
        .iter()
        .map(|task| {
            format!(
                "### {}（{}）\n{}",
                task.title,
                task.assignee_name,
                task.result.clone().unwrap_or_else(|| "已完成".to_string())
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    if let Some(view) = state.team.update(&run_id, |run| {
        run.phase = RunPhase::Succeeded;
        match &synthesis {
            Ok((text, cost)) => {
                run.summary = Some(text.clone());
                run.spent_usd += (*cost).unwrap_or(0.0);
                run.error = None;
            }
            Err(error) => {
                run.summary = Some(fallback);
                run.error = Some(format!("汇总失败：{error}"));
            }
        }
    }) {
        emit_run(&app, &view);
    }
}

#[tauri::command]
pub(crate) fn team_capabilities() -> Vec<capability::AgentCapability> {
    capability::capabilities()
}

#[tauri::command]
pub(crate) fn team_start_plan(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
    goal: String,
    workspace: String,
    budget_usd: Option<f64>,
    concurrency: Option<u32>,
) -> Result<TeamRunView, String> {
    let goal = goal.trim().to_string();
    if goal.is_empty() {
        return Err("请先写清楚团队目标".to_string());
    }
    if goal.chars().count() > 4000 {
        return Err("目标太长，请压缩到 4000 字以内".to_string());
    }
    let budget_usd = normalize_budget(budget_usd)?;
    let concurrency = concurrency.unwrap_or(2).clamp(1, 4);
    let workspace = normalize_workspace(&workspace)?;
    let repo = worktree::detect(&workspace)?;
    let workspace_warning = repo.dirty.then(|| {
        "主工作区有未提交改动；worktree 从当前 HEAD 创建，任务看不到这些改动，合并前需要先提交或暂存"
            .to_string()
    });
    let available = capability::available_agent_ids();
    if !available.iter().any(|id| id == "claude") {
        return Err(capability::capabilities()
            .into_iter()
            .find(|item| item.id == "claude")
            .map(|item| item.detail)
            .unwrap_or_else(|| "Claude 当前不可用".to_string()));
    }
    let now = epoch_millis();
    let run = TeamRunView {
        id: next_id("team"),
        goal: goal.clone(),
        workspace: workspace.to_string_lossy().into_owned(),
        main_agent: "claude".to_string(),
        main_agent_name: "Claude Code".to_string(),
        phase: RunPhase::Planning,
        plan_summary: None,
        summary: None,
        tasks: Vec::new(),
        created_at_ms: now,
        updated_at_ms: now,
        error: None,
        planner_session_id: None,
        budget_usd,
        spent_usd: 0.0,
        repo_root: Some(repo.root.to_string_lossy().into_owned()),
        base_branch: Some(repo.branch),
        base_commit: Some(repo.head),
        worktree_mode: true,
        workspace_warning,
        concurrency,
        parent_run_id: None,
        round: 1,
    };
    state.team.insert(run.clone());
    emit_run(&app, &run);
    let prompt = plan::planner_prompt(
        &goal,
        &workspace,
        &capability::available_agent_ids(),
    );
    spawn_planner(app, run.id.clone(), prompt, workspace, budget_usd);
    Ok(run)
}

fn spawn_planner(
    app: tauri::AppHandle,
    run_id: String,
    prompt: String,
    workspace: PathBuf,
    budget_usd: Option<f64>,
) {
    std::thread::spawn(move || {
        let app_state = app.state::<AppState>();
        let planner_key = active_key(&run_id, "planner");
        let planner_pid_key = planner_key.clone();
        let result = planner_call(&prompt, &workspace, budget_usd, |pid| {
            app_state
                .team
                .active
                .lock_recover()
                .insert(planner_pid_key.clone(), pid);
        }, |cost| {
            if let Some(cost) = cost {
                let _ = app_state.team.update(&run_id, |run| {
                    run.spent_usd += cost;
                });
            }
        });
        app_state.team.active.lock_recover().remove(&planner_key);
        if app_state
            .team
            .get(&run_id)
            .is_some_and(|run| run.phase == RunPhase::Cancelled)
        {
            return;
        }
        match result {
            Ok((plan, session_id, _cost)) => {
                let tasks: Vec<TeamTaskView> = plan.tasks.iter().map(from_plan_task).collect();
                if let Some(view) = app_state.team.update(&run_id, |run| {
                    run.phase = RunPhase::AwaitingPlan;
                    run.plan_summary = Some(plan.summary.clone());
                    run.tasks = tasks;
                    run.planner_session_id = Some(session_id);
                    run.error = None;
                }) {
                    emit_run(&app, &view);
                }
            }
            Err(error) => {
                if let Some(view) = app_state.team.update(&run_id, |run| {
                    run.phase = RunPhase::Failed;
                    run.error = Some(error);
                }) {
                    emit_run(&app, &view);
                }
            }
        }
    });
}

#[tauri::command]
pub(crate) fn team_get_run(state: tauri::State<AppState>, id: String) -> Option<TeamRunView> {
    state.team.get(&id)
}

#[tauri::command]
pub(crate) fn team_list_runs(state: tauri::State<AppState>) -> Vec<TeamRunView> {
    state.team.list()
}

fn task_state_label(state: TaskState) -> &'static str {
    match state {
        TaskState::Queued => "排队",
        TaskState::Running => "执行中",
        TaskState::WaitingApproval => "等待审批",
        TaskState::Succeeded => "完成",
        TaskState::Failed => "失败",
        TaskState::Cancelled => "取消",
        TaskState::Skipped => "跳过",
    }
}

fn previous_round_context(run: &TeamRunView) -> String {
    let mut text = format!("上一轮目标：{}\n", run.goal);
    if let Some(summary) = run.summary.as_deref().or(run.plan_summary.as_deref()) {
        text.push_str(&format!("上一轮总结：{}\n", cap_text(summary, 4_000)));
    }
    text.push_str("上一轮任务：\n");
    for task in &run.tasks {
        let outcome = task
            .result
            .as_deref()
            .or(task.error.as_deref())
            .unwrap_or("没有返回结果");
        text.push_str(&format!(
            "- [{}] {}：{}\n",
            task_state_label(task.state),
            task.title,
            cap_text(outcome, 1_500)
        ));
        if let Some(verification) = &task.verification {
            text.push_str(&format!(
                "  验收：{}（退出码 {}）：{}\n",
                if verification.passed { "通过" } else { "失败" },
                verification
                    .exit_code
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "—".to_string()),
                cap_text(&verification.output, 500)
            ));
        }
    }
    text
}

#[tauri::command]
pub(crate) fn team_continue_run(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
    id: String,
    instruction: String,
    budget_usd: Option<f64>,
    concurrency: Option<u32>,
) -> Result<TeamRunView, String> {
    let instruction = instruction.trim().to_string();
    if instruction.is_empty() {
        return Err("请写一句下一轮要做什么".to_string());
    }
    if instruction.chars().count() > 2_000 {
        return Err("下一轮指令太长，请压缩到 2000 字以内".to_string());
    }
    let parent = state
        .team
        .get(&id)
        .ok_or_else(|| "上一轮团队任务不存在".to_string())?;
    if matches!(
        parent.phase,
        RunPhase::Planning | RunPhase::AwaitingPlan | RunPhase::Running | RunPhase::Summarizing
    ) {
        return Err("上一轮还在运行或汇总，等结束再来一轮".to_string());
    }
    let unresolved: Vec<String> = parent
        .tasks
        .iter()
        .filter(|task| {
            (matches!(task.merge_state, MergeState::Pending | MergeState::Conflict)
                || !task.changed_files.is_empty())
                && !matches!(task.merge_state, MergeState::Merged | MergeState::Discarded)
        })
        .map(|task| task.title.clone())
        .collect();
    if !unresolved.is_empty() {
        return Err(format!(
            "先处理上一轮的改动（合并或丢弃）再来一轮：{}",
            unresolved.join("；")
        ));
    }
    if state.team.active.lock_recover().values().next().is_some() {
        return Err("已有团队任务在运行，请先等它结束或取消".to_string());
    }
    let workspace = normalize_workspace(&parent.workspace)?;
    let repo = worktree::detect(&workspace)?;
    let workspace_warning = repo.dirty.then(|| {
        "主工作区有未提交改动；worktree 从当前 HEAD 创建，任务看不到这些改动，合并前需要先提交或暂存"
            .to_string()
    });
    let budget_usd = normalize_budget(budget_usd.or(parent.budget_usd))?;
    let concurrency = concurrency
        .unwrap_or(parent.concurrency)
        .clamp(1, 4);
    let previous = previous_round_context(&parent);
    let now = epoch_millis();
    let run = TeamRunView {
        id: next_id("team"),
        goal: instruction.clone(),
        workspace: workspace.to_string_lossy().into_owned(),
        main_agent: "claude".to_string(),
        main_agent_name: "Claude Code".to_string(),
        phase: RunPhase::Planning,
        plan_summary: None,
        summary: None,
        tasks: Vec::new(),
        created_at_ms: now,
        updated_at_ms: now,
        error: None,
        planner_session_id: None,
        budget_usd,
        spent_usd: 0.0,
        repo_root: Some(repo.root.to_string_lossy().into_owned()),
        base_branch: Some(repo.branch),
        base_commit: Some(repo.head),
        worktree_mode: true,
        workspace_warning,
        concurrency,
        parent_run_id: Some(parent.id.clone()),
        round: parent.round.saturating_add(1),
    };
    state.team.insert(run.clone());
    emit_run(&app, &run);
    let allowed = capability::available_agent_ids();
    let prompt = plan::followup_planner_prompt(
        &instruction,
        &workspace,
        &allowed,
        &previous,
    );
    spawn_planner(app, run.id.clone(), prompt, workspace, budget_usd);
    Ok(run)
}

#[tauri::command]
pub(crate) fn team_confirm_plan(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
    id: String,
    tasks: Vec<PlanTask>,
) -> Result<TeamRunView, String> {
    let run = state
        .team
        .get(&id)
        .ok_or_else(|| "团队任务不存在".to_string())?;
    if run.phase != RunPhase::AwaitingPlan {
        return Err("当前阶段不能确认计划".to_string());
    }
    if !state.hook_enabled.load(Ordering::SeqCst) {
        return Err("团队任务需要先开启 Claude 事件（hooks），否则无法审批".to_string());
    }
    if state.team.active.lock_recover().values().next().is_some() {
        return Err("已有团队任务在运行，请先等它结束或取消".to_string());
    }
    let workspace = PathBuf::from(&run.workspace);
    let repo = worktree::detect(&workspace)?;
    let workspace_warning = repo.dirty.then(|| {
        "主工作区有未提交改动；worktree 从当前 HEAD 创建，任务看不到这些改动，合并前需要先提交或暂存"
            .to_string()
    });
    let allowed = capability::available_agent_ids();
    let plan = TeamPlan {
        summary: run.plan_summary.clone().unwrap_or_default(),
        tasks: tasks.clone(),
    };
    plan::validate_plan(&plan, &allowed).map_err(|errors| errors.join("；"))?;
    let views: Vec<TeamTaskView> = plan.tasks.iter().map(from_plan_task).collect();
    let view = state
        .team
        .update(&id, |run| {
            run.phase = RunPhase::Running;
            run.tasks = views;
            run.summary = None;
            run.error = None;
            run.repo_root = Some(repo.root.to_string_lossy().into_owned());
            run.base_branch = Some(repo.branch.clone());
            run.base_commit = Some(repo.head.clone());
            run.worktree_mode = true;
            run.workspace_warning = workspace_warning.clone();
        })
        .ok_or_else(|| "团队任务不存在".to_string())?;
    emit_run(&app, &view);
    let run_id = id.clone();
    std::thread::spawn(move || run_scheduler(app, run_id));
    Ok(view)
}

#[tauri::command]
pub(crate) fn team_retry_run(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
    id: String,
) -> Result<TeamRunView, String> {
    let run = state
        .team
        .get(&id)
        .ok_or_else(|| "团队任务不存在".to_string())?;
    if matches!(run.phase, RunPhase::Planning | RunPhase::Running | RunPhase::Summarizing) {
        return Err("任务还在运行，先取消或等它结束".to_string());
    }
    if state.team.active.lock_recover().values().next().is_some() {
        return Err("已有任务在运行，请先等它结束或取消".to_string());
    }
    if run.repo_root.is_none() {
        return Err("旧运行没有 worktree 信息，请新建一轮".to_string());
    }
    if run
        .budget_usd
        .is_some_and(|budget| budget - run.spent_usd <= 0.005)
    {
        return Err(format!(
            "预算已用完（已花 ${:.2} / 预算 ${:.2}），请新建一轮或提高预算",
            run.spent_usd,
            run.budget_usd.unwrap_or(0.0)
        ));
    }
    if let Some(repo_root) = run.repo_root.as_deref() {
        let root = PathBuf::from(repo_root);
        for task in &run.tasks {
            if task.state == TaskState::Succeeded {
                continue;
            }
            if let (Some(path), Some(branch)) = (&task.worktree_path, &task.branch) {
                worktree::cleanup(&root, &PathBuf::from(path), branch);
            }
        }
    }
    if run.tasks.iter().all(|task| task.state == TaskState::Succeeded) {
        return Err("这一轮已经全部完成，不需要重试".to_string());
    }
    let view = state
        .team
        .update(&id, |run| {
            run.phase = RunPhase::Running;
            run.error = None;
            run.summary = None;
            for task in &mut run.tasks {
                if task.state == TaskState::Succeeded {
                    continue;
                }
                task.state = TaskState::Queued;
                task.started_at_ms = None;
                task.ended_at_ms = None;
                task.exit_code = None;
                task.result = None;
                task.error = None;
                task.cost_usd = None;
                task.verification = None;
                task.log.clear();
                task.session_id = None;
                task.pending_approval = None;
                task.approval_id = None;
                task.worktree_path = None;
                task.branch = None;
                task.base_commit = None;
                task.changed_files.clear();
                task.diff_stat = None;
                task.merge_state = MergeState::None;
                task.merge_error = None;
            }
        })
        .ok_or_else(|| "团队任务不存在".to_string())?;
    emit_run(&app, &view);
    let run_id = id.clone();
    std::thread::spawn(move || run_scheduler(app, run_id));
    Ok(view)
}

#[tauri::command]
pub(crate) fn team_task_diff(
    state: tauri::State<AppState>,
    id: String,
    task_id: String,
) -> Result<String, String> {
    let run = state
        .team
        .get(&id)
        .ok_or_else(|| "团队任务不存在".to_string())?;
    let task = run
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .ok_or_else(|| "任务不存在".to_string())?;
    let root = run
        .repo_root
        .as_deref()
        .ok_or_else(|| "这一轮不是 worktree 模式".to_string())?;
    let base = task
        .base_commit
        .as_deref()
        .or(run.base_commit.as_deref())
        .ok_or_else(|| "缺少基准 commit".to_string())?;
    let branch = task
        .branch
        .as_deref()
        .ok_or_else(|| "这个任务没有 worktree 分支".to_string())?;
    worktree::diff(Path::new(root), base, branch)
}

#[tauri::command]
pub(crate) fn team_merge_task(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
    id: String,
    task_id: String,
) -> Result<TeamRunView, String> {
    let run = state
        .team
        .get(&id)
        .ok_or_else(|| "团队任务不存在".to_string())?;
    let task = run
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .ok_or_else(|| "任务不存在".to_string())?;
    if task.state != TaskState::Succeeded {
        return Err("只有已完成的任务才能合并".to_string());
    }
    if matches!(
        run.phase,
        RunPhase::Planning | RunPhase::AwaitingPlan | RunPhase::Running | RunPhase::Summarizing
    ) {
        return Err("这一轮还在运行或汇总，等结束后再合并".to_string());
    }
    if let Some(blocker) = dependency_blocker(&run.tasks, task) {
        return Err(blocker);
    }
    if task.merge_state != MergeState::Pending {
        return Err("这个任务没有待合并的改动".to_string());
    }
    if task.changed_files.is_empty() {
        return Err("这个任务没有文件改动".to_string());
    }
    let root = run
        .repo_root
        .as_deref()
        .ok_or_else(|| "这一轮不是 worktree 模式".to_string())?;
    let branch = task
        .branch
        .as_deref()
        .ok_or_else(|| "这个任务没有 worktree 分支".to_string())?;
    if let Err(error) = worktree::merge(Path::new(root), branch) {
        let conflict = error.contains("合并失败");
        let view = state.team.update(&id, |run| {
            if let Some(task) = run.tasks.iter_mut().find(|task| task.id == task_id) {
                task.merge_state = if conflict {
                    MergeState::Conflict
                } else {
                    MergeState::Pending
                };
                task.merge_error = Some(error.clone());
            }
        });
        if let Some(view) = view {
            emit_run(&app, &view);
        }
        return Err(error);
    }
    if let Some(path) = task.worktree_path.as_deref() {
        worktree::cleanup_after_merge(Path::new(root), Path::new(path));
    }
    let view = state
        .team
        .update(&id, |run| {
            if let Some(task) = run.tasks.iter_mut().find(|task| task.id == task_id) {
                task.merge_state = MergeState::Merged;
                task.merge_error = None;
                task.worktree_path = None;
            }
        })
        .ok_or_else(|| "团队任务不存在".to_string())?;
    emit_run(&app, &view);
    Ok(view)
}

#[tauri::command]
pub(crate) fn team_discard_task(
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
    id: String,
    task_id: String,
) -> Result<TeamRunView, String> {
    let run = state
        .team
        .get(&id)
        .ok_or_else(|| "团队任务不存在".to_string())?;
    let task = run
        .tasks
        .iter()
        .find(|task| task.id == task_id)
        .ok_or_else(|| "任务不存在".to_string())?;
    let ids = dependent_ids(&run.tasks, &task_id);
    for dependent in run.tasks.iter().filter(|task| ids.contains(&task.id)) {
        if matches!(
            dependent.state,
            TaskState::Running | TaskState::WaitingApproval | TaskState::Queued
        ) {
            return Err(format!(
                "依赖任务「{}」还在执行，先取消再丢弃",
                dependent.title
            ));
        }
        if dependent.merge_state == MergeState::Merged {
            return Err(format!(
                "「{}」已经合并到主工作区，不能丢弃依赖链；需要回滚请用 git revert",
                dependent.title
            ));
        }
    }
    let root = run
        .repo_root
        .as_deref()
        .ok_or_else(|| "这一轮不是 worktree 模式".to_string())?;
    for dependent in run.tasks.iter().filter(|task| ids.contains(&task.id)) {
        let path = dependent.worktree_path.clone();
        let branch = dependent.branch.clone();
        if let (Some(path), Some(branch)) = (path.as_deref(), branch.as_deref()) {
            worktree::cleanup(Path::new(root), Path::new(path), branch);
        }
    }
    let root_title = task.title.clone();
    let view = state
        .team
        .update(&id, |run| {
            for task in &mut run.tasks {
                if !ids.contains(&task.id) {
                    continue;
                }
                task.merge_state = MergeState::Discarded;
                task.merge_error = (task.id != task_id)
                    .then(|| format!("依赖任务「{root_title}」已丢弃"));
                task.worktree_path = None;
                task.branch = None;
            }
        })
        .ok_or_else(|| "团队任务不存在".to_string())?;
    emit_run(&app, &view);
    Ok(view)
}

#[tauri::command]
pub(crate) fn team_cancel_run(    app: tauri::AppHandle,
    state: tauri::State<AppState>,
    id: String,
) -> Result<TeamRunView, String> {
    let Some(run) = state.team.get(&id) else {
        return Err("团队任务不存在".to_string());
    };
    if matches!(
        run.phase,
        RunPhase::Succeeded | RunPhase::Failed | RunPhase::Cancelled
    ) {
        return Ok(run);
    }
    let pids: Vec<u32> = {
        let active = state.team.active.lock_recover();
        active
            .iter()
            .filter(|(key, _)| key.starts_with(&format!("{id}:")))
            .map(|(_, pid)| *pid)
            .collect()
    };
    for pid in pids {
        runner::kill_process_tree(pid);
    }
    let view = state
        .team
        .update(&id, |run| {
            run.phase = RunPhase::Cancelled;
            run.error = Some("已取消".to_string());
            for task in &mut run.tasks {
                match task.state {
                    TaskState::Running | TaskState::WaitingApproval => {
                        task.state = TaskState::Cancelled;
                        task.error = Some("已取消".to_string());
                        task.ended_at_ms = Some(epoch_millis());
                        task.pending_approval = None;
                        task.approval_id = None;
                    }
                    TaskState::Queued => {
                        task.state = TaskState::Skipped;
                        task.error = Some("运行已取消".to_string());
                        task.ended_at_ms = Some(epoch_millis());
                    }
                    _ => {}
                }
            }
        })
        .ok_or_else(|| "团队任务不存在".to_string())?;
    emit_run(&app, &view);
    Ok(view)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str, state: TaskState, depends_on: &[&str]) -> TeamTaskView {
        TeamTaskView {
            id: id.into(),
            title: id.into(),
            assignee: "claude".into(),
            assignee_name: "Claude Code".into(),
            prompt: "做事".into(),
            depends_on: depends_on.iter().map(|value| (*value).to_string()).collect(),
            acceptance: "可验收".into(),
            verify_command: None,
            verification: None,
            state,
            started_at_ms: None,
            ended_at_ms: None,
            exit_code: None,
            result: None,
            error: None,
            cost_usd: None,
            log: Vec::new(),
            session_id: None,
            pending_approval: None,
            approval_id: None,
            worktree_path: None,
            branch: None,
            base_commit: None,
            changed_files: Vec::new(),
            diff_stat: None,
            merge_state: MergeState::None,
            merge_error: None,
        }
    }

    #[test]
    fn picks_the_first_ready_task_and_skips_blocked_dependents() {
        let tasks = vec![
            task("a", TaskState::Queued, &[]),
            task("b", TaskState::Queued, &["a"]),
        ];
        assert!(matches!(pick_task(&tasks), Pick::Ready(0)));
        let blocked = vec![
            task("a", TaskState::Failed, &[]),
            task("b", TaskState::Queued, &["a"]),
        ];
        assert!(matches!(pick_task(&blocked), Pick::Blocked(1)));
    }

    #[test]
    fn dependent_ids_cascade_a_linear_chain() {
        let tasks = vec![
            task("a", TaskState::Succeeded, &[]),
            task("b", TaskState::Succeeded, &["a"]),
            task("c", TaskState::Succeeded, &["b"]),
            task("independent", TaskState::Succeeded, &[]),
        ];
        let ids = dependent_ids(&tasks, "a");
        assert_eq!(ids.len(), 3);
        assert!(ids.contains("a") && ids.contains("b") && ids.contains("c"));
        assert!(!ids.contains("independent"));
    }

    #[test]
    fn dependency_blocker_requires_a_merged_dependency() {
        let mut dependency = task("a", TaskState::Succeeded, &[]);
        dependency.changed_files = vec!["a.txt".into()];
        dependency.merge_state = MergeState::Pending;
        let dependent = task("b", TaskState::Queued, &["a"]);
        let blocked = dependency_blocker(&[dependency.clone(), dependent.clone()], &dependent);
        assert!(blocked.unwrap().contains("先合并依赖任务"));

        dependency.merge_state = MergeState::Merged;
        assert!(dependency_blocker(&[dependency.clone(), dependent.clone()], &dependent).is_none());

        dependency.merge_state = MergeState::Discarded;
        let discarded = dependency_blocker(&[dependency, dependent.clone()], &dependent).unwrap();
        assert!(discarded.contains("已丢弃"));
    }

    #[test]
    fn uuid_is_version_four_and_unique() {
        let first = new_uuid();
        let second = new_uuid();
        assert_ne!(first, second);
        assert_eq!(first.len(), 36);
        assert_eq!(&first[14..15], "4");
        assert!(matches!(&first[19..20], "8" | "9" | "a" | "b"));
    }

    #[test]
    fn managed_sessions_are_tracked_for_team_hooks() {
        let state = TeamState {
            runs: Mutex::new(HashMap::new()),
            active: Mutex::new(HashMap::new()),
            managed_sessions: Mutex::new(HashMap::new()),
        };
        assert!(state.task_for_session("s").is_none());
        state.register_session("s", "run-1", "task-1", Path::new("C:\\worktree"));
        assert!(state.task_for_session("s").is_some());
        assert_eq!(
            state.task_for_session("s"),
            Some(("run-1".to_string(), "task-1".to_string()))
        );
        state.unregister_session("s");
        assert!(state.task_for_session("s").is_none());
    }

    #[test]
    fn workspace_display_strips_the_windows_verbatim_prefix() {
        let dir = live_dir("workspace");
        let normalized = normalize_workspace(&dir.to_string_lossy()).expect("workspace");
        let text = normalized.to_string_lossy();
        assert!(!text.starts_with(r"\\?\"), "界面不应显示 verbatim 前缀：{text}");
        let _ = fs::remove_dir_all(&dir);
    }

    fn outcome(status: Option<&str>) -> ProcessOutcome {
        ProcessOutcome {
            ok: true,
            result: Some("已完成".into()),
            structured: status.map(|status| {
                serde_json::json!({
                    "status": status,
                    "summary": "创建了文件",
                    "files_changed": ["a.txt"],
                    "verification": "未验证"
                })
            }),
            session_id: "s".into(),
            ..ProcessOutcome::default()
        }
    }

    #[test]
    fn blocked_worker_is_not_a_success() {
        let (state, display, error) = worker_report(&outcome(Some("blocked")));
        assert_eq!(state, TaskState::Failed);
        assert!(display.unwrap().contains("创建了文件"));
        assert!(error.unwrap().contains("创建了文件"));
    }

    #[test]
    fn completed_worker_is_a_success() {
        let (state, display, error) = worker_report(&outcome(Some("completed")));
        assert_eq!(state, TaskState::Succeeded);
        assert!(display.unwrap().contains("验证：未验证"));
        assert!(error.is_none());
    }

    #[test]
    fn worker_without_structured_output_is_not_a_success() {
        let (state, _, error) = worker_report(&outcome(None));
        assert_eq!(state, TaskState::Failed);
        assert!(error.unwrap().contains("结构化"));
    }

    #[test]
    fn budget_must_be_a_positive_sane_amount() {
        assert_eq!(normalize_budget(Some(2.0)).unwrap(), Some(2.0));
        assert_eq!(normalize_budget(Some(0.129)).unwrap(), Some(0.13));
        assert!(normalize_budget(Some(0.0)).is_err());
        assert!(normalize_budget(Some(-1.0)).is_err());
        assert!(normalize_budget(Some(101.0)).is_err());
        assert_eq!(normalize_budget(None).unwrap(), None);
    }

    #[test]
    fn old_run_records_without_budget_default_to_no_cap() {
        let view = TeamRunView {
            id: "r".into(),
            goal: "g".into(),
            workspace: "C:\\work".into(),
            main_agent: "claude".into(),
            main_agent_name: "Claude Code".into(),
            phase: RunPhase::Succeeded,
            plan_summary: None,
            summary: None,
            tasks: vec![task("a", TaskState::Succeeded, &[])],
            created_at_ms: 1,
            updated_at_ms: 1,
            error: None,
            planner_session_id: None,
            budget_usd: Some(2.0),
            spent_usd: 0.5,
            repo_root: None,
            base_branch: None,
            base_commit: None,
            worktree_mode: false,
            workspace_warning: None,
            concurrency: 1,
            parent_run_id: None,
            round: 1,
        };
        let mut value = serde_json::to_value(&view).unwrap();
        value.as_object_mut().unwrap().remove("budgetUsd");
        value.as_object_mut().unwrap().remove("spentUsd");
        let parsed: TeamRunView = serde_json::from_value(value).unwrap();
        assert_eq!(parsed.budget_usd, None);
        assert_eq!(parsed.spent_usd, 0.0);
    }

    fn live_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "agent-island-live-{tag}-{}-{}",
            std::process::id(),
            epoch_millis()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 真实调用 Claude 的规划链路。默认忽略，手动运行：
    /// `cargo test --lib live_planner -- --ignored --nocapture`
    #[test]
    #[ignore = "live Claude call; costs tokens"]
    fn live_planner_returns_a_valid_plan() {
        let dir = live_dir("planner");
        let prompt = plan::planner_prompt(
            "Create a file named island_live.txt containing LIVE_OK. Do not change anything else.",
            &dir,
            &capability::available_agent_ids(),
        );
        let plan = planner_call(
            &prompt,
            &dir,
            None,
            |_| {},
            |_| {},
        )
        .expect("planner should return a valid plan");
        assert!(!plan.0.tasks.is_empty());
        assert!(plan
            .0
            .tasks
            .iter()
            .all(|task| task.assignee == "claude"));
        let _ = fs::remove_dir_all(&dir);
    }

    /// 真实调用 Claude 汇总。默认忽略，手动运行：
    /// `cargo test --lib live_synthesis -- --ignored --nocapture`
    #[test]
    #[ignore = "live Claude call; costs tokens"]
    fn live_synthesis_returns_text() {
        let dir = live_dir("synthesis");
        let mut finished = task("a", TaskState::Succeeded, &[]);
        finished.title = "Create island_live.txt".into();
        finished.result = Some("Created island_live.txt with LIVE_OK.".into());
        let (text, _cost) = synthesis_call(
            "Create a marker file and report what changed",
            &dir,
            &[finished],
            None,
            |_| {},
        )
        .expect("synthesis should return text");
        assert!(!text.trim().is_empty());
        assert!(text.chars().count() > 20);
        let _ = fs::remove_dir_all(&dir);
    }

    /// 真实调用 Claude worker 写文件。默认忽略，手动运行：
    /// `cargo test --lib live_worker -- --ignored --nocapture`
    #[test]
    #[ignore = "live Claude call; costs tokens"]
    fn live_worker_can_write_a_file() {
        let dir = live_dir("worker");
        let run_id = format!("live-{}", epoch_millis());
        let settings = worker_settings_path(&run_id).expect("worker settings");
        let program = capability::claude_program().expect("claude");
        let env = capability::claude_settings_env();
        let task = PlanTask {
            id: "a".into(),
            title: "Write the marker file".into(),
            assignee: "claude".into(),
            prompt: "Create island_live.txt in the current directory with exact content LIVE_OK and a trailing newline."
                .into(),
            depends_on: Vec::new(),
            acceptance: "File exists with LIVE_OK".into(),
            verify_command: None,
        };
        let prompt = plan::worker_prompt("Write a marker file", &dir, &task);
        let session_id = new_uuid();
        let schema = plan::worker_schema().to_string();
        let config = RunnerConfig {
            program: &program,
            cwd: &dir,
            prompt: &prompt,
            settings: Some(&settings),
            session_id: &session_id,
            permission_mode: "acceptEdits",
            max_turns: 8,
            timeout: Duration::from_secs(180),
            output_format: "stream-json",
            json_schema: Some(&schema),
            max_budget_usd: None,
            env: &env,
        };
        let outcome = runner::run_claude(&config, |_| {});
        assert!(outcome.ok, "worker failed: {:?}", outcome.error);
        assert_eq!(
            outcome
                .structured
                .as_ref()
                .and_then(|value| value.get("status"))
                .and_then(Value::as_str),
            Some("completed"),
            "worker 应自报 completed"
        );
        let content = fs::read_to_string(dir.join("island_live.txt")).expect("file");
        assert_eq!(content.trim(), "LIVE_OK");
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(team_run_dir(&run_id));
    }
}
