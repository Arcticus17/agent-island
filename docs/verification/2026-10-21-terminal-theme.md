# 终端主题（换皮，不动布局）

日期：2026-10-21
范围：`src/themes/terminal.css`（新）、`src/themes/theme-style.ts`（新）、`src/app/App.svelte`、`src/components/controls/{SettingsPopover,ActionBar}.svelte`、`tests/theme-tokens.test.ts`、`tests/visual/island.visual.test.ts`

## 结论

选的是**换皮不换结构**：等宽字体、方角、纯黑、去掉玻璃/模糊/阴影，布局、间距、交互一个没动。
主题作为第三套与"石墨玻璃""纯黑"并列，设置里可随时切回去。

## 改了什么

| 维度 | 做法 |
| --- | --- |
| 字体 | `Cascadia Mono / Consolas / Sarasa Mono SC / monospace`，表单控件显式 `font-family: inherit`（默认不继承） |
| 圆角 | 主题内 `border-radius: 0 !important`，包含 `.agent-island` 根节点（第一版漏了根，胶囊还是圆的） |
| 玻璃 | `backdrop-filter: none`、`box-shadow: none`、`--elevation-*: none`、`--island-background` 换成纯色 |
| 配色 | chrome 全单色（白灰）；状态色保留但降饱和（工作中=白、已完成=灰绿、等待=暗金、错误=灰红） |
| TUI 记号 | 诊断行 `❯`、卡片眉标 `#`、选中态反相（白底黑字）、焦点改虚线 |
| 分隔 | 卡片保留 1px 边框 + 顶部一条极淡的白色渐变，模拟终端表格线 |

状态色没有做成纯灰度：用量条、状态点的语义靠颜色区分，纯灰度会让"完成/报错"只能靠肉眼读字。这是有意的折中。

## 代码结构

- `theme-style.ts` 成为主题的单一来源（`THEME_STYLES / ThemeStyle / THEME_LABELS / storedThemeStyle()`），
  App 与两个控件都从这里取类型，不再各自写 `"graphite" | "pure-black"`；
- 主题色全部走 `themes/terminal.css`，组件里零改动（除了设置里多一个 option）；
- `theme-tokens.test.ts` 的契约用例扩到三个主题，保证新主题必须定义全部 32 个语义 token。

## 验证

- 视觉回归：新增 7 张 `terminal-*` 基线（14 → **21**），确认方角、等宽、单色都落在截图上；
- 实机切主题：设置 → 外观 → 终端，岛立即变方角等宽（`mac-island-analysis/terminal-theme-app.png`）；
- 切回石墨玻璃同样即时生效。

## 回归门禁

`check` 0/0 · `unit` **130** · `frontend` 26 · `browser` 98 · `visual` **21**。

## 没做的（下一档再说）

- ASCII 框线：中文不是等宽，框线一律用 CSS 画；
- 顶部状态栏 / 提示符行 / 键位优先：属于 B 档（TUI 结构），要动 markup；
- CRT 扫描线、辉光、开机自检：C 档，且必须可关闭。
