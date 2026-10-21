# 固定 Agent

日期：2026-10-21
范围：`src/models/pinned-agent.ts`（新）、`src/app/App.svelte`、`src/components/controls/{ActionBar,SettingsPopover}.svelte`、`src/palette/commands.ts`、测试

## 为什么

自动选择按"谁最需要看"排序（工作中 > 等待确认 > …），巡视时很合适；但一旦决定盯住某个会话，
每隔几秒被换走就很难受——尤其是你正在它的终端里敲东西。

## 行为

- 设置里「固定 Agent / 固定当前」把**当前选中的** Agent 钉住，按钮随后显示「已固定 <名字>」；
- 钉住期间 `selectDefaultAgent` 直接停在它上面，不再跑自动排序；
- 固定的 Agent 从快照里消失（进程退出且被过滤）时**自动解除固定**，回退到自动选择，不会卡死在一个空项上；
- 选择持久化在 localStorage（`agent-island-pinned-agent`），重启仍生效；
- 命令面板里也有「固定 / 取消固定当前 Agent」，不想开设置时用 `Ctrl+K` 两步搞定。

## 一个刻意的取舍

固定只影响**自动切换**，不影响审批通知的自动展开——有 Agent 在等你确认时，岛仍然会弹出来。
"我在盯 A" 和 "B 在等我点允许" 是两件事，前者不该把后者静音。

## 验证

**实机**：设置 → 固定 Agent → 按钮变「已固定 Codex CLI」；再点一次取消。
截图：`mac-island-analysis/pin-settings-3.png`（固定前）、`pin-on.png`（已固定）。

| 层 | 用例 |
| --- | --- |
| 单测（3 条） | 命中固定项、固定项消失时 `stalePin=true`、读取持久化时忽略空白 |
| 浏览器（1 条） | 切到 beta 并固定 → 下一轮快照里 alpha 变成"工作中" → 选中仍是 beta，且按钮显示「已固定」 |

## 门禁

`check` 0/0 · `unit` **160** · `browser` **108** · `visual` 21 · Rust 160。

## 后续

- 固定项在胶囊上没有额外视觉标记（只有设置里的按钮文案），下一步可以在名字旁加个📌；
- 目前只能固定一个 Agent，多个固定需要重新设计优先级。
