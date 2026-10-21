# 审批卡「记住这条」+ 额度预警

日期：2026-10-21
范围：`src/approvals/rules.ts`（新）、`src/usage/quota.ts`（新）、`src/components/approvals/ApprovalStack.svelte`、`src/app/App.svelte`、测试与视觉基线

## 一、审批卡上的「记住这条」

上一轮做完了允许清单，但入口只有抽屉里手写模式——太绕。现在审批卡上直接给一个按钮：

```
[拒绝]  [记住这条]                                    [允许]
```

- 模式由命令推导（`suggestRulePattern`）：取前两段 + `*`
  - `npm install left-pad` → `npm install *`
  - `git status` → `git status`（单段不加通配）
  - `rm -rf ./build` → `rm -rf *`（危险命令也只放宽到前缀，仍要人工点一次）
- 点击后写入规则文件，并推一条岛内提示「已加入允许清单：Bash · npm install *」
- 只加规则，不会顺便放行当前这条（当前这条仍要用户自己点允许/拒绝）

## 二、额度预警

用量百分比越过 **80%** 时提醒一次，同一个"额度窗口"不重复。窗口标识优先用 `usage.resets_at_secs`（额度重置时间），没有就按当天算。

```
Claude Code 额度已用 92%，1 小时后重置
```

- 数据来自快照里的 `usage.used_percent`，不额外读文件；
- `usage.stale` 为真时不提醒（避免拿过期数字吓人）；
- 上次提醒的标记存 localStorage（`agent-island-quota-warned`），形如 `<agentId>@<window>`；
- 阈值目前固定 80，做成设置项留给后续。

## 顺手修的一个坑：本地提示被"防伪造"规则拦掉

`pushNotification` 里有防伪造校验（hook 事件可以带任意 session，必须反查到真实 Agent 且 id 必须是 `claude`）。
额度预警和规则回执是**岛自己产生的**，不该走这条校验——第一版这么写导致单测里通知根本不出现。

拆成两条路径：

| 来源 | 走哪条 |
| --- | --- |
| hook 事件 | `pushNotification`（session 反查 + 防伪造） |
| 岛内自产（额度/规则回执） | `pushLocalNotice(agentName, sessionId, message)`（可信来源，直接入栈） |

## 测试

| 层 | 用例 |
| --- | --- |
| 单测（9 条） | 模式推导（两段/单段/多空格/空串）、额度窗口 key、同窗口只提醒一次、换窗口重提醒、低于阈值/数据过期/无用量不提醒、自定义阈值、文案（小时/分钟/无重置时间） |
| 浏览器（2 条） | 审批卡点「记住这条」→ 规则以 `Bash · npm install *` 写入；用量 92% → 出现「额度已用 92%」且重复快照不刷屏 |

## 门禁

`check` 0/0 · `unit` **147** · `browser` **104** · `visual` **21**（3 张 approval 基线因新按钮重生成）· Rust 160。

## 后续

- 「记住这条」目前直接落规则，没有再给"改成更宽/更窄"的确认弹窗；
- 额度阈值应做成设置项，并支持按 Agent 单独设；
- 真正的**花费/预算**（美元）还没做：需要按天累计 `usage.cost_usd` 的增量，比百分比复杂一档。
