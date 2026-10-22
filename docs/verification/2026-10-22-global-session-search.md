# 跨会话全文搜索

日期：2026-10-22
范围：`src-tauri/src/session_search.rs`（新）、`src/components/search/GlobalSearch.svelte`（新）、`src/app/App.svelte`、`src/bridge/*`、测试

## 做了什么

面板里新增「全局搜索」抽屉：输入 ≥2 个字符，在**本地所有会话文件**里找消息，命中后显示
`agent · role · 时间` + 片段，点一条能在资源管理器里定位到那个 `.jsonl`。

```
全局搜索  1 条
[npm                                            ]
共 1 条（最多显示 20 条）
claude · Assistant · 08-18 19:10
…发布形式**（在 "dynamic-island/ 目录下）：```bash npm install npm run tauri dev``` · 打包与测试：…
```

## 后端实现（`session_search.rs`）

- 枚举 `~/.claude/projects` 与 `~/.codex/sessions`，用已有的 `list_newest_files` 取**最新 40 个**会话文件；
- 每个文件只读**尾部 512 KB**，交给对应 adapter 的解析器（复用 `CodexAdapter` / `ClaudeAdapter`），
  因此不需要为搜索再实现一遍各家格式；
- 只搜 User / Assistant 消息，**不搜工具输出**（噪音大且容易命中无关内容）；
- 大小写不敏感的字节匹配 + 单行片段（命中处两侧各留 40 字符，多字节安全）；
- 凑够 `limit`（默认 30）就停，不做全量扫描。

`reveal_session(path)`：只允许定位**已知识别目录下的 `.jsonl`**（`~/.claude/projects`、codex sessions），
其它路径一律拒绝（有单测），用 `explorer.exe /select,<path>` 定位文件。

## 为什么不做倒排索引

文件量在几十个量级、单文件几百 KB，按需读尾部的成本远低于维护索引的复杂度；
索引还会引入"索引与磁盘不一致"的新问题。等真到了几百个会话再考虑。

## 验证

**实机**：在真实数据上搜 `npm`，1 条命中，片段正确、时间与角色都对（截图 `mac-island-analysis/search-results2.png`）。
（输入法干扰：中文输入法是组合态，最后改用剪贴板粘贴才顺利输入——这不是应用的问题。）

| 层 | 用例 |
| --- | --- |
| Rust（5 条） | 命中消息与片段、大小写不敏感且尊重 limit、空查询/单字符不扫、多字节片段安全、定位白名单 |
| 浏览器（1 条） | 展开抽屉 → 输入 npm → 2 条结果与计数 → 点击结果触发 `revealSession` 且路径正确 |

## 门禁

`check` 0/0 · `unit` 179 · `browser` **113** · `visual` 24（15 张因新增抽屉重生成）· Rust **174**（+5）。

## 后续

- 结果点击目前只是"定位文件"，没有做到"直接跳到该会话记录"（需要后端支持按 session id 加载任意历史会话）；
- 搜索只覆盖 claude / codex；opencode、hermes 的存储格式不同，尚未接入；
- 没有做高亮（片段已经是命中上下文，够用）。
## 八、点搜索结果直接打开会话（追加）

原来点结果只能"在资源管理器里定位文件"，看不到内容。现在改成两级操作：

| 操作 | 行为 |
| --- | --- |
| 点结果行 | **打开该会话**：加载记录、只读展示（日志卡片 + 状态卡片显示「本地记录」），并禁止向它发指令 |
| 行尾 `⋯` | 在资源管理器里定位 `.jsonl`（保留原来的能力） |
| 横幅「返回当前会话」 | 退出搜索结果，回到桌面端当前会话 |

后端新增 `load_session_by_path(path)`：复用 `session_scan_from_file` 解析，只允许**已知识别目录下的 `.jsonl`**，
返回 `SessionView`（含 records）。界面用一个独立的 `searchedSession` 状态作为会话来源优先级最高的一档，
不动快照与历史列表——搜索到的会话可能根本不在当前 Agent 的 history 里。

**实机验证**：搜索 `npm` → 点结果 → 横幅显示「正在查看搜索结果（claude · 这是一个灵动岛的想法）」，
状态卡片变「本地记录」，日志卡片显示命中消息原文；输入框被禁用并提示不能向历史会话发指令。
截图：`mac-island-analysis/jump-open.png`、`jump-open-records.png`。

**测试**：Rust 2 条（越权路径拒绝、真实形状文件加载出 records）；浏览器用例扩展为
「点结果 → 打开会话 + 内容出现 + 输入禁用 → `⋯` 定位文件 → 返回当前会话」。
