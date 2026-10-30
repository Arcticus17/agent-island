# 发现驱动的 Agent 列表（2026-10-30）

用户提出的模型：**岛内至少要显示电脑里已有的 Agent，且从环境变量（PATH）检测；下载了没跑过也算已有。**
同时明确：不预置"内置 Agent"，内置只允许存在于"可一键安装的清单"。

## 旧模型的结构性问题

- `agent_defs()` 只有 4 个内置 + `agents.json`，快照的 Agent 列表完全由配置决定；
- `detect_agent_candidates()`（16 项人工清单）只服务「+」面板，命中也不参与快照；
- 结论：装在 PATH 上但没写进配置的 CLI（例如 Pi）永远不会进岛。

## 新模型

```
scan_agent_defs() = 显式配置（内置 4 + agents.json 自定义）
                  + 本机探测到的候选（agent_catalog × resolve_agent_program）
```

- 自动发现的项 `source = "detected"`，**只存在于内存**，不写回 `agents.json`：
  卸载后自然消失，也不会替用户把他的机器"配置化"。
- 每个 Agent 带 `source`（`builtin` / `custom` / `detected`），随快照下发。
- 岛内默认显示：`source != "detected"`，或运行中，或已固定，或最近 7 天用过；
  其余留在「+」面板的已安装列表里，点「添加」即固定为 `custom`（写 `agents.json`）。
- 扫描仍用 `program_available` 兜底：装了又卸载的 CLI 不会留在岛上。

## 真机验证（本机 PATH 上有 claude / codex / pi）

把 `~/.agent-island/agents.json` 临时移走后运行扫描：

```text
DEF name=Claude Code source=builtin program=claude
DEF name=Codex CLI   source=builtin program=codex
DEF name=OpenCode    source=builtin program=opencode
DEF name=Hermes      source=builtin program=hermes
DEF name=Pi          source=detected program=pi
```

`Pi` 之前必须手动"添加"才会出现；现在只要 `~/.pi/agent/bin` 在 PATH 上就会被发现。
恢复配置后 Pi 变回 `source=custom`（用户显式添加优先于自动发现），已验证。

## 验证

| 层 | 结果 |
| --- | --- |
| `cargo test --lib` | 232 passed |
| `npm run test:browser` | 142 passed：新增"停止的自动发现 Agent 不进岛""运行后进岛""固定后进岛" |
| `npm run test:unit` / `test:visual` | 188 / 16 passed |
| `svelte-check` | 0 errors / 0 warnings |

## 终端：每个岛内 Agent 都启动自己的命令

- 快照新增 `program`（def.program()：内置为 claude/codex/…，自定义为配置里的命令或完整路径）。
- `terminalTarget()` 的优先级：内置映射 → 快照 `program` → `powershell`。
- 后端 `terminal_program_allowed` 改用 `scan_agent_defs()`，自动发现的 keyword（如 `pi`）也放行。
- 终端抬头显示 Agent 名（Pi），不再显示 `pi` 这种命令名。

## 边界

- 探测范围仍是**签名表**（`agent_catalog.rs`，目前 17 项）+ PATH/已知目录；完全陌生的 CLI 需要手动添加或后续加签名。
- 自动发现的 Agent 没有日志适配器时只有进程状态（`has_log_adapter=false`），冷启动/无适配器文案已在前一轮覆盖。
- 「+」面板的候选现在会同时出现"自动发现（本机已安装）"与"已在岛内配置"两类，用 `configured` 区分。
- 一键安装/更新（P1，npm 全局）尚未实现；本轮只完成发现与展示。
