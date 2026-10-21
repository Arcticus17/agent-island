# 灵动岛 ↔ Agent 双向交互：发指令 + 审批同步弹出

日期：2026-10-21
范围：`src/components/controls/PromptComposer.svelte`（新）、`src/app/App.svelte`、`src/components/approvals/ApprovalStack.svelte`、`src/components/events/types.ts`、`src-tauri/src/hooks.rs`、`src-tauri/src/tests.rs`、`tests/browser/island-actions.test.ts`

## 目标

1. 在**岛上**直接给当前 Agent 发指令（不用切到总览窗口）。
2. Agent 的请求（Claude Code 的 `PreToolUse` 审批、`Notification`）要**同步弹到岛上**。

## 一、岛上发指令

新增 `PromptComposer`，放在展开面板顶部（诊断行与卡片布局之间）：

- 输入框 + 发送；发送走既有 `send_to_session`（overview 窗口用的同一条命令），拿到 `taskId`；
- 每 800ms 轮询 `get_send_output`，把 stdout/stderr 逐行流到岛内一个 112px 高的只读区，结束显示"已结束"，可"清空"；
- 无当前会话 / 正在看历史记录时输入框禁用并说明原因；`inspectingHistory` 时不允许向历史会话发指令；
- 发送成功后清空输入框，避免重复发；失败按 `CommandFailure` 显示 code + message。

## 二、Agent 请求同步弹出（端到端已验证）

链路本来就是通的：`claude` → `.claude/settings.json` 的 `PreToolUse( Bash )` → `node ~/.agent-island/hook-bridge.mjs` → `POST http://127.0.0.1:8799/event` → 岛弹审批卡 → 用户点「允许/拒绝」→ `respond_hook_approval` 写决定 → bridge 轮询 `/decision` 拿到 `allow/deny` → Claude 继续/放弃。

用真实 HTTP 走了一遍（模拟 hook 客户端，与 bridge 协议一致）：

```
POST /event  {"name":"PreToolUse","session":"e2e-approval-session","tool":"Bash",
              "command":"rm -rf /tmp/agent-island-e2e","cwd":"D:/demo"}
← {"decision":"pending","id":"a1a15a6225b1"}     # 岛内立刻弹出审批卡（自动展开）
GET  /decision?id=a1a15a6225b1
← {"decision":"allow"}                            # 在岛上点「允许」之后
```

### 过程中修掉的真 bug

`read_http_request` 解析请求头时是**大小写敏感**的：

```rust
l.strip_prefix("content-length:")          // 只认小写
l.strip_prefix("x-agent-island-token:")
```

Node 的 undici 恰好发小写，所以 Claude Code 的 bridge 一直能用；但 curl / 其它 HTTP 客户端发的是规范写法 `Content-Length:`，于是 body 被当成空，`name` 解析不出来，审批事件被静默丢弃（返回 `{}`，卡片永不出现）。已改成大小写不敏感解析，并补了单测 `hook_http_request_reads_canonical_case_headers`。

另外审批卡现在会显示**是哪个 Agent 在请求**（按 `approval.session` 反查快照里的 `active_session.id`），卡片头部从 `Bash` 变成 `Claude Code · Bash`。

## 验证

| 检查 | 结果 |
| --- | --- |
| `cargo test --no-default-features --lib` | 141 passed（新增 HTTP 头解析 1 条） |
| `npm run check` | 0 errors / 0 warnings |
| `npm run test:unit` | 123 passed |
| `npm run test:frontend` | 26 passed |
| `npm run test:browser` | 81 passed（新增 2 条：发指令流式回执、历史会话禁用发送） |
| `npm run test:visual` | 14 passed（展开态基线含新输入框，已刷新） |
| 实机 | 审批卡弹出→点允许→`/decision` 返回 `allow`；岛上输入框按会话状态正确禁用 |

## 边界

- **真实 CLI 未登录**：本机 `claude -p` 返回 `Not logged in · Please run /login`，所以"真 Agent 触发审批"这一步没法在本机跑通；协议层（hook bridge、事件服务器、决策回传）已用等价 HTTP 客户端完整验证。登录后无需改代码即可生效。
- 发指令要求"可靠匹配的当前会话"（这是 v1.9 会话一致性规则），没有确认会话时输入框是禁用的，防止把指令发进别的会话。
- 审批并发上限沿用后端（最多 5 条待处理），超出会淘汰最旧的。
