# Agent 更新按钮（联网检查版本 + 岛内执行升级）

日期：2026-10-21
范围：`src-tauri/src/updates.rs`（新）、`src/agent/updates.ts`（新）、`src/app/App.svelte`、`src/bridge/{types,tauri,mock}.ts`

## 需求

在「内置终端」按钮旁边加一个 Update 按钮，用来更新 Agent；版本检测需要联网，所以要把灵动岛访问网络的路打通。

## 分工：本地版本走 Rust，最新版本走 WebView

| 需要的东西 | 走哪条路 | 为什么 |
| --- | --- | --- |
| 本地装的版本 | Rust 命令 `probe_cli_version`，跑 `<cli> --version` | 不联网，但需要 CreateProcess |
| npm 上的最新版本 | 前端直接 `fetch("https://registry.npmjs.org/<pkg>/latest")` | registry 返回 `access-control-allow-origin: *`，WebView 里跨域就能拿；不需要在 Rust 侧背一个 HTTP 客户端 |
| 升级 | 丢进岛内终端跑 `npm install -g <pkg>@latest` | 过程、进度、报错全部直接可见 |

`tauri.conf.json` 的 `security.csp` 是 `null`（不限制），所以 WebView 的 `fetch` 可以直接出网——这就是"打通网络"的全部改动，不用加 Rust HTTP 依赖。

探测命令统一过 `cmd /C <program> --version`：npm 装的 CLI 多是 `.cmd` shim，直接 `Command::new("claude")` 会 spawn 失败（实测 codex 是原生 exe 能跑，claude 是 shim 跑不了，所以第一版 Claude 显示"检查更新"而不是"更新 v2.1.306"）。

## 交互

按钮就挨着「内置终端」，一个按钮走完所有状态：

| 状态 | 文案 |
| --- | --- |
| 未查过 | 检查更新 |
| 查询中 | 检查中… |
| 有新版本 | **更新 v2.1.306**（强调色） |
| 已最新 | 已是最新 |
| 网络失败 | 重试检查 |

- 展开面板时自动查一次（每个 Agent 一次）；换 Agent 会重查；
- 点「更新」→ 自动打开内置终端，在里面跑 `npm install -g <pkg>@latest`，命令退出后重新探测版本；
- npm 包映射：Claude Code → `@anthropic-ai/claude-code`、Codex CLI → `@openai/codex`、OpenCode → `opencode-ai`；Hermes 没有已知包，按钮显示"不支持更新"。

## 实机验证

| Agent | 本地版本 | npm 最新 | 按钮 |
| --- | --- | --- | --- |
| Codex CLI | 0.163.0 | 0.163.0 | 已是最新 |
| Claude Code | 2.1.270 | 2.1.306 | **更新 v2.1.306**（强调色） |

截图：`mac-island-analysis/update-button.png`（Codex 已最新）、`update-button-claude3.png`（Claude 可更新）。

升级动作没有在实机上真跑（会真的改用户环境），由单测断言命令形状：
`cmd.exe /C "npm install -g @anthropic-ai/claude-code@latest"`。

## 测试

- Rust：`parse_version` 三个用例（普通 / v 前缀 / prerelease / 无版本）；
- 单测 `tests/updates.test.ts` 6 条：包映射、版本解析、版本比较、registry 成功/失败、命令拼装；
- 浏览器测试 3 条：有新版本时显示并点击后开终端跑安装、已最新、registry 503 时退化为「重试检查」。
- 视觉测试里把 `fetch` 固定成"已是最新"，避免真实网络让按钮文案抖动（8 张基线重生成）。

## 回归门禁

`check` 0/0 · `unit` 129 · `frontend` 26 · `browser` 98 · `visual` 14 · Rust 146。
