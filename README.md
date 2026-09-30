# Agent Island

一个常驻 Windows 桌面的“Agent 灵动岛”：一眼看到正在运行的 AI 编程 Agent，并直接在桌面上控制它们。

## 功能

- 常驻灵动岛：点击顶部展开/收起，支持托盘和全局呼出
- Agent 监控：Claude Code、Codex CLI、OpenCode、Hermes
- Codex 的实时进程入口针对独立 CLI；桌面端 app-server 不代表单个 CLI 会话。本地桌面根会话可通过胶囊“查看会话”手动选择，实时桌面任务状态和控制尚未接入，不会用最近日志冒充当前任务。
- 会话一致性：默认只显示可靠匹配的当前会话；历史记录可在总览或胶囊中明确选择查看。未手动选择时不会用历史代替当前；所选记录消失时提示重新选择，不偷偷换成其他对话。
- 状态真相源：进行中、已完成、报错与等待只由结构化事件（Claude hooks、官方会话事件）结合会话绑定判定；正文文本不会改变状态，最多作为诊断提示出现，陈旧日志不会代表当前任务。
- 单实例：重复启动不会出现第二个小岛。后启动的进程会自行退出，并尝试把已在运行的窗口唤到前台；这是必要的，因为 hook 端口只属于第一个进程，重复实例会收不到事件却照常写统计。
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

仓库当前版本为 1.9.0，仓库进度不代表对应安装包已经发布。原生 DPI、多显示器及安装/卸载验收仍待完成，详见[验收记录](docs/verification/2026-09-04-adaptive-ui.md)与[视觉基线刷新记录](docs/verification/2026-09-30-visual-baseline-refresh.md)。

本项目面向其他用户分发，不应依赖开发者本机环境。正式发布前须完成[公开发布适配门槛](docs/verification/public-release-compatibility.md)，涵盖干净系统、普通权限、中文/空格路径、CLI 安装方式、混合 DPI、多屏和升级卸载。未实测的系统与架构不列为已支持。

## 布局与外观

首次加载和重新展开胶囊时，默认优先显示正在执行任务的 Agent；无人执行时，选择后端记录中最近结束任务的 Agent（包括成功或失败）。多个 Agent 同时执行时保留当前执行中的选择。展开后可手动切换，普通轮询不会抢回选择；显式开启“聚焦错误”时仍按该模式显示。

展开小岛后点击“编辑”，通过拖拽或每张卡的上移/下移按钮调整顺序，并选择紧凑、标准或宽版尺寸。用量、会话、统计卡可隐藏，在“已隐藏”区恢复；状态和日志卡始终保留。身份栏、审批与核心操作不参加卡片排序。

提供“极简”“监控”“调试”三个预设；“重置”恢复当前选中的预设，自定义配置重新启动后默认以监控作为重置目标。窄窗口自动变成单列，保存的顺序与尺寸不会被覆盖。

布局更改自动保存在当前应用的本地存储。配置损坏时尝试保留原文备份并回退监控预设；存储失败会提示“已应用，但暂时未保存”，下次调整会重试。在“设置 → 外观”切换石墨玻璃/纯黑，在同一菜单开启隐私遮罩或专注模式。

卡片尺寸会实际改变内容密度：紧凑、标准、宽版的日志区高度分别为 96、164、240px。展开与工作状态带有轻量动画，系统开启减少动画时停用。透明窗口外层不使用背景模糊或外溢阴影，避免胶囊外出现矩形合成边缘；原生 WebView2 表现仍需实机验收。

### 操作与数据边界

- 胶囊“查看会话”列出当前快照中的本地记录，选择后展示对应对话与目录，并标记“本地记录／非实时状态”。目录和终端使用所选会话；终端仅打开工作目录，不代表恢复桌面任务。历史模式禁止停止、重启与跳回，避免作用到其他运行任务。切换 Agent 会退出手动查看。
- Codex 数据目录优先采用非空 `CODEX_HOME`，否则使用用户目录 `.codex`；日志和用量使用同一数据根目录。历史可见记录仍有数量上限；明确识别的 Desktop 根用户会话优先于辅助日志，未知来源不冒认为桌面根任务，所有候选仍参与身份歧义检查。

- 总览打开时还原并置顶，窗口操作失败会提示；跳回只依赖运行进程，不再要求已匹配会话。Windows 拒绝切换焦点时不会报告成功。
- 目录、终端需要可靠的当前会话信息；缺失时点击会说明原因，不会猜测历史目录。会话缺失不再隐藏状态、用量和布局编辑。
- 胶囊“重启”使用已记录的启动方式重新打开 Agent，**不会停止现有任务**；“已请求启动”不代表 CLI 已启动成功或任务已恢复。总览“恢复”打开指定会话的恢复终端，需在终端确认后续状态；CLI 不在 PATH 中会明确报错。
- 设置中的隐私遮罩、专注模式、主题会显示当前含义；Claude hooks 开关仍需后端返回确认。浏览器测试不能替代真实 hooks 和窗口操作验收。
- Codex 长会话元数据按完整首记录读取（上限 1MiB），不再因首行超过 16KiB 而漏掉有效会话；当前会话仍须经过身份匹配。

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

若 `npx playwright install chromium` 长时间无进展（官方地址会跳转到 Google 存储，实测只有几十到两百 KB/s），可用 npmmirror 镜像装同一版本：从 `npx playwright install --dry-run chromium` 读出版本号与目录名，下载 `https://cdn.npmmirror.com/binaries/chrome-for-testing/<版本>/win64/chrome-win64.zip`，解压到 `%LOCALAPPDATA%\ms-playwright\chromium-<revision>\`，确认 `chrome-win64\chrome.exe` 存在，再在该目录下创建空的 `INSTALLATION_COMPLETE` 文件。实测镜像速度约为官方路径的 26 倍。

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
- [验证记录](docs/verification/)：会话归属、状态判定、布局与公开发布门槛的实测记录
