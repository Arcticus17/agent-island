# 灵动岛三态：隐藏 → 悬停显示 → 点击完全展开

日期：2026-10-21
范围：`src/app/App.svelte`、`src/preview/app-preview.ts`、`tests/browser/island-readonly.test.ts`、`tests/visual/island.visual.test.ts`

## 需求

岛平时**隐藏**，不占屏幕；鼠标移上去**显示胶囊**；再**点击**才完全展开。

## 状态机

```
          pointerenter                click
  hidden ────────────────▶ peek ────────────────▶ expanded
    ▲                       │                        │
    │   pointerleave+480ms  │                        │ 点 ⌄ / Esc
    └───────────────────────┘                        ▼
                                                     peek（鼠标移开再隐藏）
```

- 新增 `peeked` 状态，与既有 `expanded` 组成三态；`data-presence="hidden|peek|expanded"` 便于测试与样式。
- 窗口高度跟着状态走：隐藏 `13px`、胶囊 `60px`、展开按内容（上限 620px / 终端放大 92%）。
- 隐藏态只渲染一条 104×9 的「缺口」热区（深色胶囊 + 状态色小圆点），鼠标扫过顶部热区即回到胶囊。
- 展开态**不会**因为鼠标移开而收起（避免打断阅读/终端输入）；收起靠点 ⌄ 或按 Esc。
- 收起 → 胶囊态，鼠标再移开 480ms 才回到隐藏。

## 关键实现

- 悬停监听挂在 `<svelte:body onpointerenter/onpointerleave>`：`pointerenter` 不冒泡，挂在 `window` 上收不到（实机验证过，窗口不会长大）。
- 进入时 60ms 防抖（避免鼠标扫过顶边误触发）；离开时 480ms 延迟隐藏。
- Esc 直接隐藏（终端放大态会先退出放大再隐藏）。
- 预览/测试用 `initiallyPeeked` 跳过隐藏态，保证既有视觉基线仍是胶囊态。

## 实机验证（DPI aware，窗口 420 宽）

| 动作 | 窗口尺寸 |
| --- | --- |
| 启动 | **420 × 13**（只剩热区） |
| 鼠标移到顶部热区 | 420 × 60（胶囊：像素头像 / Codex CLI / 已停止） |
| 点击胶囊 | 420 × 620（完整面板） |
| 移开鼠标 1.6s | 420 × 620（保持，不打断） |
| 点 ⌄ 收起 | 420 × 60 |
| 再移开鼠标 1.3s | **420 × 13**（回到隐藏） |

截图：`mac-island-analysis/presence-notch.png`（隐藏）、`presence-peek.png`（胶囊）、`presence-expanded.png`（展开）。

## 测试

`tests/browser/island-readonly.test.ts` 新增 2 条：

1. `hides by default, peeks on hover, then expands on click` —— 覆盖隐藏→悬停→点击→展开→移开不收起→收起→隐藏 全链路，并断言窗口高度 13/60。
2. `goes back to hidden when the pointer leaves the peek pill`

## 回归门禁

`check` 0/0 · `unit` 123 · `frontend` 26 · `browser` **88** · `visual` 14。

## 取舍

- 隐藏态窗口仍是 420 宽（只留 13px 高），热区好命中；代价是屏幕顶部中间 420×13 的不可点击条带，和任何"隐藏但仍可悬停"的实现一样无法避免（若要更小，需要额外做窗口横向重定位）。
- 展开态不自动收起是刻意选择：面板里有输入框和终端，鼠标移开就收会很烦。
