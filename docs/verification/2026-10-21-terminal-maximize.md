# 终端放大（展开）功能

日期：2026-10-21
范围：`src/components/terminal/TerminalPanel.svelte`、`src/app/App.svelte`、`tests/browser/island-actions.test.ts`

## 目标

内置终端默认只有 240px 高、窗宽 420，看 Agent 的 TUI（尤其带长命令/日志的界面）太挤。要能**把终端放大**，用完还原。

## 实现

### 前端状态

`App.svelte` 新增两个状态：

- `terminalMax`：是否处于放大模式；
- `widthBeforeMax`：放大前的窗口宽度，用于还原。

`toggleTerminalMax()`：

1. 放大时把窗口宽度从当前值提到 **720**（`Math.min(760, Math.max(当前宽, 720))`，本来就比 720 宽就不缩）；
2. 还原时按 `widthBeforeMax` 把窗口宽度放回去；
3. 等一帧让 `window.innerWidth` 反映新宽度，再 `syncWindowSize()` 重算高度。

`syncWindowSize()` 的高度上限按模式分档：

```ts
termMax ? 可用高度 * 0.92 : min(620, 可用高度 * 0.78)
```

即放大时允许窗口长到屏幕的 92%，平时仍守 620px。

### 终端区

`TerminalPanel` 新增 `maximized` / `onToggleMax` props：

- `.screen` 常态 240px，`.screen.maximized` 560px（带 240ms 高度过渡）；
- 头部是**图标按钮**（见下节），`data-testid="terminal-maximize"`，`aria-pressed` 反映状态。

### 按钮选型（2026-10-21 迭代）

第一版是文字按钮「放大 / 还原」，重复出现在工具条和终端头部各一个，占宽又啰嗦。改成**终端头部一个图标按钮**：

| 状态 | 图标 | 语义 |
| --- | --- | --- |
| 常规 | Lucide `maximize-2`（`⤢` 对角向外箭头） | 放大（expansion / fullscreen / enlarge 的通用语义） |
| 放大后 | Lucide `minimize-2`（`⤡` 对角向内箭头） | 还原 |

依据：

- Lucide 官方把 `maximize-2` 的别名标为 `fullscreen / arrows / expand`（[lucide.dev/icons/maximize-2](https://lucide.dev/icons/maximize-2)），`minimize-2` 是它的对称配对，两枚图标同一套 24×24 / `stroke-width: 2` / `round` 端点，视觉重量一致；
- 语义上对角向外箭头 = "把这块区域变大"，比 Material 的 `open_in_full`（方框+箭头）在小尺寸下更清晰，也比 VS Code 面板用的 `^`/`v` 折角更少歧义——我们这里不只是"展开面板"，而是连窗口一起放大；
- 图标按钮必须补无障碍标签：`aria-label` + `title`（[CSS-Tricks：icon-only button 的无障碍做法](https://css-tricks.com/do-you-need-an-icon-only-button-without-screwing-up-the-accessibility/)），并且放大态用 `aria-pressed="true"` + 主题色描边给出状态反馈；
- 工具条里重复的那个文字按钮已删除，只保留「收起终端」和一句提示。

图标取自 Iconify 的 Lucide 集（ISC 许可），内联成 `stroke="currentColor"` 的 SVG，跟随主题色，不额外引依赖。

### 放大时让位

`section.adaptive-details` 加 `class:terminal-max`，CSS 把诊断行、指令输入、会话选择、卡片区全部 `display: none`，把高度全给终端；Agent 快捷条和底部动作条保留。

## 实机验证

| 动作 | 窗口尺寸 |
| --- | --- |
| 展开面板 | 420 × 620 |
| 打开内置终端（codex） | 420 × 568 |
| 点图标 `⤢`（放大） | **720 × 817**（终端区 560px，卡片/诊断/输入框隐去，Codex 更新提示完整显示不折行截断） |
| 点图标 `⤡`（还原） | 回到 **420 × 568** |

截图：`mac-island-analysis/terminal-maximized.png`、`terminal-restored.png`。

## 测试

| 检查 | 结果 |
| --- | --- |
| `npm run check` | 0 errors / 0 warnings |
| `npm run test:unit` | 123 passed |
| `npm run test:frontend` | 26 passed |
| `npm run test:browser` | **85 passed**（新增 2 条：放大/还原切换 `terminal-max` 与 `.screen.maximized`、放大状态下关闭终端会退出放大并回收 PTY） |
| `npm run test:visual` | 14 passed（默认布局不含终端，基线无变化） |
| Rust | 本轮无后端改动（上一轮 143 passed 不变） |

测试刻意只断言"状态切换 + 至少请求过一次窗口尺寸调整"，不断言具体像素：窗口宽度/高度由宿主（Tauri 窗口 vs 测试浏览器视口）决定，真实尺寸在实机截图里验证。

## 边界

- 放大模式只在终端打开时可用；收起终端会自动退出放大并还原窗口宽度。
- 还原用的是"打开终端前的宽度"；如果放大的同时手动拖过窗口宽度，还原仍回到记录值。
- 终端仍不跨面板存活：收起面板会关 PTY（组件卸载）。
- 560px 的终端高度是定值（不是 `dvh`），避免"窗口高度依赖内容、内容高度依赖窗口"的循环。
