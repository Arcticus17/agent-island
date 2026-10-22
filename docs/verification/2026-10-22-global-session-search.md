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
