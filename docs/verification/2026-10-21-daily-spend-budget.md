# 今日花费 + 每日预算守卫

日期：2026-10-21
范围：`src/usage/spend.ts`（新）、`src/components/cards/UsageCard.svelte`、`src/components/controls/{ActionBar,SettingsPopover}.svelte`、`src/app/App.svelte`、测试

## 语义：只统计"岛在线期间"的增量

`usage.cost_usd` 是**当前额度窗口的累计花费**，里面有多少是"今天"的，从外部无从判断。
所以不猜，直接按增量算：

```
第一次看到 cost=2.50   → 只建立基线，今日 $0.00
再看到     cost=3.40   → +0.90，今日 $0.90
cost 变小（换会话/窗口重置）→ 基线拉回，不产生负数
跨天 → 归零，重新建立基线
```

界面上写明"岛在线期间累计"，不假装是账单。状态持久化在 localStorage（已被 `settings.json` 镜像，强杀不丢）。

## 预算

设置面板新增「每日预算」：不设 / $5 / $10 / $20 / $50。设为 N 后：

- 用量卡片出现一行 `今日 $0.90 / 预算 $10`（≥80% 变琥珀，>100% 变红加粗）；
- 首次越过 80% 时推一条岛内提示，每天最多一次（`agent-island-budget-warned` 记日期）；
- 预算为 0 时这一行不出现，零打扰。

## 调试记录：一个"改了没生效"的假象

新加的预算选择器怎么写都读不到，`innerHTML` 里也找不到「每日预算」，看起来像组件没编译进去。
排查过程：清 Vite 缓存 → 无效；把 `{#if onBudget}` 临时改成 `{#if true}` → 还是看不到（其实是被 800 字符截断骗了）；
最后定位到真正原因：**测试里 `new Event("change")` 没有 `bubbles: true`**。

Svelte 5 对 `change` 采用事件委托，不冒泡的事件到不了根监听器，`onBudget` 自然没被调用。
加上 `{ bubbles: true }` 后一切正常——和之前会话里 `picker.dispatchEvent(new Event("change", { bubbles: true }))` 的写法一致。

## 验证

| 层 | 用例 |
| --- | --- |
| 单测（6 条） | 只算增量、首次只建基线、变小不回退、多 Agent 分开记账、跨天归零、预算状态（off/ok/near/over）、`$10` 不显示成 `$10.0`、读回预算 |
| 浏览器（1 条） | 打开设置选 $10 → localStorage 落盘、用量卡片出现「今日 $0.00 / 预算 $10」 |

实机：设置面板里能看到「每日预算 / 不设预算」这一行（截图 `mac-island-analysis/budget-settings-2.png`）。

## 门禁

`check` 0/0 · `unit` **153** · `browser` **105** · `visual` 21 · Rust 160。

## 后续

- 只统计在线时段，要拿到"真实日花费"得从 Claude/Codex 的会话日志里按消息累计，成本高；
- 阈值（80%）与档位（$5/10/20/50）都写死了，可以做成可配置；
- 还没做"超预算后自动拦截"，目前只是提示。
