# 修复：收起/展开灵动岛会重启 Agent 终端

日期：2026-10-21
范围：`src/app/App.svelte`、`src/components/terminal/TerminalPanel.svelte`、`tests/browser/island-actions.test.ts`

## 现象

内置终端开着的时候，点一下胶囊收起岛、再点开，里面的 Agent（如 `codex`）会重新启动：TUI 从头加载、未完成的输入丢失、进程 PID 变化。

## 根因

终端会话的生命周期绑在了**展开面板的组件树**上：

- `App.svelte` 里展开内容整体包在 `{#if expanded}` 中，收起岛 → `TerminalPanel` 被卸载；
- `TerminalPanel` 的 `onMount` 会「自动开一个终端」，卸载时又执行 `stop(false)` → `terminal_close` 杀掉 PTY；
- 于是每次收起/展开就是一次「关掉 + 重开」，Agent 自然重启。

## 修法：把会话所有权上移到 App

`TerminalPanel` 退化成纯渲染组件（xterm + fit + 键盘），会话状态全部由 `App` 持有：

| App 状态 | 作用 |
| --- | --- |
| `terminalId` | 当前 PTY 会话 id |
| `terminalProgram` | 启动的程序（claude / codex / cmd.exe） |
| `terminalText` | 累积输出（上限 200k 字符，超出保留尾部 150k） |
| `terminalStarting` / `terminalExited` / `terminalError` | 状态展示 |
| `terminalSize` | 最近一次 xterm 的行列，用于新建会话 |

- `listenTerminalEvents` 改由 App 在 `onMount` 里**常驻订阅**——收起面板/收起岛期间输出照样进 `terminalText`，重新展开时面板整体重放，历史不丢；
- `TerminalPanel` 卸载时只 `dispose()` xterm，不碰 PTY；重新挂载时按 `sessionId` 直接接上；
- 真正结束会话只有两条显式路径：终端头部的「关闭」按钮，或「收起终端」（`toggleTerminal` 里调 `stopTerminal`）；
- 顺手修掉：收起岛时如果处于终端放大态，先退出放大并还原窗口宽度，避免收起后的胶囊还挂着 720 宽。

## 验证

**自动化**（`tests/browser/island-actions.test.ts` 新增 1 条）：

```
keeps the agent session alive when the island collapses and reopens
  → 收起前面板挂载，terminal_open 只调用 1 次
  → 收起岛（面板卸载）再展开
  → 仍然只有 1 次 open、0 次 close，phase 仍是 running
```

**实机**（同一台机器，DPI 125%，窗口 525×775 物理）：

```
PTY children before collapse: [30056, 31664]     # cmd.exe + node(codex)
collapsed:                    525x75
PTY children while collapsed: [30056, 31664, ...] # 进程仍在
re-expanded:                  525x775
PTY children after reopen:    [30056, 31664]     # PID 完全一致 → 没有重启
```

重新展开后终端里仍是同一个 Codex 会话（`OpenAI Codex (v0.163.0)` / `Ask Codex to do anything`），没有被重新初始化。

截图：`mac-island-analysis/live-reopen.png`。

## 回归门禁

`check` 0/0 · `unit` 123 · `frontend` 26 · `browser` **86** · `visual` 14；Rust 无改动。
