# 根级改法：从「观测 Agent」到「岛自己拉起 Agent」

日期：2026-10-22
决策人：用户（选择 A）

## 一、为什么这是根

当前所有不可靠都来自同一处：**岛与 Agent 之间没有契约，只有观测**。

| 症状（本会话都真实出现过） | 根源 |
| --- | --- |
| 「当前会话未确认」「自动：可靠匹配的当前会话」 | 会话身份靠 `进程 cwd + 文件 mtime + confidence 启发式` 猜 |
| 审批归属要按 session 反查、usage 归属可能错位 | 同上 |
| 1.5–4.5s 才更新一次状态 | 没有事件通道，只能轮询 |
| 「固定 Agent」这个功能本身 | 是对"被自动切走"的补丁 |
| Codex/OpenCode/Hermes 无法审批 | 只有 Claude 有 hooks 通道 |

结论：**先拿到权威的 (pid → agent → session → cwd) 绑定**，其余（审批、用量、搜索跳转、状态延迟）都会随之变可靠；
拿不到绑定的部分则如实标注"推测"，不再假装确定。

## 二、契约：三种绑定，一种事实

```
Launched  岛启动的（PTY 后代进程）   ← 权威：我们看到了它出生
Hooked    Agent 通过 hooks 自报      ← 权威：agent 自己说它是谁（Claude）
Inferred  日志/进程推断              ← 不确定，UI 必须显式标注
```

规则：

1. **Launched 优先**：`claude`/`codex`/`opencode` 进程若是我们某个 PTY 会话的后代，绑定为 `Launched`。
2. **Hooked 次之**：Claude hooks 带来的 `session_id` 与该进程 cwd/会话文件一致时，绑定为 `Hooked`。
3. 其余保持 `Inferred`；UI 不再用"自动匹配"这种模糊说法，而是显示 `推测` + 允许一键绑定/切换。

## 三、为什么 "PTY 后代" 是最小可用的权威信号

岛已经内置终端（`portable-pty`）。PTY 子进程的 pid 我们是知道的，而 Agent 是它的**后代**。
从 Agent 进程沿 `parent()` 向上走，若命中某个 PTY 根 pid → 该 Agent 一定是"我们拉起/在我们终端里跑的"。
这条链不依赖任何启发式，也不依赖 Agent 配合，且**用户手动在岛内终端敲 `claude` 也算数**——覆盖了最常用的路径。

代价：岛外启动的 Agent 仍是 `Inferred`。这部分留给下一阶段（hooks / 显式绑定），不假装解决。

## 四、分片计划

| 片 | 内容 | 验收 |
| --- | --- | --- |
| **S1（本次）** | 采进程父子关系 → 判定 `Launched`；快照新增 `binding` 字段；UI 显示「已确认 / 推测」 | 单测覆盖祖先链（含环、深度上限）；实机用岛内终端验证 |
| S2 | 内建终端面板显示"这个终端里的 Agent 已绑定"，并提供「在此终端启动 Agent」按钮 | 启动后 1 个轮询周期内出现「已确认」 |
| S3 | 外部启动的 Agent：显示「推测」+ 一键绑定到某个会话（绑定结果持久化） | 绑定后跨重启仍生效 |
| S4 | Codex/OpenCode 的显式事件（JSON 输出 / server 模式），把更多 Agent 提升为 `Hooked` | 每个适配器至少有 1 条契约测试 |

## 五、兼容与回退

- `binding` 是**新增可选字段**，老快照/老前端不会崩（serde 默认 `Inferred`）；
- 判定错误时最坏结果 = 显示「已确认」但会话选错，因此 S1 只把 `Launched` 用于**提升置信度与提示**，
  不改变现有的会话选择算法——先证明信号准确，再让它参与决策。

## 六、本片不做什么

- 不改审批流程、不改会话选择策略、不做新 UI 面板；
- 不引入新依赖；
- 不在岛内自动启动 Agent（那是 S2 的产品决策）。
## 七、S1 落地记录（2026-10-22）

已实现：

- `terminal.rs`：PTY 会话记录 shell 的 pid；
- `launcher.rs`：`parent_map` / `descends_from` / `binding_for_pids`，纯函数、可单测（覆盖环、自环、父进程已退出、深度上限）；
- `process_scan.rs`：每次扫描从 sysinfo 构一次 pid→父 pid 表，Agent 的进程若经岛内 PTY 即标记 `launched`；
- 快照新增 `binding` 字段（`launched|hooked|inferred`，serde 默认 `inferred`，老数据不炸）；
- 会话卡片：`已确认`（launched/hooked，主题色）vs `推测`（灰色），带 tooltip 解释来源。

**已验证**

| 项 | 证据 |
| --- | --- |
| 祖先链判定 | Rust 5 条单测：直接/深层后代、根自身、环、父缺失、自环、空集合、最强绑定选择 |
| 默认不误判 | 没有 PTY 时一律 `Inferred`（单测 + 浏览器用例：无绑定时显示「推测」） |
| 界面展示 | 浏览器用例：`binding: "launched"` → 「已确认」、`inferred` → 「推测」 |
| 全量回归 | Rust 182、unit 181、browser 116、visual 24、check 0/0 |

**正向验证（已拿到证据，2026-10-22 19:1x）**

用户在岛内终端启动 `claude` 后，进程树证实了这条链：

```
35456 dynamic-island.exe
 └─ 26880 cmd.exe /C claude          ← PTY 的 shell
     └─ 3176 claude.exe …            ← Agent，PTY 的后代
```

界面上：选中 Claude Code 后，头部出现紫色徽标 **「岛内启动」**；选中 Codex CLI（不是岛内启动）时**没有**徽标。
截图：`mac-island-analysis/binding-claude.png` 与 `binding-header2.png`。

这说明判定链（sysinfo 父子表 → 祖先遍历 → 绑定标记 → UI）在真机上是通的。

**一个仍然存在的现象**：此时"会话"仍未确认（头部显示「当前会话未确认」）。原因不是绑定失败，而是
claude 停在模型选择界面、**还没有产生会话文件**——没有会话就没有会话卡片可显示。
这正是 S2 要解决的问题：把"我启动了这个 Agent"这个事实本身展示出来（现已先落在头部徽标上）。

