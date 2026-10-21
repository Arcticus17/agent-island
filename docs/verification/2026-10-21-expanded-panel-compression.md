# 展开面板变长治理：收纳次要视图与按钮

日期：2026-10-21
范围：`src/app/App.svelte`、`src/components/layout/LayoutEditor.svelte`、`src/components/controls/{ActionBar,SettingsPopover}.svelte`、`tests/browser/island-readonly.test.ts`、视觉基线

## 背景

实机展开态高度 **993px**（420 宽、monitoring 预设、5 张卡全展开 + 诊断面板常驻 + 动作条两行）。使用者反馈"展开太长了，希望有些视图和按钮隐藏起来，用的时候再弹出"。

## 改法（渐进披露）

| 改动 | 效果 |
| --- | --- |
| 诊断信息收进 `<details>`，默认收起，摘要行常显"N 项提示" | 常驻面板（约 200px）变成一行（34px），点开才渲染展开内容 |
| 「窗口宽度 −/＋/↔」从独立一行移入设置面板 | 省掉一整行（34px），控件仍在 DOM 里（aria-label 不变，测试不受影响） |
| 动作条改为单行横向滚动（≤420 宽时不再 3×2 网格） | 两行 → 一行（约 −44px） |
| 展开窗口高度硬上限 `min(620px, 78% 可用高度)`，超出在岛内滚动 | 内容再多也不会无限拉长，993 → 620 封顶 |
| 布局预设下拉常显在「卡片布局」工具栏（极简/监控/调试） | 一键切极简，卡片数量按需减少 |

## 窗口高度契约变更

`syncWindowSize()` 原本是 `min(内容高度, 92% 可用高度)`，现在多了 620px 封顶：

```ts
const expandedHeightCap = Math.max(60, Math.min(620, Math.floor(availableHeight * 0.78)));
const height = expanded ? Math.min(Math.max(60, renderedHeight), expandedHeightCap) : 60;
```

对应把 `tests/browser/island-readonly.test.ts` 里钉住高度公式的断言改成同一公式（该断言本来就是"窗口高度契约"，行为变更是有意的）。

注意：**没有**在 CSS 里用 `dvh` 做上限——Tauri 窗口高度本身由内容算出，`dvh` 会形成反馈环把岛越压越小，所以上限只放在 JS 里。

## 结果

- 实机：**993 → 620px**（同内容、同预设），动作条一行，诊断一行，卡片区内部滚动。
- 再点「监控 → 极简」可进一步降到 3 张卡以内。

## 验证

| 检查 | 结果 |
| --- | --- |
| `npm run check` | 0 errors / 0 warnings |
| `npm run test:unit` | 123 passed |
| `npm run test:frontend` | 26 passed |
| `npm run test:browser` | 79 passed（含高度契约断言的更新） |
| `npm run test:visual` | 14 passed（8 张展开态基线已按新布局刷新） |

## 未做（可选下一步）

- 卡片级折叠（点标题收起成一行）：收益更大，但会改 `CardGrid` 的布局契约与多张基线，且多处浏览器测试用 `getBoundingClientRect` 断言卡片尺寸，需要一起改。
- 动作条图标化（6 个文字按钮 → 图标 + tooltip）。
