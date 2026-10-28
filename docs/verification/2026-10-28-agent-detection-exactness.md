# Agent 检测与系统一致性：只显示本机真实存在的 Agent

日期：2026-10-28
范围：`src-tauri/src/process_scan.rs`、`src-tauri/src/lib.rs`、`src/bridge/types.ts`、`src/bridge/tauri.ts`、`src/bridge/mock.ts`、`src/app/App.svelte`、`tests/browser/inbox-and-approval.test.ts`、`tests/verification`。

## 一、问题

用户反馈：岛里显示的 Agent 与系统里实际安装的 Agent 不对应，有缺少。实测发现两个独立根因：

### 1. 幽灵进程：命令行里出现关键词就被当成 Agent

旧实现（非 Codex 分支）：

```rust
name.contains(keyword) || args.join(" ").contains(keyword)
```

任何进程只要**命令行文本**里出现 `opencode` / `hermes`（包括 PowerShell 的搜索命令、提示词、grep 参数），就会被算成该 Agent 正在运行。于是岛里出现了实际没装的 OpenCode/Hermes，且因为「有进程」绕过了「可执行文件存在」检查。

### 2. 已安装探测漏掉自定义 npm prefix

`known_program_paths` 只查了 `%APPDATA%\npm`、`%LOCALAPPDATA%\npm`、`.local/bin` 等少数目录。本机 npm 的 `prefix` 被 `~/.npmrc` 改成了 `D:\node_global`，Claude/Codex 的 shim 在这里；一旦 `where claude` 因 PATH/临时安装问题失败，岛就把已安装的 Claude 判成「不存在」。

另外，Claude 有一次 npm 安装只留下 500 字节的 `native binary not installed` 占位文件、全局 shim 缺失；这是安装问题，不是岛的数据问题，已用 `npm install -g @anthropic-ai/claude-code@2.1.312 --include=optional --foreground-scripts` 修复。

## 二、修复

### 1. 进程匹配只看可执行文件/入口，不看提示词

`matches_agent_process` 现在：

- 比较可执行文件名（`claude.exe` / `claude.cmd` / `claude`）；
- Node 场景只看**脚本路径**（`node .../opencode/bin/opencode.js`），不看第 2 个之后的参数；
- `cmd.exe /C <agent>.cmd` 只看 `/C` 后的命令 token；
- Codex 保留原有 `app-server` 排除逻辑；
- 任意 `powershell.exe -Command "... opencode ..."`、`node server.js --prompt hermes` 都不再匹配。

### 2. 已安装探测覆盖真实安装位置

`known_program_paths_from` 现在汇总：

- `~/.npmrc` 的 `prefix=`（本机 `D:\node_global`）
- `NPM_CONFIG_PREFIX` / `npm_config_prefix`
- `PNPM_HOME`、`SCOOP`/`SCOOP_GLOBAL`（shims）、`ChocolateyInstall`（bin）、`VOLTA_HOME`（bin）、`BUN_INSTALL`（bin）
- `%APPDATA%\npm`、`%LOCALAPPDATA%\npm`、`%ProgramFiles%\nodejs`、`%ProgramFiles(x86)%\nodejs`
- `~/.local/bin`、`~/.cargo/bin`、`~/.bun/bin`、`~/.opencode/bin`、`~/.claude/local`、`~/.npm-global/bin`、`~/.hermes/bin`、`~/.volta/bin`、`~/.local/share/pnpm`、`~/scoop/shims`
- 所有 `PATH` 目录，以及每个目录的 `node_modules/.bin`
- Windows 优先 `.exe` → `.cmd` → `.bat` → `.ps1`，最后才回退无扩展名 shim（`CreateProcess` 不能运行 POSIX 无扩展名脚本）

`resolve_agent_program` 返回**实际路径**，不再只返回 bool；`program_available` 复用它。缓存 60 秒，`refresh_agent_detection` 会清缓存。

### 3. 环境变量探测

除 PATH/常见目录外，新增 `environment_program_paths`：

- **直接文件值**：任意环境变量的值本身是文件，且文件名匹配 Agent（例如 `CLAUDE_CODE_PATH=C:\tools\claude.exe`）→ 直接采用；
- **目录值**：变量名或路径里包含 Agent 关键字（例如 `CLAUDE_CONFIG_DIR`、`CODEX_HOME`、`OPENCODE_HOME`、`HERMES_HOME`），在该目录及 `bin/`、`local/`、`node_modules/.bin/` 下查找 `<agent>.exe/.cmd/.bat/.ps1`；
- `PATH` 本身不按目录值处理（已有专门的 PATH 解析）；
- 只认**真实存在的文件/目录**；提示词、token、任意文本不会变成 Agent。

环境变量命中的路径优先于常见安装目录，诊断里显示为「环境变量」，tooltip 带变量名。纯函数 `environment_program_paths_from` 有单测覆盖直接文件、目录 + `bin`、提示词误值三种情况。

「+」面板还会扫描环境变量指向的目录（含 `bin/`、`local/`、`node_modules/.bin/`），把文件名带 `agent`/`claude`/`codex`/`opencode`/`hermes`/`gemini`/`aider`/`cursor`/`copilot` 等关键字的可执行文件列为**候选**，例如 `MY_AGENT_HOME=C:\tools\my-agent` → 候选 `my-agent.exe`；只接受 `.exe/.cmd/.bat/.ps1/.com`，`.json/.err/.dll` 不会出现；变量名或值不含 Agent 关键字的通用目录（`JAVA_HOME`/`TEMP`/`HOME`）不扫。仍然需要用户点「添加」。

支持示例：

```text
CLAUDE_CODE_PATH=C:\tools\claude.exe
CLAUDE_CONFIG_DIR=C:\Users\me\.claude      → .claude\local\claude.exe
CODEX_HOME=C:\tools\codex                 → codex\bin\codex.exe
OPENCODE_HOME=D:\tools\opencode           → opencode\bin\opencode.cmd
```

注意：这里读的是**应用进程启动时继承的环境变量**；运行中修改系统环境变量后，需要点「重新检测」并重启应用才会看到新值。

### 4. 诊断可见：为什么这个 Agent 显示/隐藏

新增命令：

- `detect_agents`：列出每个配置的 Agent 的 `installed / running / process_count / path / source`
- `refresh_agent_detection`：清缓存后重新检测

在「诊断信息」抽屉底部新增 **Agent 检测**：

```text
Claude Code   D:\node_global\claude.cmd                 已安装
Codex CLI     D:\node_global\codex.cmd                  已安装
OpenCode      C:\Users\...\AppData\Roaming\npm\opencode.cmd  已安装
Hermes        未找到可执行文件                           未检测到
```

只列出**配置里的 Agent**；未配置的 CLI 不会凭空出现。点「重新检测」立即绕过 60 秒缓存。

## 三、验证

### 自动化

| 层 | 结果 |
| --- | --- |
| Rust | **228 passed**：新增/更新 11 条 process_scan 用例（提示词不匹配、opencode/hermes 幽灵、cmd shim、`.npmrc` prefix、Scoop/Volta、环境变量文件/目录） |
| 浏览器 | **136 passed**：新增「Agent 检测只显示本机找到的」与「+ 检测/添加面板」2 条用例 |
| 视觉 | 24 passed |
| `svelte-check` | 0 errors / 0 warnings |

关键用例：

- `never_turns_prompt_text_into_open_code_or_hermes_processes`
- `accepts_cmd_shims_only_when_the_command_token_is_the_agent`
- `finds_the_npm_prefix_from_the_user_npmrc`
- `includes_scoop_and_volta_prefixes_from_the_environment`
- `finds_agents_from_environment_variables`
- `finds_unknown_agents_referenced_by_environment_directories`

### 真机（CDP）

```json
[
  { "name": "Claude Code", "installed": true,  "path": "D:\\node_global\\claude.cmd" },
  { "name": "Codex CLI",   "installed": true,  "path": "D:\\node_global\\codex.cmd" },
  { "name": "OpenCode",    "installed": true,  "path": "C:\\Users\\LENOVO\\AppData\\Roaming\\npm\\opencode.cmd" },
  { "name": "Hermes",      "installed": false, "path": null }
]
```

岛里的 Agent 条：Claude Code / Codex CLI / OpenCode，与上述三个 `已安装` 完全对应；Hermes 不显示。

截图：`mac-island-analysis/agent-detection-live.png`。

## 四、Agent 条上的「+」：检测本机 + 自己添加

Agent 条末尾新增虚线「+」按钮（`data-testid="agent-add"`），点开：

- **检测本机**：`detect_agent_candidates` 返回配置内 Agent + 内置候选目录里**实际解析到可执行文件**的 Agent；未安装的目录项不会出现。
- 已配置的显示「已添加」，未配置的显示「添加」；点添加会写入 `~/.agent-island/agents.json`（旧文件先备份为 `agents.json.bak`，临时文件 + rename 原子写），随后 `reload_agent_defs` 并刷新快照。
- **如没有想要的，可自己添加**：名称 + 命令/完整路径；后端会先 `resolve_agent_program`，找不到就拒绝（`program_not_found`），不会把未安装的东西加进岛。
- 自定义 Agent 没有日志适配器，只有进程/状态；`resume_args`/`send_args` 留空，避免凭空构造命令。
- 候选目录当前包含 Gemini CLI、Aider、Cursor Agent、GitHub Copilot CLI、Qwen Code、Crush、Goose、Amp、Droid、Kilo Code、Auggie、Plandex、OpenHands、Cline、Warp Agent；只有本机装了的才会出现。

真机：

- 打开面板显示 Claude Code / Codex CLI / OpenCode 三条「已添加」，与诊断检测一致；
- 用 `add_agent_definition("Notepad Test", "notepad")` 验证：写入成功、检测出现 `C:\WINDOWS\system32\notepad.exe`；删除 `agents.json` + `reload_agent_defs` 后恢复为 3 个。
- 截图：`mac-island-analysis/agent-add-panel-live.png`。

## 五、边界

1. **只检测配置里的 Agent**：内置 Claude/Codex/OpenCode/Hermes + `~/.agent-island/agents.json` 自定义项；不在配置里的 CLI 不会显示（这是产品范围，不是漏检）。
2. **桌面 App 不等于 CLI**：如果某个 Agent 只有桌面 App、没有 CLI shim，且不在已知路径里，仍可能检测不到；可在 `agents.json` 里补 `keyword`/路径，或把它加入 `known_program_paths`。
3. **运行中的进程即使可执行文件已删除也显示**：这是「本机正在跑」的事实，不应隐藏；诊断里 `source=process`、`path=null` 可区分。
4. **缓存 60 秒**：装/卸 Agent 后最多等 60 秒，或点「重新检测」。
5. **WSL / 远程机器**不在本机进程扫描范围。
6. **同名进程误判**：按可执行文件名匹配，理论上另一个同名的 `claude.exe` 也会被认作 Claude；这是可接受的边界，诊断路径会暴露实际位置。
