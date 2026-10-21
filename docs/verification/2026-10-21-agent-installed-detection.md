# Agent 只在"真的存在"时才显示：可执行文件探测

日期：2026-10-21
范围：`src-tauri/src/process_scan.rs`

## 背景

两个诉求：

1. 岛上的 Agent 要分两种显示模式（「全部检测」/「仅运行中」）——前端已完成。
2. 「全部检测」过去的语义是**配置里有几个适配器就显示几个**，于是会出现：
   - 从没装过的 Agent（例如本机没装 Hermes）长期以"已停止"占位；
   - **装了又卸载、日志还留着**的 Agent，看起来像"存在"。

## 改法

在 `scan_agents()` 里，对每个适配器先做一次"可执行文件是否还在"的判断：

```rust
let processes = matching_processes(sys, &def.keyword);
if processes.is_empty() && !program_available(&def.keyword) {
    session.activity.remove(agent_name);
    session.runtime_start.remove(agent_name);
    continue; // 不再进快照，也不再扫它残留的会话日志
}
```

判定只看**可执行文件**，不看日志/配置/会话残留：

1. `where.exe <keyword>`（PATH，含 PATHEXT，覆盖 npm 全局 `.cmd` 安装）；
2. 兜底常见落点：`%APPDATA%\npm`、`%LOCALAPPDATA%\npm`、`~/.local/bin`、`~/.cargo/bin`、`~/.bun/bin`、`~/.opencode/bin`、`~/.claude/local`（各带 `"" / .exe / .cmd / .bat / .ps1` 后缀）。

结果缓存 60s（`OnceLock<Mutex<HashMap<..>>>`），避免每次轮询都起 `where.exe`；只在"没有进程"时才会调用。

## 语义结果

| 情形 | 改前 | 改后 |
| --- | --- | --- |
| 正在运行 | 显示（运行中） | 显示（运行中） |
| 已安装、当前没跑 | 显示（已停止） | 显示（已停止） |
| 从没装过 | 显示（已停止 + 诊断噪音） | **不显示** |
| 装过又卸载、日志残留 | 显示（已停止） | **不显示** |

本机实测：`where claude / codex / opencode` 都能解析到，`hermes` 不在 PATH → 改后 Agent 条从 4 个（Claude / Codex / OpenCode / Hermes）变成 3 个，诊断提示也从 12 项降到 10 项（Hermes 的两条采集告警随之消失）。

## 副作用与边界

- 卸载残留的会话/日志不再被扫描，也不再计入诊断——这是有意的：残留日志不能证明 Agent 还在。
- 自定义适配器（`~/.agent-island/agents.json`）的 `keyword` 同时用于进程匹配和可执行文件探测；如果某个自定义 CLI 的可执行名和 `keyword` 不同，未运行时会不显示。需要的话后续给 `AgentDef` 加独立的 `exe` 字段。
- 用 `npx` 临时跑、没全局安装的 CLI：运行中仍能被进程参数匹配到（`args` 里含包名），退出后即隐藏。

## 验证

| 检查 | 结果 |
| --- | --- |
| `cargo test --no-default-features --lib` | 140 passed（新增 `detects_installed_cli_and_rejects_uninstalled_leftovers`） |
| `npm run check` | 0 errors / 0 warnings |
| `npm run test:unit` / `test:browser` / `test:visual` | 123 / 79 / 14 passed |
| 实机 | Agent 条 4 → 3（Hermes 消失），诊断 12 → 10 项 |
