//! 主 Agent 的计划协议：schema、提示词、校验。
//!
//! 计划是团队任务里唯一跨进程的"契约"。这里把它做成可单测的纯函数：
//! - schema 交给 Claude 的 `--json-schema`，让模型必须返回合法结构；
//! - 校验不信任模型输出（负责人、依赖、环、数量都要重新检查）；
//! - 提示词明确"worker 看不到本次对话"，逼主 Agent 把任务写成自包含的。

use crate::*;
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub(crate) const MAX_TASKS: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PlanTask {
    pub id: String,
    pub title: String,
    pub assignee: String,
    pub prompt: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub acceptance: String,
    /// 在任务 worktree 内执行的验收命令；通过才允许合并。
    #[serde(default)]
    pub verify_command: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TeamPlan {
    #[serde(default)]
    pub summary: String,
    pub tasks: Vec<PlanTask>,
}

/// Claude 的结构化输出 schema。负责人只允许当前真正可用的 Agent。
pub(crate) fn plan_schema(allowed_agents: &[String]) -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "summary": { "type": "string" },
            "tasks": {
                "type": "array",
                "minItems": 1,
                "maxItems": MAX_TASKS,
                "items": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "title": { "type": "string" },
                        "assignee": { "type": "string", "enum": allowed_agents },
                        "prompt": { "type": "string" },
                        "depends_on": {
                            "type": "array",
                            "items": { "type": "string" }
                        },
                        "acceptance": { "type": "string" },
                        "verify_command": { "type": "string" }
                    },
                    "required": ["id", "title", "assignee", "prompt", "depends_on", "acceptance", "verify_command"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["summary", "tasks"],
        "additionalProperties": false
    })
}

fn workspace_entries(workspace: &Path) -> Vec<String> {
    let Ok(reader) = fs::read_dir(workspace) else {
        return Vec::new();
    };
    let mut entries: Vec<String> = reader
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') && name != ".claude" {
                return None;
            }
            let suffix = if entry.path().is_dir() { "/" } else { "" };
            Some(format!("{name}{suffix}"))
        })
        .take(40)
        .collect();
    entries.sort();
    entries
}

pub(crate) fn planner_prompt(goal: &str, workspace: &Path, agents: &[String]) -> String {
    let entries = workspace_entries(workspace);
    let listing = if entries.is_empty() {
        "（目录为空或不可读）".to_string()
    } else {
        entries.join("  ")
    };
    format!(
        "你是 Agent Island 的团队主 Agent。你只负责把目标拆成任务，不执行任务。\n\n\
         工作区：{workspace}\n\
         工作区顶层：{listing}\n\
         目标：{goal}\n\n\
         可派发的次级 Agent：{agents}\n\n\
         规则：\n\
         - 返回 1–{max} 个任务；任务越少越好，能一次做完就不要拆。\n\
         - 每个任务会交给一个**看不到本次对话**的 Agent，所以任务 prompt 必须自包含：背景、要改的文件或目录、验收标准、不要做什么。\n\
         - 可以用 depends_on 表示先后顺序：每个任务最多一个依赖，且一个任务最多被一个后续任务依赖（线性链）；不允许多依赖或分叉。\n\
         - 彼此独立的步骤不要加依赖，能并行就并行；依赖任务会从依赖任务的分支创建 worktree，能看到依赖的改动，但 prompt 仍要写清楚背景。\n\
         - 不要安排必须等用户确认才能开始的任务；不要编造目标里没有的文件或需求。\n\
         - 如果一个命令能验证任务（如 npm test -- xxx、cargo test xxx），填 verify_command；没有合适命令就填空字符串。验收命令会在任务 worktree 内执行，退出码 0 才算通过。\n\
         - 优先小步、可验证；每个任务都要能独立验收。\n",
        workspace = workspace.display(),
        listing = listing,
        goal = goal.trim(),
        agents = agents.join("、"),
        max = MAX_TASKS
    )
}

/// worker 的收尾契约：退出码只能说明进程结束，不能说明任务完成。
/// 用 schema 强制 worker 自报 completed / blocked / failed，避免"说了未完成
/// 却被记成成功"的假答案。
pub(crate) fn worker_schema() -> Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "status": {
                "type": "string",
                "enum": ["completed", "blocked", "failed"]
            },
            "summary": { "type": "string" },
            "files_changed": {
                "type": "array",
                "items": { "type": "string" }
            },
            "verification": { "type": "string" }
        },
        "required": ["status", "summary", "files_changed", "verification"],
        "additionalProperties": false
    })
}

/// 发给 worker 的完整提示词。worker 不知道主 Agent 的对话，这里补上最少上下文。
pub(crate) fn worker_prompt(goal: &str, workspace: &Path, task: &PlanTask) -> String {
    format!(
        "你是 Agent Island 派发的次级 Agent。你只负责下面这一项任务，不要扩展到其他任务。\n\n\
         团队目标（仅作背景，不需要你重新规划）：{goal}\n\
         工作区：{workspace}\n\
         任务：{title}\n\
         验收标准：{acceptance}\n\n\
         {prompt}\n\n\
         执行要求：\n\
         - 直接开始；不要询问用户。\n\
         - 不能完成时说明原因并停止，不要伪造结果。\n\
         - 写文件优先用 Write / Edit 工具；不要用 shell 的 > / >> 重定向创建文件。\n\
         - 修改文件后，在最后一条消息里列出改了哪些文件。\n\
         - 不要执行 git push、发布、删除工作区之外的文件。\n\
         - 结束时必须调用结构化输出：只有确实完成验收标准才填 completed；\n\
           权限被拒、依赖缺失或做不到，填 blocked 或 failed。\n\
         - summary 写实际做了什么；files_changed 写真实改动的文件；\n\
           verification 写实际跑过的验证，没跑就明确写\"未验证\"。\n\
         - 系统会在你结束后独立执行计划里的验收命令；不要伪造命令输出或声称已验证未运行的东西。\n",
        goal = goal.trim(),
        workspace = workspace.display(),
        title = task.title.trim(),
        acceptance = task.acceptance.trim(),
        prompt = task.prompt.trim()
    )
}

pub(crate) fn synthesis_prompt(goal: &str, workspace: &Path, tasks: &[PlanTask], results: &[String]) -> String {
    let mut lines = String::new();
    for (task, result) in tasks.iter().zip(results.iter()) {
        let capped: String = if result.chars().count() > 4_000 {
            let mut value: String = result.chars().take(4_000).collect();
            value.push('…');
            value
        } else {
            result.clone()
        };
        lines.push_str(&format!(
            "### {}（{}，{}）\n{}\n\n",
            task.title,
            task.assignee,
            task.id,
            capped.trim()
        ));
    }
    format!(
        "你是团队主 Agent。下面是团队执行结果，请用中文写一份简短汇总，不要重新规划。\n\n\
         目标：{goal}\n\
         工作区：{workspace}\n\n\
         {lines}\n\
         要求：\n\
         - 只根据上面的结果陈述，不要把没有证据的验证写成已通过。\n\
         - 分别写：完成了什么、改了哪些文件、哪些还没验证、冲突或风险、建议下一步。\n\
         - 200–400 字，直白，不夸张。\n",
        goal = goal.trim(),
        workspace = workspace.display(),
        lines = lines
    )
}

/// 下一轮规划：把上一轮的目标、汇总和任务结果带给主 Agent，避免它从零开始。
pub(crate) fn followup_planner_prompt(
    instruction: &str,
    workspace: &Path,
    agents: &[String],
    previous: &str,
) -> String {
    let entries = workspace_entries(workspace);
    let listing = if entries.is_empty() {
        "（目录为空或不可读）".to_string()
    } else {
        entries.join("  ")
    };
    format!(
        "你是 Agent Island 的团队主 Agent，现在在做下一轮规划。你只负责拆解任务，不执行任务。\n\n\
         工作区：{workspace}\n\
         工作区顶层：{listing}\n\
         用户对这一轮的指令：{instruction}\n\n\
         上一轮上下文：\n{previous}\n\n\
         可派发的次级 Agent：{agents}\n\n\
         规则：\n\
         - 返回 1–{max} 个任务；任务越少越好，能一次做完就不要拆。\n\
         - 每个任务会交给一个**看不到本次对话**的 Agent，prompt 必须自包含：背景、要改的文件、验收标准、不要做什么。\n\
         - 可以用 depends_on 表示先后顺序：每个任务最多一个依赖，且一个任务最多被一个后续任务依赖（线性链）；彼此独立的步骤不要加依赖。\n\
         - 依赖任务会从依赖任务的分支创建 worktree，能看到依赖的改动。\n\
         - 基于上一轮的实际结果规划；上一轮没验证的东西可以安排验证，但不要把推断写成已验证。\n\
         - 如果一个命令能验证任务，填 verify_command；没有就填空字符串。\n\
         - 不要重复上一轮已经成功且用户已合并的改动。\n\
         - 不要安排必须等用户确认才能开始的任务。\n",
        workspace = workspace.display(),
        listing = listing,
        instruction = instruction.trim(),
        previous = previous.trim(),
        agents = agents.join("、"),
        max = MAX_TASKS
    )
}

/// 不信任模型：负责人、依赖、环、数量、空字段都重新检查。
pub(crate) fn validate_plan(plan: &TeamPlan, allowed_agents: &[String]) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    if plan.tasks.is_empty() {
        errors.push("计划里没有任务".to_string());
    }
    if plan.tasks.len() > MAX_TASKS {
        errors.push(format!("任务过多：{}（上限 {MAX_TASKS}）", plan.tasks.len()));
    }
    let allowed: HashSet<&str> = allowed_agents.iter().map(String::as_str).collect();
    let mut ids = HashSet::new();
    for task in &plan.tasks {
        let id = task.id.trim();
        if id.is_empty() {
            errors.push("任务 id 不能为空".to_string());
        } else if !ids.insert(id.to_string()) {
            errors.push(format!("任务 id 重复：{id}"));
        }
        if task.title.trim().is_empty() {
            errors.push(format!("任务 {id} 缺少标题"));
        }
        if task.prompt.trim().is_empty() {
            errors.push(format!("任务 {id} 缺少 prompt"));
        }
        if !allowed.contains(task.assignee.trim()) {
            errors.push(format!(
                "任务 {id} 的负责人 {} 不在可用列表里",
                task.assignee
            ));
        }
        if task
            .verify_command
            .as_deref()
            .is_some_and(|command| command.chars().count() > 1_000)
        {
            errors.push(format!("任务 {id} 的验收命令太长（上限 1000 字）"));
        }
    }
    let mut dependents: HashMap<&str, usize> = HashMap::new();
    for task in &plan.tasks {
        if task.depends_on.len() > 1 {
            errors.push(format!(
                "任务 {} 有多个依赖；当前只支持单个依赖的线性链",
                task.id
            ));
        }
        for dependency in &task.depends_on {
            if dependency == &task.id {
                errors.push(format!("任务 {} 依赖了自己", task.id));
            } else if !ids.contains(dependency) {
                errors.push(format!("任务 {} 依赖了不存在的 {}", task.id, dependency));
            } else {
                *dependents.entry(dependency.as_str()).or_insert(0) += 1;
            }
        }
    }
    for (id, count) in dependents {
        if count > 1 {
            errors.push(format!(
                "任务 {id} 被多个任务依赖；当前只支持一条线性链，不允许分叉"
            ));
        }
    }
    if has_cycle(&plan.tasks) {
        errors.push("任务依赖存在环".to_string());
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn has_cycle(tasks: &[PlanTask]) -> bool {
    let by_id: HashMap<&str, &PlanTask> = tasks.iter().map(|task| (task.id.as_str(), task)).collect();
    let mut state: HashMap<&str, u8> = HashMap::new();
    fn visit<'a>(
        id: &'a str,
        by_id: &HashMap<&'a str, &'a PlanTask>,
        state: &mut HashMap<&'a str, u8>,
    ) -> bool {
        match state.get(id).copied().unwrap_or(0) {
            1 => return true,
            2 => return false,
            _ => {}
        }
        state.insert(id, 1);
        if let Some(task) = by_id.get(id) {
            for dependency in &task.depends_on {
                if visit(dependency.as_str(), by_id, state) {
                    return true;
                }
            }
        }
        state.insert(id, 2);
        false
    }
    tasks
        .iter()
        .any(|task| visit(task.id.as_str(), &by_id, &mut state))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str, assignee: &str, depends_on: &[&str]) -> PlanTask {
        PlanTask {
            id: id.into(),
            title: format!("任务 {id}"),
            assignee: assignee.into(),
            prompt: "做点具体的事".into(),
            depends_on: depends_on.iter().map(|value| (*value).to_string()).collect(),
            acceptance: "能验证".into(),
            verify_command: None,
        }
    }

    fn allowed() -> Vec<String> {
        vec!["claude".to_string()]
    }

    #[test]
    fn accepts_a_linear_chain() {
        let plan = TeamPlan {
            summary: "拆成两步".into(),
            tasks: vec![
                task("a", "claude", &[]),
                task("b", "claude", &["a"]),
                task("c", "claude", &["b"]),
            ],
        };
        assert_eq!(validate_plan(&plan, &allowed()), Ok(()));
    }

    #[test]
    fn accepts_parallel_roots() {
        let plan = TeamPlan {
            summary: "两条独立链".into(),
            tasks: vec![
                task("a", "claude", &[]),
                task("b", "claude", &["a"]),
                task("c", "claude", &[]),
            ],
        };
        assert_eq!(validate_plan(&plan, &allowed()), Ok(()));
    }

    #[test]
    fn rejects_fan_out_and_multiple_dependencies() {
        let fan_out = TeamPlan {
            summary: String::new(),
            tasks: vec![
                task("a", "claude", &[]),
                task("b", "claude", &["a"]),
                task("c", "claude", &["a"]),
            ],
        };
        let errors = validate_plan(&fan_out, &allowed()).unwrap_err();
        assert!(errors.iter().any(|error| error.contains("分叉")));

        let multi_parent = TeamPlan {
            summary: String::new(),
            tasks: vec![
                task("a", "claude", &[]),
                task("b", "claude", &[]),
                task("c", "claude", &["a", "b"]),
            ],
        };
        let errors = validate_plan(&multi_parent, &allowed()).unwrap_err();
        assert!(errors.iter().any(|error| error.contains("多个依赖")));
    }

    #[test]
    fn rejects_unknown_assignee_and_missing_dependency() {
        let plan = TeamPlan {
            summary: String::new(),
            tasks: vec![task("a", "codex", &["missing"])],
        };
        let errors = validate_plan(&plan, &allowed()).unwrap_err();
        assert!(errors.iter().any(|error| error.contains("不在可用列表")));
        assert!(errors.iter().any(|error| error.contains("不存在的")));
    }

    #[test]
    fn rejects_cycle_and_duplicate_ids() {
        let duplicate = TeamPlan {
            summary: String::new(),
            tasks: vec![task("a", "claude", &[]), task("a", "claude", &[])],
        };
        let duplicate_errors = validate_plan(&duplicate, &allowed()).unwrap_err();
        assert!(duplicate_errors.iter().any(|error| error.contains("重复")));

        let cycle = TeamPlan {
            summary: String::new(),
            tasks: vec![task("a", "claude", &["b"]), task("b", "claude", &["a"])],
        };
        let cycle_errors = validate_plan(&cycle, &allowed()).unwrap_err();
        assert!(cycle_errors.iter().any(|error| error.contains("环")));
    }

    #[test]
    fn schema_requires_every_plan_field() {
        let schema = plan_schema(&allowed());
        let required = schema["properties"]["tasks"]["items"]["required"]
            .as_array()
            .unwrap();
        for field in [
            "id",
            "title",
            "assignee",
            "prompt",
            "depends_on",
            "acceptance",
            "verify_command",
        ] {
            assert!(
                required.iter().any(|value| value == field),
                "missing {field}"
            );
        }
    }

    #[test]
    fn worker_prompt_is_self_contained() {
        let prompt = worker_prompt("做一个按钮", Path::new("C:/work"), &task("a", "claude", &[]));
        assert!(prompt.contains("C:/work"));
        assert!(prompt.contains("不要询问用户"));
        assert!(prompt.contains("任务：任务 a"));
    }

    #[test]
    fn followup_prompt_carries_previous_context_and_chain_rule() {
        let prompt = followup_planner_prompt(
            "补齐上一轮没验证的测试",
            Path::new("C:/work"),
            &allowed(),
            "上一轮：已创建 A.txt；验证：未验证",
        );
        assert!(prompt.contains("补齐上一轮没验证的测试"));
        assert!(prompt.contains("上一轮：已创建 A.txt"));
        assert!(prompt.contains("depends_on"));
        assert!(prompt.contains("线性链"));
    }
}
