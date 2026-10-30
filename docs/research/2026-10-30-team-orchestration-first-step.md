# 第一步设计：团队任务的审批、状态与入口

日期：2026-10-30。范围：团队编排的**第一步**——把已经跑通的骨架补成可长期使用的最小闭环；并行、预算、Codex 适配留给下一步。

## 调研到的好做法

### 1. 队列化查看 + 动作级审批（WorkOS）

[Curating queues beats approving actions](https://webflow.workos.com/blog/curating-queues-beats-approving-actions) 的核心观点：

- 每个动作都弹确认，会把用户训练成"反射性点允许"，确认变成安全剧场；
- 可逆、影响面小的工作适合"批量候选 + 队列 + 人来挑"；不可逆、影响面大的动作才保留动作级审批；
- 审批应按"可逆性、影响范围、数据敏感度"分级，而不是二选一（全自动 / 每步都问）；
- 要关注拒绝率和队列堆积，而不是只看通过记录。

对我们的直接影响：

- **文件编辑**可逆、且用户已经在计划阶段确认过工作区，走 `acceptEdits` 自动放行；
- **Bash 命令**可能不可逆（删文件、git push、安装依赖），保留动作级审批；
- **只读命令**自动放行，避免审批疲劳；
- 审批不是弹一个框就完事：任务要进入"等待审批"，用户能在岛上看到是哪条命令、哪个任务，拒绝要有原因。

### 2. 并行与 worktree 是下一步的事（Cursor / Claude Code）

- [Cursor 3.2](https://cursor.com/fr/changelog/04-24-26) 用 `/multitask` 把任务拆成异步子任务，并让每个任务在独立 worktree/branch 上跑，完成后再拉回前台；
- Claude Code 也提供 [并行 agents](https://code.claude.com/docs/en/agents) 与 [worktree 会话](https://code.claude.com/docs/en/worktrees)；
- 这证明"主 Agent 拆解 + 子任务隔离"是成熟方向，但**隔离和合并**是独立的一步，不应该和审批/状态机混在一起做。

第一步仍然保持**同一工作区串行写入**；worktree、并发、合并留到下一步。

### 3. 人机回环要可中断、可恢复（LangChain HITL）

[Human-in-the-Loop](https://docs.langchain.com/oss/python/langchain/frontend/human-in-the-loop) 的通用模式是：中断 → 人处理 → 恢复，而不是让 agent 在后台无限等。我们的落地：

- worker 在 hook 上等待审批时，任务状态是 `waiting_approval`，不是假装"执行中"；
- 小岛审批卡允许/拒绝后，worker 继续或失败；
- 5 分钟无人处理按拒绝处理，任务失败，可重试；
- 退出应用时杀掉进程树，重启把中断任务标失败，不恢复成"还在跑"。

## 第一步的决策

| 决策 | 选择 | 理由 |
| --- | --- | --- |
| 文件编辑 | `acceptEdits` 自动放行 | 可逆、计划阶段已确认工作区 |
| 只读 Bash | 自动放行（含 `2>/dev/null;` 只读链） | 避免审批疲劳 |
| 危险/复合 Bash | 进入 `waiting_approval`，岛上允许/拒绝 | 不可逆动作保留动作级审批 |
| 审批超时 | 5 分钟，按拒绝处理 | 不让 worker 无限挂起 |
| worker 完成判定 | 强制结构化 `status: completed/blocked/failed` | 退出码 0 不等于完成 |
| 失败恢复 | "重试未完成任务"，已成功的不重跑 | 降低重试成本 |
| 主岛入口 | 展开态一条团队条，点击打开总览团队页 | 不破坏"两秒一眼"的紧凑布局 |
| 并行/预算/Codex | 下一步 | 先把状态、审批、恢复做对 |

## 验收口径

1. 危险命令会让任务进入"等待审批"，小岛出现审批卡，任务条显示命令。
2. 允许 → worker 继续 → 完成 → 主 Agent 汇总；拒绝 → worker 收到拒绝 → 任务失败 → 重试可用。
3. worker 说 `blocked/failed` 或没有结构化结果时，任务不能记成功。
4. 重试只重跑未完成任务，成功后能拿到汇总。
5. 主岛团队条显示进度/待审批，点击能打开总览团队页。
6. 重启/退出不会留下假装运行中的任务或孤儿进程。

## 第二步：预算（同日）

第一步跑完后先补预算，而不是马上上并发：

- Claude 的 `--max-budget-usd` 是**近似**上限，实测预算 $0.05 时仍花了 $0.093 才停；所以界面显示"真实累计成本"，不把上限画成精确线。
- 规划、worker、汇总三次调用都计入同一个 `spent_usd`；超预算后停止派发未开始的任务，重试也受剩余预算约束。
- 不做自动充值、不自动降级模型：预算不够就明确失败，让用户提高预算或新建一轮。

## 第三步：worktree 隔离（同日）

预算之后做 worktree，而不是先做并行：

- 每个任务从确认时的 HEAD 创建独立 `agent-island/<run>/<task>` 分支和 worktree；worker 只改 worktree，主工作区不动。
- 任务结束把 worktree 里的改动提交到任务分支，团队页按任务查看 diff，手动「合并」或「丢弃」。
- 合并要求主工作区干净；冲突时 `git merge --abort`，保留分支/worktree 供人工处理。
- 非 git 工作区直接拒绝；主工作区有未提交改动时启动会警告（任务看不到这些改动），合并也会被拒绝。
- worktree 只改 cwd，`Write` 仍可能用绝对路径写主工作区；因此 Edit/Write 的 PreToolUse 额外校验路径必须落在任务 worktree 内，`..`/绝对路径逃逸直接拒绝。
- 任务依赖在第三步时被明确拒绝（worktree 同基）；第六步改为支持单依赖线性链，见下。
- **先保持串行**：第三步时并行独立任务、merge 顺序与冲突策略留到后续。

## 第四步：独立任务并行（同日）

worktree 之后才开并发：

- 只并行互不依赖的任务，默认并发 2，上限 4；当时依赖任务明确拒绝（第六步已改为线性链）。
- 每个 worker 仍有独立 worktree；并行只改变执行时间，不改变隔离边界。
- 预算按“剩余预算 / 并发槽位”给每个任务预留上限，避免并发任务各自拿全额上限导致总和超支。
- 合并仍逐个由用户确认；某个任务冲突只影响它自己，其他任务继续可合并。
- 先不做自动合并、不做按依赖链的并行；合并顺序仍由用户决定。

## 第五步：多轮重规划（同日）

- 上一轮结束且所有改动都已合并/丢弃后，允许「再来一轮」。
- 新 run 携带上一轮目标、主 Agent 汇总、每个任务的状态与结果；主 Agent 基于实际结果规划，不重复已合并的工作。
- 新 worktree 从**当前 HEAD** 创建，因此必须先把上一轮改动合并进主工作区；还有待合并/冲突时直接拒绝。
- 新计划仍要预览、确认；不做自动连续执行，也不自动合并。
- 这样多轮不会基于旧代码，也不会把“上一轮没验证”悄悄当成已验证。

## 第六步：线性依赖链（同日）

- 只支持**单依赖、线性链**：每个任务最多一个依赖，每个任务最多被一个后续任务依赖；分叉和多依赖直接拒绝，避免多父合并。
- 依赖任务从依赖分支的 tip 创建 worktree；它能看到前一个任务的改动，但仍是独立 worktree/分支。
- 调度只要求依赖成功；同链自然串行，独立根任务仍可并行。
- 合并必须先合并依赖；依赖还有 pending/conflict 时，后续任务不能合并。
- 丢弃依赖会级联丢弃整条后续链；有后续任务正在执行时拒绝丢弃，避免把运行中的 worktree 删掉。
- B 的 diff 以 A 分支 tip 为 base，只显示 B 自己的改动；A→B 合并顺序保持干净。
- 通用 DAG、分叉链、自动合并仍不做。

## 第七步：执行级验收（同日）

- 计划任务可带可选的验收命令；留空就不跑，不猜项目该跑什么。
- worker 结束后、任务标记成功前，在**同一个任务 worktree**里执行命令；记录退出码、耗时、stdout/stderr。
- 退出码 0 才算验收通过；失败/超时的任务标失败，改动和输出保留，但 `merge_state=none`，不允许合并。
- 超时 120 秒，输出上限 20k；验收进程注册到 active，取消运行会杀进程树。
- 验收命令来自用户确认的计划，拥有当前用户权限；这是执行级证据，不是沙箱，也不是另一个 LLM 的评审意见。
- 验收结果进入任务记录，后续合并 UI 和再来一轮的上下文都能看到。

## 参考

- [Curating queues beats approving actions — WorkOS](https://webflow.workos.com/blog/curating-queues-beats-approving-actions)
- [Cursor 3.2: parallel subagents, worktrees](https://cursor.com/fr/changelog/04-24-26)
- [Run agents in parallel — Claude Code](https://code.claude.com/docs/en/agents)
- [Run parallel sessions with worktrees — Claude Code](https://code.claude.com/docs/en/worktrees)
- [Human-in-the-Loop — LangChain](https://docs.langchain.com/oss/python/langchain/frontend/human-in-the-loop)
