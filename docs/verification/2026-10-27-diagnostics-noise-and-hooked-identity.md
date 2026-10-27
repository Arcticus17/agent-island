# 诊断噪音与 Agent 自报身份（2026-10-27）

## 一、问题：Claude 永远显示「11 项提示」

修 S3 时注意到 Claude 的诊断一直挂着十几条提示。诊断本该是"真出问题了"的信号，
一旦长期有噪音，用户就学会忽略它——比没有诊断更糟。

用真实目录离线扫描（`~/.claude/projects`，3 个会话文件）复现，得到 **9 条**记录：

| code | 数量 | 含义 |
| --- | --- | --- |
| `acquisition_incomplete` | 6 | 采集不完整（无 session/无路径，看不出是哪个文件） |
| `parse_failed skipped=3` | 2 | 整个文件"没有一条能解析"，3 行被跳过 |
| `partial_parse skipped=7` | 1 | 部分行无法解析 |

## 二、三个根因（都是真 bug，不只是噪音）

### 1. `cwd` 在第一行就停住了

`session_scan` 解析会话元信息时，找到第一条带 `sessionId` 的行就返回。而 Claude 的
`mode` 行带 `sessionId` 却没有 `cwd`：

```json
{"type":"mode","sessionId":"4c4cb04d-…"}
{"type":"file-history-snapshot","sessionId":"4c4cb04d-…"}
{"type":"user",…,"cwd":"C:\\Users\\LENOVO",…}
```

结果：`cwd = None` → 每个文件刷一条 `acquisition_incomplete`，**而且会话无法与进程对上**
（这正是「当前会话未确认」的一个直接来源）。

修复：`claude_metadata()` 分别收集 id 和 cwd，两者都拿到才停；索引路径和详情路径共用它。

### 2. 斜杠命令被当成"解析失败"

Claude 把 `/exit` 这类命令、命令输出、上下文提示写成**字符串 content**：

```json
{"type":"user","isMeta":true,"message":{"role":"user","content":"<local-command-caveat>…"}}
{"type":"user","message":{"role":"user","content":"<command-name>/exit</command-name>"}}
{"type":"user","message":{"role":"user","content":"<local-command-stdout>See ya!</local-command-stdout>"}}
```

旧解析器只接受数组 content，于是这些行全部 `skipped_lines += 1`；
一个"启动后立刻 /exit"的会话就变成"整个文件解析失败"。

修复：
- 字符串 content 且命中命令信封前缀（`<command-name>`/`<local-command-stdout>` 等）→ **正常数据，跳过不计数**；
- `isMeta: true` 的行（注入的上下文）→ 同上；
- 其余字符串 content 当纯文本用户消息解析（新版 Claude 也会这么写）。

### 3. 无消息会话在详情阶段丢 id

详情解析（`session_scan_from_file`）过去对 claude 传 `metadata_id = None`，只依赖消息事件里的
session id。没有消息的会话就 fallback 到**文件名**，与索引阶段的 id 不一致，
触发 `identity_replaced` → 又一条 `acquisition_incomplete`。

修复：详情路径也用 `claude_metadata()` 取 id/cwd，索引与详情对同一个文件得到同一个身份。

## 三、结果

同一份真实数据，修前 **9 条诊断**，修后 **0 条**，且三个会话都拿到了 `cwd`：

```
candidates=3 diagnostics=0
candidate session=4c4cb04d-… cwd=Some("C:\\Users\\LENOVO")
candidate session=a29d1bc4-… cwd=Some("C:\\Users\\LENOVO")
candidate session=610ea405-… cwd=Some("C:\\Users\\LENOVO")
```

界面上 Claude 的诊断从「11 项提示」变成「无异常」。

**测试**（4 条新增，全在 Rust）
- `log_noise_tests::command_envelopes_and_meta_lines_are_not_parse_failures`
- `log_noise_tests::plain_text_user_message_is_parsed`
- `claude_session_finds_cwd_even_when_the_first_session_line_lacks_it`
- `claude_command_only_session_produces_no_diagnostics`

## 四、S4.1：Agent 自报身份（hooks → 「已确认」）

`Inferred`（猜）之外，本来设计里有 `Hooked`（Agent 自报），但一直是死代码。
Claude 的 hook 每次都会带 `session_id` 和 `cwd`——这是唯一能**直接指出"哪个会话"**的证据。

实现：

| 位置 | 内容 |
| --- | --- |
| `hooked.rs`（新） | `HookedSession`、`record_hooked`、`fresh_hooked`（10 分钟有效期 + 时钟偏移容忍） |
| `hooks.rs` | 任何 hook 事件都记录 `(claude, session_id, cwd, now)`，不论审批开关 |
| `process_scan` | 有有效自报且候选中存在该 session → 活动会话固定为它，`binding = hooked` |
| `launcher::choose_binding` | 优先级：自报 > 岛内启动 > 手动绑定 > 推断 |

**真机验证**（向本机 hook 服务 POST 一条 `Stop` 事件，session 用真实会话 id）：

```json
{"presence":"expanded","binding":"hooked",
 "sessionCard":"610ea405-42a9-4b3a-af52-3d44beaad9a5","diagnostics":"无异常"}
```

头部徽标显示 `hooks`，会话卡片直接是自报的那个 session——**身份由 Agent 说出，不再靠猜**。

**测试**（5 条新增）
- `hooked::tests`：有效期、空 session 不覆盖、未来时钟容忍
- `launcher::binding_priority_tests`：自报 > 启动 > 手动 > 推断

## 五、还差什么（S4.2 / S4.3）

`codex exec --json` 已确认存在（"Print events to stdout as JSONL"，codex-cli 0.163.0），
这是 Codex 的显式事件通道。要把它变成可用功能还差两步：

1. **受控运行**：岛内"任务模式"用 `codex exec --json <prompt>` 起一个子进程，解析 JSONL
   （`thread.started` 给 session id → 直接判定 `hooked`；`turn.*`/`item.*` 给轮次事件）。
2. **交互式 TUI 的事件**（用户日常用的模式）需要订阅 `codex app-server` 的 JSON-RPC，
   属于另一块工作量，不能靠读日志伪装成"已确认"。

OpenCode 本机没有安装（`opencode` 不在 PATH），无法验证它的 server/SSE 事件，
所以本轮没有为它写任何"看起来能用"的代码——没验证过的集成比没有更危险。
## 六、S4.2 进度：Codex 显式事件的解析层（已落地，UI 待接）

用真实运行抓到了 `codex exec --json` 的事件流（本机网络到不了 Codex 后端，所以任务本身失败，
但事件格式是真实的），存成 `src-tauri/tests/fixtures/codex/exec-json.jsonl`：

```text
{"type":"thread.started","thread_id":"01a17db0-…"}
{"type":"turn.started"}
{"type":"item.completed","item":{…}}
{"type":"error","message":"…"}
{"type":"turn.failed","error":{"message":"…"}}
```

新增 `agent_tasks.rs`：
- `parse_codex_event()`：只认上述事件；stderr 的 tracing 行、未知类型一律忽略（CLI 升级不至于把任务卡死）；
- `next_state()`：事件 + 进程是否退出 → 状态机；**进程退出本身也算结束**，避免"永远运行中"；
- `start_agent_task` / `list_agent_tasks` / `cancel_agent_task` 三个命令：
  起 `codex exec --json --skip-git-repo-check`，逐行解析，`thread.started` 的 thread id 直接
  当成权威 session id 写进 `hooked`，事件通过 `agent-task` 推给前端。

**测试**：`agent_tasks::tests` 3 条（真实事件流解析、忽略非 JSON/未知类型、状态机含退出兜底）。

**还差**：前端没有接「任务」入口（下一步：把发送框对"停止状态的 Codex"启用，走这条链路）。
另外本机 Codex 后端不可达（`chatgpt.com/backend-api` 超时），所以真实任务的成功路径无法在本机验证——
这一点必须如实记录，不能拿"解析器通过"冒充"任务能跑通"。
