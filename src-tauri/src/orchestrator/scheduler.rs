//! 独立任务的并行调度。
//!
//! 前提：计划已拒绝依赖任务，所有 worktree 从同一个 base 创建。这里只负责
//! 在并发上限内多起几个 worker，每个 worker 仍有自己的 worktree；合并仍然
//! 由用户在团队页逐个确认。
//!
//! 预算按"已完成花费 + 正在跑的预留"计算，避免两个并发任务各按剩余预算
//! 拿到上限、最后总和超支。

use super::*;
use std::sync::mpsc::{channel, Sender};
use std::time::Duration;

pub(crate) fn run_parallel(app: tauri::AppHandle, run_id: String) {
    let (tx, rx) = channel::<String>();
    let mut running: usize = 0;
    let mut caps: HashMap<String, f64> = HashMap::new();
    let mut budget_exhausted = false;

    loop {
        let state = app.state::<AppState>();
        let Some(run) = state.team.get(&run_id) else {
            return;
        };
        if matches!(
            run.phase,
            RunPhase::Cancelled | RunPhase::Failed | RunPhase::Succeeded
        ) {
            break;
        }
        let concurrency = run.concurrency.max(1) as usize;

        while running < concurrency {
            let state = app.state::<AppState>();
            let Some(run) = state.team.get(&run_id) else {
                return;
            };
            if matches!(run.phase, RunPhase::Cancelled | RunPhase::Failed) {
                break;
            }
            if !run.tasks.iter().any(|task| task.state == TaskState::Queued) {
                break;
            }
            let slots = concurrency.saturating_sub(running).max(1) as f64;
            let reserved: f64 = caps.values().sum();
            let available = run
                .budget_usd
                .map(|budget| (budget - run.spent_usd - reserved).max(0.0));
            if available.is_some_and(|value| value <= 0.005) {
                budget_exhausted = true;
                break;
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
                Pick::None => break,
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
                break;
            };
            let repo_path = PathBuf::from(&repo_root);
            let base_commit = match task_base_commit(&run, &task, &repo_path) {
                Ok(commit) => commit,
                Err(error) => {
                    fail_task_setup(&app, &state, &run_id, &task.id, error);
                    break;
                }
            };
            let mut repo = match worktree::detect(&repo_path) {
                Ok(repo) => repo,
                Err(error) => {
                    fail_task_setup(&app, &state, &run_id, &task.id, error);
                    break;
                }
            };
            repo.head = base_commit;
            let task_worktree = match worktree::create(&repo, &run_id, &task.id) {
                Ok(worktree) => worktree,
                Err(error) => {
                    fail_task_setup(&app, &state, &run_id, &task.id, error);
                    break;
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
                break;
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

            let task_cap = available.map(|value| value / slots);
            if let Some(cap) = task_cap {
                caps.insert(task.id.clone(), cap);
            }
            spawn_worker(
                app.clone(),
                run_id.clone(),
                task.clone(),
                task_workspace,
                task_worktree,
                run.goal.clone(),
                task_cap,
                tx.clone(),
            );
            running += 1;
        }

        if budget_exhausted {
            while running > 0 {
                if let Ok(finished) = rx.recv() {
                    running = running.saturating_sub(1);
                    caps.remove(&finished);
                }
            }
            break;
        }
        if running == 0 {
            let state = app.state::<AppState>();
            if let Some(run) = state.team.get(&run_id) {
                if run.tasks.iter().any(|task| task.state == TaskState::Queued) {
                    if let Some(view) = state.team.update(&run_id, |run| {
                        run.phase = RunPhase::Failed;
                        run.error = Some("任务依赖无法满足，未完成".to_string());
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
            }
            break;
        }
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(finished) => {
                running = running.saturating_sub(1);
                caps.remove(&finished);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    let state = app.state::<AppState>();
    let Some(run) = state.team.get(&run_id) else {
        return;
    };
    if run.phase == RunPhase::Cancelled {
        return;
    }
    if budget_exhausted {
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
            &run.main_agent,
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

fn spawn_worker(
    app: tauri::AppHandle,
    run_id: String,
    task: TeamTaskView,
    task_workspace: PathBuf,
    task_worktree: worktree::TaskWorktree,
    goal: String,
    max_budget_usd: Option<f64>,
    tx: Sender<String>,
) {
    std::thread::spawn(move || {
        let task_id = task.id.clone();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let state = app.state::<AppState>();
            let outcome = execute_task(
                &app,
                &run_id,
                &task,
                &task_workspace,
                &task_worktree.path,
                &goal,
                max_budget_usd,
            );
            if state
                .team
                .get(&run_id)
                .is_some_and(|run| run.phase == RunPhase::Cancelled)
            {
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
            let _ = apply_task_completion(
                &app,
                &state,
                &run_id,
                &task,
                &outcome,
                &finalized,
                verification,
                ended,
            );
        }));
        if result.is_err() {
            log_event("ERROR", "team", &format!("worker panicked task={task_id}"));
        }
        let _ = tx.send(task_id);
    });
}
