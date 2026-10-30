# Pi 检测不到：候选目录漏项（2026-10-29）

用户反馈：系统里装了 `pi`（`C:\Users\LENOVO\.pi\agent\bin\pi.cmd`，在 PATH 里），
「检测 / 添加 Agent」面板却只列出 Claude Code / Codex CLI / OpenCode。用户怀疑环境变量检测失效。

## 根因（不是环境变量检测）

`detect_agent_candidates` 的来源只有两类：

1. `agent_defs()` 里**已配置**的 Agent（内置 Claude / Codex / OpenCode / Hermes + `~/.agent-island/agents.json`）；
2. `agent_catalog::CATALOG` 这份**人工维护**的候选清单（Gemini、Aider、Cursor Agent…）。

`pi` 两类都不在：它既没有写进 `agents.json`，也不在 `CATALOG` 里。环境变量探测
（`environment_program_paths_from`）只对**已知 program** 生效，且只认值为文件或目录、名字/路径带 Agent 关键字的变量；
PATH 目录不会拿来枚举候选。所以检测逻辑没有"坏"，是清单漏了一项。

## 修复

- `agent_catalog.rs` 新增 `Pi`（program `pi`，描述 "Pi coding agent"）。
- `known_program_paths_from` 的 home 目录清单新增 `.pi/agent/bin`（Pi 的托管安装在 `~/.pi/agent`，
  不保证 PATH 一定包含它）。
- 新增两条测试：`catalog_programs_are_unique_and_include_pi`、`finds_the_pi_managed_install_directory`。
- 真机验证（临时测试，跑完已删除）：

```text
LIVE_PI resolution=Some("C:\\Users\\LENOVO\\.pi/agent/bin\\pi.cmd")
candidate=AgentCandidate { name: "Pi", program: "pi", installed: true, configured: false, path: Some("...\\pi.cmd"), keyword: "pi" }
```

即：`detect_agent_candidates()` 现在返回 Pi（已安装、未配置）→ 「检测 / 添加」面板会出现 **Pi → 添加**。

## 验证

| 层 | 结果 |
| --- | --- |
| `cargo test --lib` | 231 passed（新增 2 条） |
| 真机 | `resolve_agent_program("pi")` 命中 `~/.pi/agent/bin/pi.cmd`；候选包含 Pi |

## 自定义 Agent 的降级行为（同日）

用户添加 Pi 后，界面暴露出三个"非常规 Agent"问题，一并修了：

1. **终端启动失败**：折叠态点「内置终端」时，前端把 `agentCliName()` 的兜底值 `"Shell"` 当程序名传给后端，
   而后端白名单只允许 `cmd/powershell/pwsh/claude/codex/opencode/hermes/node/bash`，于是报"不允许在岛内启动的程序"。
   现在 `terminalTarget()` 对非内置 Agent 回退到 **`powershell`**，终端抬头显示 `PowerShell`；
   面板里的 `Shell` 按钮仍是 `cmd.exe`。
2. **更新按钮是死的**：没有 npm 包信息的 Agent 不再渲染 `agent-update` 按钮（之前显示灰掉的"不支持更新"）。
3. **满屏"会话未确认"**：`AgentView` 新增 `has_log_adapter`（内置四种为 true）；
   为 false 时面板顶部出一条说明：「「Pi」是自定义 Agent：灵动岛只监控进程状态；会话、日志与用量需要对应的日志适配器。」
   指令框的占位文案也改成「该 Agent 没有日志适配器，无法发送指令或读取会话」。

验证：`cargo test --lib` 232 passed（新增 `only_builtin_agents_have_log_adapters`）；
浏览器 143 passed（新增"custom agents without a log adapter"：说明条、无更新按钮、终端启动自己的命令）。

## 边界

- 候选清单一向是 curated：其他不在清单里的 PATH CLI（例如本机 `~/.grok/bin`、`~/.mimocode/bin`）
  同样不会自动出现；要么加一行 `CATALOG`，要么在「+」面板手动填名称 + 路径。
- Pi 目前只有进程/状态能力（没有日志适配器），添加后能显示是否在跑，但不会解析会话与用量。
