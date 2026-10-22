# 两处契约加固：命令面板处理表 + Agent 条键盘导航

日期：2026-10-22
范围：`src/palette/commands.ts`、`src/app/App.svelte`、`tests/palette-contract.test.ts`（新）、`tests/browser/island-readonly.test.ts`

## 一、命令面板：把"漏实现"变成编译错误

第七轮踩过的坑——`PALETTE_ACTIONS` 里有「固定当前 Agent」，`switch` 里却没有对应分支，点了没反应，测试也照样绿。

这次从两头堵：

1. **类型层面**：动作 id 收成联合类型 `PaletteActionId`，App 用
   ```ts
   const paletteHandlers: Record<PaletteActionId, () => void> = { … }
   ```
   少一个 key 直接编译不过，`switch` 的静默遗漏从根上消失。
2. **测试层面**：`tests/palette-contract.test.ts` 读源码，抓出 `PALETTE_ACTIONS` 里声明的 id
   和 `paletteHandlers` 里实现的 id，断言两边**完全一致**（顺带断言 id 不重复、顺序稳定）。

> 也试过写端到端的"依次执行每条命令"用例，但它在隔离运行时很不稳（要反复开关面板、依赖前序状态），
> 换成源码契约测试后确定性强得多——这类"结构约束"用读源码的测试更合适。

## 二、Agent 条键盘导航

之前只能用鼠标点顶部的 Agent 条。现在方向键可用：

| 按键 | 行为 |
| --- | --- |
| `→` | 下一个 Agent（末尾循环回第一个） |
| `←` | 上一个 Agent（开头循环到最后一个） |
| `Home` / `End` | 跳到第一个 / 最后一个 |

实现要点：处理器挂在**按钮**上（不是 `<nav>` 上），否则 Svelte 会报
`a11y_no_noninteractive_element_interactions`；切换后把焦点移到新选中的按钮，视线和操作保持一致。

## 验证

| 层 | 用例 |
| --- | --- |
| 单测（2 条） | 声明与实现的动作 id 集合相等、id 列表稳定 |
| 浏览器（1 条） | 聚焦第一个 Agent → `→` 选中 beta 且焦点跟随 → `←` 回到 alpha → `End` 到末尾 → `Home` 回开头 |

## 门禁

`check` 0/0 · `unit` **172**（+2）· `browser` **111**（+1）· `visual` 24 · Rust 167。

## 后续

- Agent 条还没做 roving tabindex（现在每个按钮都可通过 Tab 到达，切换后焦点由代码移动）；
- 会话列表（历史会话下拉）也是原生控件，暂不需要自定义键盘导航。
