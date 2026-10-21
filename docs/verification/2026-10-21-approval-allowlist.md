# 审批允许清单 + 审计

日期：2026-10-21
范围：`src-tauri/src/approval_rules.rs`（新）、`src-tauri/src/hooks.rs`、`src-tauri/src/lib.rs`、`src/components/approvals/ApprovalRules.svelte`（新）、`src/bridge/*`、测试与视觉基线

## 为什么

内置 `classify_command` 只认极少数安全命令，其余每次都要人工点一次。同一条 `npm test` 一天点十次，
人就会变成无脑点「允许」——这是审批机制最真实的失效方式。允许清单把"我确认过一次"变成"我确认过这一类"。

## 做了什么

| 能力 | 实现 |
| --- | --- |
| 规则 | `{ id, tool, pattern }`，`tool` 空表示任意工具；`pattern` 支持 `*` 通配，大小写不敏感 |
| 命中 | PreToolUse 且内置分类没放行时查规则，命中就直接回 `{"decision":"allow"}`，不弹卡片 |
| 留痕 | 每次自动放行写 `~/.agent-island/approval-audit.jsonl`（时间/工具/命令/命中的规则），只保留最近 200 条 |
| 可见 | 自动放行同时推一条岛内通知「已按规则自动允许：<command>」，不会悄悄发生 |
| 管理 | 展开面板新增「审批规则」抽屉：当前规则列表 + 删除、工具下拉 + 模式输入 + 添加、最近自动放行 |

匹配语义（有单测覆盖）：

```
npm test        → 子串匹配，命中 "npm test -- --watch"
npm *           → 前缀通配，命中 "npm run build"，不命中 "sudo npm run build"
*--dry-run      → 后缀通配
```

## 一个真实踩到的坑

用 PowerShell 的 `Set-Content -Encoding UTF8` 写的规则文件带 **BOM**，`serde_json` 解析失败 →
规则全部静默失效（POST 仍然返回 `pending`）。修法两处：

1. 读规则/设置时先 `trim_start_matches('\u{feff}')`；
2. 加一条单测 `tolerates_a_utf8_bom_and_broken_json`，把"带 BOM 能读、坏 JSON 读成空"钉住。

## 验证

**实机（真发 hook 请求）**

```
规则: [{"id":"rule-live-1","tool":"Bash","pattern":"npm *"}]
POST /event {name:PreToolUse, tool:Bash, command:"npm install left-pad"}
→ 200 {"decision":"allow"}            （修复前是 {"decision":"pending"}）
→ approval-audit.jsonl:
  {"at_ms":…,"id":"a1a15e409a8c","tool":"Bash","command":"npm install left-pad",
   "rule":"rule-live-1","decision":"auto_allow"}
```

重启应用后打开抽屉：`审批规则 1 条 / Bash · npm *`、`最近自动放行 19:37 npm install left-pad` —— 规则与审计都从磁盘读回。
截图：`mac-island-analysis/rules-persisted.png`。

测试用的规则文件已删除（避免替用户做主放行 `npm *`）；审计条目保留，作为这次验证的记录。

**测试**

| 层 | 用例 |
| --- | --- |
| Rust（5 条） | 字面量/前缀/后缀通配匹配、工具过滤、规则往返、审计只保留最新 200 条、BOM 容错 |
| 浏览器（1 条） | 抽屉里添加规则 → 列表出现「任意工具 · npm *」+ 计数「1 条」→ 删除后清空 |

## 门禁

`check` 0/0 · `unit` 138 · `browser` **102** · `visual` **21**（12 张展开态基线因新增抽屉重生成）· Rust **160**。

## 后续

- 规则目前只能手写模式，没有"从审批卡一键记住这条命令"的入口（很好的下一步）；
- 审计只在抽屉里看最近 5 条，没有独立的历史窗口/导出。
