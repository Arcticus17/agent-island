# 图标风格切到像素吉祥物与视觉基线刷新

日期：2026-10-21
范围：`src/components/brand/*`、`src/components/island/CompactIsland.svelte`、`src/components/controls/{ActionBar,SettingsPopover}.svelte`、`src/app/App.svelte`、`tests/visual/__screenshots__/island.visual.test.ts/*.png`

## 背景

参考一条 Mac 灵动岛演示片（拆解见 `mac-island-analysis/Mac灵动岛视频拆解报告.md`），把其中"像素吉祥物 + 状态色 + 数值徽标"的紧凑态语言落到本项目，并按产品决定把**像素风格设为默认**，圆点风格保留为可选项。

## 变化

- 新增 `PixelAgent`：8×8 单色像素精灵（claude / codex / opencode / hermes / generic），用 `shape-rendering: crispEdges` 画 rect 网格，颜色取 `--island-status-current`，因此一只精灵自动跟四种状态色。
- 紧凑胶囊新增 `iconStyle: "dot" | "pixel"`，**默认 `"pixel"`**；像素模式下右翼多一个数值徽标：优先 `usage.used_percent`（额度百分比），无用量数据时回退到本会话事件条数；`waiting` 状态下徽标替换为琥珀色 `?` 并带呼吸动画，头像轻微浮动。
- 设置面板新增"图标"选择（像素吉祥物 / 圆点），写入 `localStorage["agent-island-icon-style"]`，与外观（主题）同一套模式。
- 圆点模式完全保留：`iconStyle: "dot"` 时 DOM 与样式与改动前一致。

## 为什么基线必须刷新

像素模式是**默认值**，14 项视觉基线的紧凑条都会变（头像由 12px 圆点换成 20px 像素螃蟹，右翼多一个数值徽标）。这不是抗锯齿差异，是设计变更。

判断依据：改动前先以 `iconStyle` 默认 `"dot"` 跑过一次 `npm run test:visual`，**14/14 通过**，证明新增分支没有污染旧样式；把默认值切到 `"pixel"` 后 14/14 失败，逐张查看差异图确认变化只落在紧凑条的头像与徽标位置，其余（卡片布局、日志、按钮）逐像素一致。

## 处理

1. `npm run test:visual` → 确认 14/14 失败且差异仅在预期区域。
2. `npm run test:visual:update` 刷新 14 张基线。
3. `npm run test:visual` 复跑 → **14/14 通过**。

## 结果

| 检查 | 结果 |
| --- | --- |
| `npm run check` | 0 errors / 0 warnings |
| `npm run test:unit` | 123 passed |
| `npm run test:frontend` | 26 passed |
| `npm run test:browser` | 79 passed |
| `npm run test:visual` | 14 passed（基线已刷新） |

Rust 侧未改动，`cargo test` 无需重跑。

## 回退

- 只回退视觉风格：设置面板把"图标"切回圆点，或 `localStorage.setItem("agent-island-icon-style", "dot")`。
- 整体回退：`git checkout -- src tests`（未提交 commit）。
