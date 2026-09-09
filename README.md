# Agent Island

一个常驻 Windows 桌面的“Agent 灵动岛”：一眼看到正在运行的 AI 编程 Agent，并直接在桌面上控制它们。

## 功能

- 常驻灵动岛：点击顶部展开/收起，支持托盘和全局呼出
- Agent 监控：Claude Code、Codex CLI、OpenCode、Hermes
- 会话一致性：灵动岛仅显示可靠匹配的当前会话；历史会话仅在总览中显示。匹配不明确时会显示“无法确认当前会话”，不会选择最新的历史记录代替。
- 事件通知：报错/完成/等待通知卡，按 Agent 分组折叠，自动展开与收起
- 用量监控：Claude Code 5 小时窗口 tokens/费用、Codex 额度百分比与重置倒计时，超限变色预警
- Hook 事件：Claude Code 会话完成/通知秒级直达；危险命令在岛上弹审批卡，允许/拒绝/回退原生确认
- 终端跳回：一键跳回 Agent 所在终端窗口
- 会话总览：独立窗口实时列出会话、日志、统计，可发消息、恢复、停止、重启
- 全局快捷键：`Ctrl+Alt+I` 呼出/隐藏小岛，`Ctrl+Alt+O` 打开总览
- 隐私遮罩：在设置中手动开启，模糊日志与路径
- 专注模式：全部状态、聚焦错误、静音通知
- 自适应布局：卡片拖拽或按钮排序，尺寸切换、隐藏与恢复，自动保存
- 主题：石墨玻璃与纯黑；尊重系统减少动画偏好，总览保留系统浅色模式
- 统计报表：按天/周查看各 Agent 用时、报错数、完成数

## 安装

从 GitHub Release 下载：

- `*-setup.exe`：Windows 安装器
- `*.msi`：MSI 安装包

日常使用直接运行安装后的 `Agent Island` 即可。

仓库当前版本为 1.8.2，仓库进度不代表对应安装包已经发布。当前分支的原生 DPI、多显示器及安装/卸载验收仍待完成，详见[验收记录](docs/verification/2026-09-04-adaptive-ui.md)。

## 布局与外观

首次加载和重新展开胶囊时，默认优先显示正在执行任务的 Agent；无人执行时，选择后端记录中最近结束任务的 Agent（包括成功或失败）。多个 Agent 同时执行时保留当前执行中的选择。展开后可手动切换，普通轮询不会抢回选择；显式开启“聚焦错误”时仍按该模式显示。

展开小岛后点击“编辑”，通过拖拽或每张卡的上移/下移按钮调整顺序，并选择紧凑、标准或宽版尺寸。用量、会话、统计卡可隐藏，在“已隐藏”区恢复；状态和日志卡始终保留。身份栏、审批与核心操作不参加卡片排序。

提供“极简”“监控”“调试”三个预设；“重置”恢复当前选中的预设，自定义配置重新启动后默认以监控作为重置目标。窄窗口自动变成单列，保存的顺序与尺寸不会被覆盖。

布局更改自动保存在当前应用的本地存储。配置损坏时尝试保留原文备份并回退监控预设；存储失败会提示“已应用，但暂时未保存”，下次调整会重试。在“设置 → 外观”切换石墨玻璃/纯黑，在同一菜单开启隐私遮罩或专注模式。

## 快捷键

| 按键 | 功能 |
| --- | --- |
| `Tab`、`Enter` / `空格` | 聚焦并操作按钮；在顶部按钮上展开/收起 |
| 总览会话列表中的方向键、`Home` / `End` | 切换会话 |
| `Ctrl+Alt+I` | 全局呼出/隐藏小岛 |
| `Ctrl+Alt+O` | 全局打开总览 |

以上以默认 Svelte 界面为准；旧界面特有的单字母快捷键和右键菜单不属于当前默认入口。

在“设置 → Claude 事件”检查或切换 hooks 接入（会写入 `~/.claude/settings.json`，原文件自动备份为 `settings.json.agent-island.bak`）。

## 配置

- `~/.agent-island/agents.json`：Agent 适配器配置，新增 Agent 无需改代码
- `~/.agent-island/stats.json`：累计统计
- `~/.agent-island/stats-daily.json`：每日统计

适配器格式参考 [agent-adapters.example.json](agent-adapters.example.json)。

## 隐私说明

- Agent 状态、日志、统计都保存在本机，不上传任何密钥
- 诊断导出仅导出经脱敏的诊断元数据，例如应用/系统版本、适配器、会话哈希、事件类型、问题代码与计数、时间与过期状态、项目目录名和消息长度；不包含提示词、凭据或完整路径。

## 开发

技术栈为 Tauri 2 / Rust + Svelte 5 / TypeScript + Vite。前端通过 `src/bridge` 与后端通信，状态协调、布局模型和主题令牌分别独立维护，方便局部替换。当前验收目标是 Windows，不代表已支持 macOS。

准备 Node.js 22.12+（22.x）、Rust MSVC 工具链、Visual Studio C++ Build Tools 和 WebView2。首次安装测试浏览器需要联网。

```bash
npm ci
npx playwright install chromium
npm run tauri dev
```

打包与测试：

```bash
npm run tauri build
npm run check
npm test
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

`npm test` 包含旧前端测试、单元测试、浏览器交互、视觉比对和 Rust 测试。Windows 终端须先加载 Visual Studio 开发环境。打包可能需要联网下载 NSIS/WiX 工具；构建产物位于 `src-tauri/target/release/bundle`。

视觉回归使用 Windows 与依赖锁定的 Playwright Chromium；运行 `npm run test:visual` 比对。仅在人工检查确认界面变化后执行 `npm run test:visual:update` 更新基线。不同操作系统或字体环境的截图不能直接当作同一验收环境。

GitHub Actions 在推送 `v*` 标签时，先执行构建、类型检查与完整回归，再构建 EXE/MSI 并发布到 Release。本地验证不会自动发布。

### 临时回退旧界面

Svelte 界面现为默认实现，主灵动岛与会话总览共用同一个临时回退开关。若升级后需要排查兼容问题，可在开发者控制台执行：

```js
localStorage.setItem("agent-island-ui-legacy", "1");
location.reload();
```

恢复默认 Svelte 界面：

```js
localStorage.removeItem("agent-island-ui-legacy");
location.reload();
```

旧界面只作为迁移期应急回退路径保留。

## 文档

- [开发路线图](ROADMAP.md)
- [与苹果灵动岛的差异研究与升级清单](UPGRADE-RESEARCH.md)
