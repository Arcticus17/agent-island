# Codex 团队 worker 验收

日期：2026-10-31

## 结论

Codex CLI 已接入团队任务，可作为**次级 worker**，也可作为**主 Agent** 规划；默认主 Agent 仍是 Claude。

- 环境：Windows 11 x64，Codex CLI 0.165.0（npm 安装，实际调用 `@openai/codex` 自带原生 `codex.exe`）
- 调用方式：`codex exec --json --output-schema <schema> --output-last-message <last> --sandbox workspace-write --skip-git-repo-check --ephemeral -C <worktree> <prompt>`
- worker 必须返回符合团队 schema 的 JSON：`status`（completed/blocked/failed）、`summary`、`files_changed`、`verification`
- 每个任务仍在独立 git worktree/分支执行；Codex 只能写 workspace-write 允许的工作区
- 完成后走原有流程：主 Agent 汇总、手动合并/丢弃、验收命令（若填写）

## 实机链路

1. 临时 git 仓库只有一次提交和 `seed.txt`
2. 团队页生成计划，规划器自动把单任务分配给 `codex`
3. Codex worker 执行 2m30s，退出码 0，状态 `completed`，只新增 `CODEX_TEAM_OK.txt`
4. 团队页显示“完成/待合并”和 diff
5. 点「合并」→「确认合并到主工作区」后，主工作区出现 `CODEX_TEAM_OK.txt`，内容为 `CODEX_TEAM_OK`
6. 任务记录 `costUsd=null`；Codex 只返回 token 用量，不返回 USD

另外单独跑过 `runner::run_codex` 的 live test（`#[ignore]`）：返回 schema JSON、文件写入正确、进程退出码 0。

## 过程中确认的边界

- **Shell 工作目录**：Codex 的 shell 工具在 Windows 上可能从 `C:\` 启动；文件编辑工具仍以 `-C` 指定的 worktree 为根。worker prompt 已明确禁止用 shell 重定向写文件，所有修改必须落在工作区内。
- **无岛内审批**：Codex worker 没有 Claude hooks 审批链；靠 `workspace-write` 沙箱限制写入。需要审批的命令应在计划/验收阶段处理。
- **预算**：Codex 不返回 USD 成本，团队预算只累计 Claude 规划/Claude worker 的成本；不能把 Codex 的 token 当 USD 记账。
- **验收命令**：在 Windows 上通过 `cmd /D /S /C` 执行。最初 `node -e "..."` 在 `cmd` 层被拆坏；已改为 `raw_arg` 保留外层引号，并新增 `passes_a_command_with_nested_quotes_on_windows` 回归。本次 Codex worker 的空验收链路已经验证；带引号的验收命令还需要在真实任务里再跑一次。
- **主 Agent**：默认 Claude；Codex 也可规划，已通过规划/汇总/合并实机链路。
- **未测**：多 Codex 任务并发、Codex + Claude 混合依赖链、Codex 超时/强杀、Codex 无登录凭据的失败提示、真实业务仓库。

## 主 Agent 角色配置

同一轮补了“主 Agent 可选 Claude / Codex”：

- 团队页新增主 Agent 下拉；`team_start_plan` 接收 `main_agent`，默认 Claude
- Codex 作为主 Agent 时使用 `codex exec --output-schema <plan schema>` 规划，规划结果仍走同一套校验
- 汇总阶段也按本轮主 Agent 选择 Claude 或 Codex
- 续轮沿用上一轮主 Agent

实机通过（2026-10-31 16:01）：

1. 团队页选 Codex 主 Agent
2. Codex 规划出单任务（负责人 Claude）
3. Claude worker 在 worktree 创建 `MAIN_CODEX_OK.txt`
4. Codex 生成主 Agent 汇总
5. 手动合并后主工作区出现文件

Codex planner 的 live test 也通过。此前失败的关键原因是 **Codex 对 JSON Schema 的要求更严**：`tasks.items` 的 `verify_command` 必须是 `required` 的一项，否则返回 `Invalid schema for response_format`。已在 `plan.rs` 补齐，并更新 schema 单测。

Codex worker 和 Codex 主 Agent 都依赖 Codex 账号路由；本次复测期间遇到过 `workspace routing discovery failed`，属于 Codex 服务/网络侧问题，恢复后 planner/worker live test 均通过。

## 相关代码

- `src-tauri/src/orchestrator/capability.rs`：Codex 原生 exe 解析、auth 检测、能力卡
- `src-tauri/src/orchestrator/runner.rs`：`run_codex`、JSONL 日志、schema 结果解析、超时/进程树清理
- `src-tauri/src/orchestrator/mod.rs`：按 assignee 选择 Claude/Codex runner、Codex schema/last 文件、worker prompt 约束

## 回归

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib
cargo test --manifest-path src-tauri/Cargo.toml --lib orchestrator::runner::tests::codex_worker_live_returns_schema_json -- --ignored --nocapture
```

live test 会真实调用 Codex，消耗 token，默认不跑。
