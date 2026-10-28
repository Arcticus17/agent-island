# 收件箱 + 审批 2.0（v1.10）

日期：2026-10-28
范围：`src-tauri/src/inbox_store.rs`（新）、`src-tauri/src/approval_review.rs`（新）、`src-tauri/src/hooks.rs`、`src-tauri/src/lib.rs`、`src-tauri/src/approval_rules.rs`、`src/bridge/*`、`src/app/App.svelte`、`src/components/inbox/InboxPanel.svelte`（新）、`src/components/approvals/ApprovalStack.svelte`、`src/components/island/CompactIsland.svelte`、测试与视觉基线。

## 一、为什么做这两个

1. **通知是瞬时的**：原来的通知卡只在内存里，收起/重启就丢；离开电脑回来不知道错过了什么。
2. **审批缺上下文**：只显示命令和 cwd，文件编辑不弹卡；用户看不到会改什么、风险多大，最后变成无脑点「允许」。
3. **审批疲劳是结构性的**：Anthropic 的 auto mode 数据里，用户最终会接受 93% 的请求；真正需要人的是高风险例外。

v1.10 的目标不是「更多提示」，而是：

- 把审批/通知/完成做成**可追踪、可稍后、可追溯**的收件箱；
- 把审批卡从「一条命令 + 允许/拒绝」升级为**风险 + 仓库/分支 + diff 预览 + 拒绝理由 + 限时/限范围授权**。

## 二、做了什么

### 1. 收件箱（本地持久）

| 能力 | 实现 |
| --- | --- |
| 持久化 | `~/.agent-island/inbox.json`，临时文件 + rename 原子写；容忍 BOM/坏文件；上限 300 条，已解决条目保留 7 天 |
| 事件来源 | Claude hooks：审批、`Stop`（完成）、`Notification`（通知）；后端在写事件时同步 upsert |
| 去重 | 审批按 approval id；通知按 session + 消息哈希；完成按 session，120 秒内合并 |
| 命令 | `list_inbox` / `resolve_inbox_item` / `snooze_inbox_item`；变更后 emit `inbox-changed` |
| 展开态 | 岛顶部「待你处理」区：类型图标、标题、摘要、Agent/会话、等待时长、稍后/知道了；审批条目「查看」跳转审批卡 |
| 收起/隐藏态 | 胶囊右上角数字徽标（待处理数）；完全隐藏时 `idle-notch` 的 `data-pending` 为 true；新条目到达时把岛从隐藏**peek** 成胶囊，避免「存了但看不见」 |
| 离开摘要 | 距上次处理超过 10 分钟且 ≥2 条未解决时，展开显示「你离开的 N 分钟」摘要卡，可收起 |
| 键盘 | 审批卡保留原有按钮；收件箱暂未加 J/K 快捷键（见「后续」） |

### 2. 审批 2.0

| 能力 | 实现 |
| --- | --- |
| 风险分级 | 本地规则（不调用模型）：`rm -rf`、`git push --force`、`--production`、`npm publish`、`curl \| sh` 等 → 高；安装依赖、git push、网络请求、迁移/CI/认证/会话/支付路径 → 中；只读命令 → 低 |
| 仓库/分支 | `git -C <cwd> rev-parse`，Windows 用 `CREATE_NO_WINDOW`，失败就留空 |
| diff 预览 | `PermissionRequest` 的 Edit/Write `tool_input`；行级 LCS（最多 200 行输入 / 80 行输出），统计 +/−，截断时明说 |
| 路径安全 | 词法归一化 `..`，防止 `../outside` 逃出项目；仓库外、`.env`/密钥、迁移、CI、lockfile、认证/会话/支付等单独标记 |
| 拒绝并说明 | 卡片可展开理由输入；拒绝理由经 `/decision` 回传 bridge，写进 `PermissionRequest.decision.message` / `PreToolUse.permissionDecisionReason` |
| 授权范围 | 仅本次 / 10 分钟 / 本会话；10 分钟和本会话是**内存 grant**，进程退出即失效；同工具 + 模式（Bash 用命令、Edit/Write 用文件路径）匹配 |
| 批量 | 多条审批时显示「N 条待审批」；只有 ≥2 条**显式低风险**才出现「批准 N 条低风险」，高风险永远逐条 |
| 过期 | 审批 120 秒后自动标记为 `ask`、解决收件箱项并 emit `hook-approval-expired`，前端移除卡片，避免展示已不可操作的旧审批 |
| 记住这条 | 用 `rule_key`（Bash 命令 / Edit 文件路径）生成模式；Edit 不会再把 `(+18 −4)` 带进规则 |

### 3. Hook 协议

- `merge_hooks_settings` 新增 `PermissionRequest`（matcher `Edit|Write|MultiEdit`），保留用户已有 hooks。
- 生成脚本改为 `include_str!("hook_bridge.mjs")`，单独文件便于测试；事件覆盖 `PreToolUse` / `PermissionRequest` / `Stop` / `Notification`。
- `PermissionRequest` 输出按 Claude Code SDK 的类型：
  ```json
  {"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"allow"}}}
  ```
  拒绝时带 `message`；超时/岛离线时**不输出**，回退 Claude 原生确认。
- `PreToolUse` 保留 `permissionDecision` / `permissionDecisionReason` 形状，拒绝理由同样回传。
- 旧 bridge / 简单外部客户端只发顶层 `command` 时，Rust 会把它折回 `tool_input.command`，风险分级不会退化成「未提供命令」。
- 启动时只对**已经装了 bridge** 的 settings 做增量刷新（写入新脚本 + 合并新事件），不会把用户手动删掉的 hooks 悄悄装回去。

### 4. 测试

| 层 | 结果 |
| --- | --- |
| Rust | **222 passed**（新增 inbox 7、approval_review 10、hooks 合并/临时授权等） |
| Node（`test:frontend`） | **29 passed**，含 3 条 bridge 协议集成测试（PreToolUse allow、PermissionRequest deny/allow 的 JSON 形状 + reason） |
| 浏览器 | **131 passed**，新增 8 条：收件箱渲染/解决、胶囊计数、离开摘要、风险+diff+范围、拒绝理由、只批量低风险、过期移除、长列表截断 |
| 视觉 | **24 passed**，审批卡 3 张基线因新布局更新（已人工查看实际图后更新） |
| `svelte-check` | 0 errors / 0 warnings |

### 5. 真机验证（CDP + 真实 hook HTTP 服务）

在 `npm run tauri dev` + WebView2 CDP 下：

| 步骤 | 观察 |
| --- | --- |
| POST `PermissionRequest`（Edit `src/auth/session.ts`） | 返回 `pending` id；岛展开；风险「中风险」；理由「触碰认证/登录逻辑；触碰会话逻辑」；diff `+1 −1`；「记住这条」建议 `src/auth/session.ts`（不带 `(+1 −1)`）；收件箱「需要审批 · Edit」计数 1 |
| 点「允许」 | 审批卡消失；收件箱项变 `resolved`；`GET /decision` 返回 `{"decision":"allow"}` |
| 勾「10 分钟」后允许 `npm install left-pad` | 第二次 POST `npm install right-pad` 直接返回 `{"decision":"allow"}`，不再弹卡 |
| 勾「10 分钟」后允许同一文件 Edit，再 POST 同文件 Edit | 第二次直接返回 `{"decision":"allow"}`（临时 grant 用文件路径匹配，不再带 diff 计数） |
| 拒绝 `npm run deploy -- --production` 并填理由 | `GET /decision` 返回 `{"decision":"deny","reason":"先在 staging 验证"}` |
| POST `Notification` | 收件箱写入；岛从隐藏 `peek` 成胶囊，徽标 `2`，状态「待处理」 |
| 重启前检查 | 收件箱 JSON 为 UTF-8，审批项 `resolved`；桥接测试证明脚本输出与 SDK 类型一致 |

> 测试产生的 `~/.agent-island/inbox.json` 已删除；未改动 `~/.claude/settings.json`（本次没有真正安装 hooks，只验证了服务端与 UI）。
>
> 实机截图：`mac-island-analysis/inbox-approval-2-live.png`（审批卡的风险 / diff / 范围 / 拒绝并说明）、`mac-island-analysis/inbox-live.png`（收件箱条目与胶囊「待处理」徽标）。

## 三、仍未验证 / 已知限制

1. **真实 Claude Code 触发 `PermissionRequest` 尚未跑通**：bridge 的 JSON 形状已对照 `@anthropic-ai/claude-agent-sdk` 的类型并用合成服务验证；但「Claude 在真实会话里因为 Edit 而询问，岛弹出卡片」这一步需要一次真实 Claude 会话。`PermissionRequest` matcher `Edit|Write|MultiEdit` 也需要在真实版本上确认。
2. **MultiEdit**：当前 `rule_key`/diff 主要按顶层 `file_path`；MultiEdit 的 `edits` 数组还没有逐个渲染。
3. **收件箱「全部已读」**：当前是并发逐条调用，没有失败汇总；单条失败会保持原状，但用户看不到哪条失败。
4. **snooze**：只写 `snooze_until_ms`，前端在下次 `list_inbox` 时把到期项恢复为 open；没有独立的定时唤醒。
5. **完成事件**：`Stop` 只表示本轮结束，不区分成功/失败；收件箱标题统一为「任务完成」，失败信息仍靠日志/状态卡。
6. **过期清理**：15 秒轮询；最坏情况下卡片会多留约 15 秒。`/decision` 在过期后返回 `ask`，bridge 已回退原生确认。
7. **审批卡 diff**：只对 Edit/Write 做预览；Bash 仍然只有命令、风险与 cwd/repo，不做「假 diff」。
8. **收件箱未做键盘导航**（J/K、A/D）；审批卡本身仍可用 Tab/Enter。

## 四、后续

- 真实 Claude 会话跑一次 `PermissionRequest`，把 matcher 与输出形状钉死；若 matcher 不支持 `|`，改成分别安装三个 matcher。
- 给收件箱加键盘导航与「全部已读」的失败汇总。
- MultiEdit 逐个 edit 预览与 `rule_key`。
- 把完成事件升级成「本轮完成 / 本轮失败」；失败时收件箱条目带「让 Agent 修」。
- 为审批过期/自动授权补审计条目（当前临时授权只在内存，不写 `approval-audit.jsonl`）。
