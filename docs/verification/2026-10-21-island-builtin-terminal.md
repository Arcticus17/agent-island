# 岛内内置终端：直接跑 Agent CLI

日期：2026-10-21
范围：`src-tauri/src/terminal.rs`（新）、`src-tauri/src/lib.rs`、`src-tauri/Cargo.toml`、`src/components/terminal/TerminalPanel.svelte`（新）、`src/bridge/{types,tauri,mock}.ts`、`src/app/App.svelte`、`package.json`、`tests/browser/island-actions.test.ts`

## 目标

在灵动岛里直接使用 Agent——不是一次性 `-p` 调用，而是**真正可交互的 TUI 终端**。

## 实现

### 后端：ConPTY

新增 `terminal.rs`，用 `portable-pty 0.9`（Windows 走 ConPTY）：

| 命令 | 作用 |
| --- | --- |
| `terminal_open(program, args, cwd, cols, rows) -> id` | 起 PTY + 子进程，读线程把输出以 `terminal-output` 事件推到前端；进程退出发 `terminal-exit` |
| `terminal_write(id, data)` | 键盘输入写进 PTY |
| `terminal_resize(id, cols, rows)` | 面板尺寸变化时同步 PTY 行列 |
| `terminal_close(id)` | 结束子进程并清掉会话 |
| `terminal_list()` | 当前活动终端（调试用） |

两个限制：

1. **白名单**：只允许 `claude / codex / opencode / hermes / cmd / powershell / pwsh / node / bash` 以及 `agent_defs()` 里配置过的 keyword，避免这个命令变成任意命令执行入口。
2. **shim 包装**：Windows 的 CreateProcess 不认 PATHEXT，`claude` 这种 npm `.cmd` 垫片要过 `cmd.exe /C`（复用既有的 `looks_like_shim` / `wrap_with_cmd`）。

会话表挂在 `AppState.terminals`，和 hook、发送任务一样用 `lock_recover()`。

### 前端

新增 `TerminalPanel.svelte`（xterm.js + fit addon）：

- 展开面板里点「内置终端」→ 自动用当前选中的 Agent 起终端（`Claude Code → claude`、`Codex CLI → codex`…），并把该会话的 `cwd` 带过去；
- 也有「Shell」按钮落回 `cmd.exe`（Agent 没装/没登录时还能用）；
- 尺寸变化 → `fit()` → `terminal_resize`；键盘输入 → `terminal_write`；输出事件按 `id` 过滤后写进 xterm；
- 每次启动前 `reset()`，避免上一个程序的界面残留（这是实机发现的问题：先起 codex 再切 shell 时，旧画面还挂着）；
- 关终端或收起面板 → `terminal_close`，不乱留进程；
- 开终端时把卡片区收起（`.cards-region.terminal-open { display: none }`），面板高度保持在 620px 上限内。

### 桥接契约

`AgentIslandBridge` 增加 5 个**可选**方法（和 `resizeWindow` 同类，不进 `AgentCommand`，避免污染命令联合类型）：

```ts
terminalOpen?(req): Promise<string>;
terminalWrite?(id, data): Promise<void>;
terminalResize?(id, cols, rows): Promise<void>;
terminalClose?(id): Promise<void>;
listenTerminalEvents?(handler): Promise<() => void>;   // {kind:"data"} | {kind:"exit"}
```

`tauri.ts` 实现并校验事件载荷；`mock.ts` 记录调用并支持 `emitTerminal()` 供测试回放。

## 实机验证

| 步骤 | 结果 |
| --- | --- |
| 展开 → 点「内置终端」 | 面板出现，`codex` 自动启动，标题显示「终端 codex」，状态「运行中」 |
| Codex CLI 自己的界面 | 岛内渲染出 Codex 的更新提示（`✨ Update available! 0.153.4 → 0.163.0 … Press enter to continue`）和早期的 workspace 信任提示——说明是真 TUI 在 PTY 里跑 |
| 点「Shell」 | 立即出现 `Microsoft Windows [版本 10.0.26200.8457]` + `C:\Users\<you>>` 提示符 |
| 键盘输入 `echo island-terminal-ok` + Enter | 终端回显命令并打印 `island-terminal-ok`——输入、输出双向都通 |
| 点「关闭」/ 收起面板 | `terminal_close` 结束子进程，面板回到「未启动」 |

截图：`mac-island-analysis/terminal-agent.png`（Codex TUI）、`terminal-typed.png`（echo 往返）、`terminal-open.png`（面板结构）。

## 测试

| 检查 | 结果 |
| --- | --- |
| `cargo test --no-default-features --lib` | 143 passed（新增 2 条：程序白名单、shim 包装） |
| `npm run check` | 0 errors / 0 warnings |
| `npm run test:unit` | 123 passed |
| `npm run test:frontend` | 26 passed |
| `npm run test:browser` | 83 passed（新增 2 条：终端启动/关闭、收起面板时清理 PTY） |
| `npm run test:visual` | 14 passed（展开态多了「内置终端」按钮，基线已刷新） |

## 坑与边界

- **Codex 会自动更新**：首次在 PTY 里运行 `codex` 时它自己跑 `powershell -c "irm …/install.ps1 | iex"`，需要用户按回车确认；这是 CLI 的行为，不是岛的逻辑。
- **终端不跨面板存活**：收起面板会关闭 PTY（组件卸载）。需要"后台常驻终端"的话，要把会话从组件生命周期里挪到 App 级状态。
- **暂无多标签**：一次一个终端；多标签/分屏是后续项。
- **中文输入法/组合键**：xterm 走标准 onData，IME 未专门测试。
- 依赖新增：`portable-pty 0.9`（Rust）、`@xterm/xterm` + `@xterm/addon-fit`（前端）。
