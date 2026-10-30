# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

在 Windows 上同时跑多个 AI 编程 Agent（Claude Code、Codex CLI、OpenCode、Hermes）的开发者。
他们的工作场景是：终端和编辑器占满屏幕，Agent 在自己看不见的窗口里跑几分钟到几十分钟；
他们需要的不是"再开一个仪表盘"，而是在不切换窗口的前提下，用一眼确认三件事：
还在跑吗、是不是卡住了、有没有在等我确认。

## Product Purpose

Agent Island 是常驻桌面的灵动岛：把"正在跑的 Agent 现在是什么状态"压缩进一条几十像素高的胶囊，
需要时在同一块表面上展开成可操作的面板，用完收回。成功的样子是用户一天里看它几十次，
但每次都不超过两秒，也不会因为它错过一个需要确认的危险命令。

## Positioning

状态只来自结构化事件（Claude hooks、官方会话事件）与会话身份绑定，正文文本不改变状态；
无法确认会话时，岛上明确显示"无法确认当前会话"，而不是拿最近的日志冒充当前任务。
这是同类悬浮窗/托盘工具普遍做不到的：它们要么只显示进程存活，要么用日志文本猜状态。

## Operating Context

- 常驻 Windows 桌面顶部，透明原生窗口，宽度可调（默认 420，展开 560），高度随内容自适应但封顶。
- 全局快捷键 `Ctrl+Alt+I` 呼出/隐藏，`Ctrl+Alt+O` 打开总览窗口；托盘常驻。
- 视觉回归基线只在 Windows + 锁定版 Playwright Chromium 下有效。
- 会话总览是另一个窗口（`OverviewApp.svelte`），不属于本次改造范围。

## Capabilities and Constraints

- Tauri 2 / Rust 后端 + Svelte 5 / TypeScript / Vite 前端；前端只通过 `src/bridge` 与后端通信。
- 主题、布局、设置存 localStorage，并镜像到 `~/.agent-island/settings.json`。
- 界面语言为中文；WebView2 环境，不加载远程字体，CSP 收紧。
- 三种仪表状态（进行中/已完成/报错/等待/空闲/已停止）来自后端 `display_status`。
- 审批、通知、收件箱、内置终端、命令面板、诊断卡、布局编辑（拖拽排序/尺寸/隐藏）都属于主岛现有功能，改造不能删。

## Brand Commitments

- 名称：Agent Island（界面标题保留 "Agent Island"）。中文口语称"小岛/灵动岛"。
- 像素吉祥物（`PixelAgent`）是唯一的品牌图形资产，保留。
- 语气：直白、说明原因、不夸张；错误要写清发生了什么和怎么恢复。

## Evidence on Hand

- 真实界面基线：`tests/visual/__screenshots__/island.visual.test.ts/*.png`（改造前）。
- 功能与验证记录：`docs/verification/`（用例与验收口径），`README.md`。
- 下一阶段概念稿（未实现，仅供讨论）：`docs/research/2026-10-28-concepts/`。
- 没有真实用户访谈、没有对外营销素材；不得编造用户量、性能数字或客户名。

## Product Principles

1. 状态先于一切：任何界面元素都不能比"现在是否正常"更早被看到。
2. 颜色是告警，不是壁纸：没有需要用户处理的事时，界面保持中性；需要处理时，颜色整片亮起。
3. 不给假答案：不确定就说不确定，并给出下一步；不用历史记录或日志文本冒充当前任务。
4. 原地完成：能在岛上做完的事，不要求用户开窗口、换应用。
5. 安静常驻：常驻不等于打扰；收起后是一条 40px 的贴边刘海（吉祥物 + 状态记号 + 计数），需要时可以完全藏成一条刻度。

## Accessibility & Inclusion

- 键盘可达：所有交互元素可 Tab 聚焦、有可见焦点环；快捷键在引导卡中说明。
- 尊重 `prefers-reduced-motion`：动画时长降到 1ms，循环动画只播一次。
- 状态不只靠颜色表达：每个状态都有文字标签；颜色是第二通道。
