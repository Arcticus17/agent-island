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

## 参考

- [Curating queues beats approving actions — WorkOS](https://webflow.workos.com/blog/curating-queues-beats-approving-actions)
- [Cursor 3.2: parallel subagents, worktrees](https://cursor.com/fr/changelog/04-24-26)
- [Run agents in parallel — Claude Code](https://code.claude.com/docs/en/agents)
- [Run parallel sessions with worktrees — Claude Code](https://code.claude.com/docs/en/worktrees)
- [Human-in-the-Loop — LangChain](https://docs.langchain.com/oss/python/langchain/frontend/human-in-the-loop)
