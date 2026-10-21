# 审批不被埋没：「待确认」信号 + 不再自动收起

日期：2026-10-21
范围：`src/app/App.svelte`、`src/components/island/CompactIsland.svelte`、`tests/browser/island-actions.test.ts`、视觉基线

## 为什么先放弃「系统 Toast」

原计划用 `tauri-plugin-notification` 发 Windows Toast。`cargo add` 之后发现它把整条依赖链抬了一档：

```
tauri  2.x      → 2.12.4
wry    0.55.1   → 0.57.0
+ zbus/zbus_macros/zvariant …（700+ 行 Cargo.lock 变动）
```

在一个功能回合里顺手升级 WebView 运行时（wry）风险太大，直接 `git checkout` 回退，改为**不引入依赖的注意力机制**。Toast 留作后续单独评估（要动就单独一个升级回合，带完整回归）。

## 三条信号（收起状态下也不会漏）

| 位置 | 表现 |
| --- | --- |
| 胶囊态 | 徽标变琥珀色 `!`，状态字变「待确认」，整条胶囊加一圈琥珀描边 |
| 隐藏态 | 顶部热区变宽、变琥珀、脉冲动画（`prefers-reduced-motion` 下自动关掉） |
| 自动收起 | 有待确认审批时**不再自动隐藏**，收起也保持胶囊可见 |

改动的核心是一个派生状态：

```ts
const hasPendingApprovals = $derived(approvals.length > 0);
```

它同时喂给 `scheduleIdleHide()`（挡住自动隐藏）、隐藏热区的 `data-pending`、以及 `CompactIsland` 的 `pendingApprovals`。

## 验证

**浏览器（新增 1 条）**

```
审批到达 → 自动展开，出现「允许」按钮
点 ⌄ 收起 → 胶囊态
派发 pointerleave → 等 700ms → 仍是胶囊（没有自动隐藏）
Esc → 隐藏 → 热区 data-pending="true"
```

**实机**：向 hook 服务 `POST /event`（`PreToolUse` + `git push --force`）真的造了一条待审批：

```
POST -> 200 {"decision":"pending","id":"a1a15e335f45"}
展开态 560×560：审批卡显示 Bash / git push --force / 拒绝 / 允许
点 ⌄ → 560×60：徽标变「! 待确认」（琥珀）
鼠标移开 1.2s → 仍是 560×60（没有被自动收起）
点「拒绝」→ 卡片消失
```

截图：`mac-island-analysis/approval-live.png`、`approval-pill.png`。

## 顺带修掉的测试抖动

`rejects ambiguous or spoofed notification targets …` 原先用 `setTimeout(20)` 猜"处理完了"，机器负载高时偶发失败。
改成**等一次真实轮询落地**（`harness.snapshotCalls()` 增长）再断言，语义明确且不依赖时序。

## 门禁

`check` 0/0 · `unit` 138 · `browser` **101** · `visual` **21**（3 张 approval 基线因新徽标重生成）· Rust 154。
