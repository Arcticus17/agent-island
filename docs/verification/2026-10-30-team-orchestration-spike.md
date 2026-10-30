# 团队编排验证骨架：spike 记录（2026-10-30）

状态：验证骨架已完成（完整 MVP 未开始）。本文只记录"动手前必须先证明的硬点"的真实结果，不把未验证的东西写成已完成。

## 结论摘要

| 硬点 | 结果 | 证据 |
| --- | --- | --- |
| Claude 无头规划 + 结构化输出 | ✅ 通过 | `claude -p --output-format json --json-schema …` 返回 `structured_output`，字段与 schema 一致 |
| `claude -p` 下 PreToolUse 审批阻塞 | ✅ 通过 | hook 事件到达本地服务，等待 2.5s 决策；allow 执行、deny 返回 `is_error` 且命令未执行 |
| worker 用 `acceptEdits` 真写文件 | ✅ 通过 | `hello.txt` 内容为 `SPIKE_WRITE\n`，Write 工具 `permission_decision=accept, source=config` |
| Claude 子进程可归属 | ✅ 通过 | `claude.exe` 直接由启动者派生，父 pid 明确；无额外 node 子进程 |
| Codex 无头执行 | ❌ 当前不可用 | 两次探测 `workspace routing discovery timed out`，120s / 180s 均未完成 |
| OpenCode / Hermes | ❌ 未安装 | PATH 中不存在，无法验收 |

## 1. Claude 规划（结构化输出）

真实调用：

```text
claude -p "…" --output-format json --max-turns 1 --permission-mode plan \
  --json-schema "<schema>" --session-id <uuid>
```

结果（节选）：

```json
{
  "is_error": false,
  "session_id": "…",
  "total_cost_usd": 0.08083,
  "result": "{\"tasks\":[{\"title\":\"PONG\",\"agent\":\"claude\"}]}",
  "structured_output": { "tasks": [ { "title": "PONG", "agent": "claude" } ] }
}
```

要点：

- `--json-schema` 会产出 `structured_output`，不需要从正文里正则抠 JSON。
- 一次调用约 15s、1.37 万输入 tokens，实测 $0.08；5 个 worker 加汇总的成本必须设上限。
- 本机 Claude Code 走兼容端点，凭据来自 `~/.claude/settings.json` 的 `env`；不注入这些变量时 `claude -p` 返回 `is_error: true`、`result: "Not logged in · Please run /login"`。应用进程必须自己做凭据解析，不能假设 CLI 已登录。

## 2. print 模式审批（最关键的未知）

用临时 `--settings` 注入 PreToolUse hook（`command` 为字符串），本地 18999 服务模拟"人 2.5s 后点按钮"：

- allow：hook 先收到 `PreToolUse`，等待决策，随后 Bash 执行，`tool_result` 为 `SPIKE_OK`。
- deny：hook 收到决策后命令**没有执行**，`tool_result` 为：

```json
{
  "is_error": true,
  "content": "PreToolUse:Bash hook error: spike",
  "tool_result_meta": [{ "permission_decision": { "decision": "reject", "source": "hook" } }]
}
```

同时确认：hook 的 `session_id` 就是我们传入的 `--session-id`，因此可以把审批精确归属到某个团队任务。

## 3. 发现的既有 bug：hook `command` 写成数组会被静默忽略

Claude Code 2.1.312 对下面两种写法的行为不同：

```json
{ "command": ["node", "C:\\…\\hook-bridge.mjs"] }   // 不执行 hook
{ "command": "node \"C:\\…\\hook-bridge.mjs\"" }     // 正常执行
```

当前 `merge_hooks_settings` 写的是数组，所以本机安装的 PreToolUse / PermissionRequest / Stop / Notification hook 很可能一直没有生效。这个 bug 会直接让团队任务的审批链路失效，必须在骨架里一起修：

- 写入时把 `command` 变成字符串（路径加引号）；
- 卸载/覆盖时同时识别字符串与旧数组两种形态，避免留下孤儿 hook；
- 补 Rust 单元测试钉住两种形态。

## 4. worker 写文件

```text
claude -p "Create hello.txt with SPIKE_WRITE" \
  --output-format stream-json --verbose --permission-mode acceptEdits
```

结果：文件真实创建，内容 `SPIKE_WRITE\n`；Write 工具结果带 `permission_decision=accept, source=config`；进程退出码 0，result 事件包含 `session_id`、`total_cost_usd`、`terminal_reason`。

## 5. 进程归属

运行期间进程表：

```text
node.exe (spike 脚本, pid 52924)
└─ claude.exe (pid 56060, --session-id 5c8bf491…)
```

`claude.exe` 由启动者直接派生，现有 `process_scan` 会按 `claude` 关键字匹配到它。因此团队任务必须维护 `managed_pids`（根 pid + 后代）并在主岛快照里排除，否则岛上会多出一个"野生 Claude"。

## 6. Codex 现状

`codex exec --json` 能启动并输出 `thread.started` / `turn.started`，但随后反复：

```text
{"type":"error","message":"Reconnecting... 2/5 (workspace routing discovery timed out)"}
```

120s 和 180s 两次都未完成。结论：适配层可以写，但不能作为本机验收对象；界面上必须显示"后端不可达/超时"，而不是无限转圈。

## 7. 对骨架的直接影响

1. 主 Agent 用 Claude，`--json-schema` 出计划，计划先预览再执行。
2. worker 用 Claude，`--output-format stream-json`、`--permission-mode acceptEdits`，需要审批时走 `--settings` 注入的字符串 hook。
3. 每个任务生成独立 `--session-id`，作为 hooks 归属与后续 resume 的键。
4. 进程按根 pid 过滤，团队任务只在团队视图出现。
5. Codex / OpenCode / Hermes 先标记能力不可用，不阻塞 Claude 闭环。

## 8. 尚未验证（进入 MVP 前补）

- 多个 worker 并发时审批卡是否会串；同一工作区串行写入的调度是否足够。
- 长任务（>10 分钟）、取消时的进程树清理、stdout 大量输出时的限流。
- `--resume` 做第二轮汇总/重规划的真实行为。
- 岛退出/崩溃后重启，运行中任务的中断标记与残留进程处理。

## 9. 真实应用端到端（2026-10-30）

通过 WebView2 远程调试（`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`）驱动总览窗口里的团队面板，跑完整流程：

```text
总览 → 团队 → 目标/工作区 → 生成计划 → 计划预览 → 确认执行 → 任务完成 → 主 Agent 汇总
```

第一次运行暴露了一个真实缺陷：team worker 的 `Write` 被 `PermissionRequest` hook 拦住，脚本没有人去点，任务在"执行中"停了 **2 分 09 秒**（正好是 hook 的 120 秒等待 + 进程收尾）。日志：

```text
Write  .../E2E_OK.txt
↳ File created successfully ...
Bash  od -c ".../E2E_OK.txt"
✗ Agent Island 未响应，回退原生确认
```

修复：

- 团队 worker 的 `--settings` 只挂 `PreToolUse`（Bash），不再挂 `PermissionRequest`；文件编辑由 `acceptEdits` 自动放行。
- `managed_sessions` 把 team worker 的 `--session-id` 登记下来；hook 服务识别到团队会话后：
  - 只读命令 → 立即 allow；
  - 危险命令 → 立即 deny，并把原因写回任务（不再等 120 秒人工审批）。
- `od` / `Get-ChildItem` / `Test-Path` 等只读命令加入安全前缀。

第二次运行：**17 秒完成**，任务花费 $0.131，文件真实存在且内容为 `E2E_OK`；主 Agent 汇总明确写了"字节级核验没有完成"，没有把推断写成已验证。控制台截图见验证时的临时文件（未入库）。

仍然存在的限制：带 `>` / `;` / `|` 的复合命令会整体被拒绝（即使其中包含只读命令）；worker 需要改用 Read / Glob 或单条只读命令。这是骨架阶段的有意取舍——**快速拒绝**优于**静默等待**。

## 10. 团队任务的权限策略（骨架）

| 场景 | 行为 |
| --- | --- |
| 文件编辑（Edit/Write/MultiEdit） | `acceptEdits` 自动放行；worker 只能改工作区内的文件 |
| 只读 Bash（ls / cat / wc / od / Get-ChildItem…） | hook 立即 allow |
| 危险或复合 Bash（git push、rm、含 `>`/`;`/`|` 的链） | hook 立即 deny，原因回传 worker；不弹 120 秒审批 |
| 用户自己的会话 | 保持原来的审批行为（approval card + 120 秒回退） |

## 11. 骨架已实现的边界

- 入口：总览窗口顶部「团队」；主岛还没有团队状态条。
- 主 Agent：Claude Code；规划走 `--json-schema`，汇总走普通 JSON。
- 调度：同一工作区串行、按依赖 DAG 取第一个就绪任务；最多 4 个任务；单任务 10 分钟超时。
- 持久化：`~/.agent-island/team-runs.json`；重启时把中断的 run 标记为失败，不假装还在跑。
- 进程归属：团队 worker 的根 pid 登记在 `TeamState.active`，主岛快照排除这些 pid；应用正常退出时 `orchestrator::shutdown` 杀掉进程树。
- 能力预检：Claude 可用；Codex 显示"已安装但未通过无头可用性验证"；OpenCode/Hermes 未接入。凭据从 `~/.claude/settings.json` / `setting.json` 的 env 解析，缺失时明确报错。

## 12. 进入 MVP 前仍要做

1. 主岛团队条 + 任务树/审批入口；当前只有总览窗口。
2. 并行与成本上限：并发数、`--max-budget-usd`、运行预算、超时后的清理。
3. 多轮重规划（主 Agent 看结果再补计划）与失败重试/改派。
4. Codex 适配器（等后端可达后再验收）、自定义 Agent 的 dispatch 模板。
5. 审批归属到 task；当前团队任务不进入人工审批，直接按策略放行/拒绝。
6. 视觉回归基线、混合 DPI、多显示器与安装包验收。

