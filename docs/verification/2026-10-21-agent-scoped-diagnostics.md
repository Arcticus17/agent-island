# 诊断信息按 Agent 隔离（不混显示）

日期：2026-10-21
范围：`src/app/App.svelte`（诊断过滤）、`tests/browser/island-actions.test.ts`

## 问题

在某个 Agent 的页面，诊断抽屉显示的是**所有 Agent 的汇总**：Claude 页和 Codex 页都写着"13 项提示"，
点开里面混着别的 adapter 的问题。用户看到的感受就是"这个页在显示别的 Agent 的数据"。

后端 `get_diagnostics` 返回的本来就是全量（诊断是系统级能力），混合是**前端展示层**没做归属过滤。

## 修法

按当前选中的 Agent 过滤，而不是按"页面"过滤（同一个 Agent 的所有会话仍然共享它的诊断）：

```ts
const selectedAdapter = $derived.by(() => {
  const fromDiagnostic = selectedAgent?.diagnostic?.adapter;   // 快照里每个 Agent 自带 adapter
  if (fromDiagnostic) return fromDiagnostic.toLowerCase();
  const name = (selectedAgent?.name ?? "").toLowerCase();
  return adapterForName[name] ?? name;                          // 快照缺 diagnostic 时按名字兜底
});
const agentDiagnosticIssues = $derived(
  diagnosticIssues.filter((issue) => issue.adapter.toLowerCase() === selectedAdapter),
);
const agentDiagnosticSources = $derived(
  diagnosticSources.filter((source) => source.source.toLowerCase().startsWith(selectedAdapter)),
);
```

抽屉徽标、面板内容、用量来源列表全部改用过滤后的数据。

## 实机验证

| 页面 | 修复前 | 修复后 |
| --- | --- | --- |
| Claude Code | 13 项提示（含 Codex/OpenCode） | **11 项提示**（只有 Claude） |
| Codex CLI | 13 项提示（同上） | **无异常** |

截图：`mac-island-analysis/diag-claude.png`、`diag-codex.png`。
顺手确认了"选中 Codex → 收起 → 再展开"仍然是 Codex，不会跳回别的 Agent。

## 测试

- 新增 `agent-scoped diagnostics`：两个 adapter 的 issue 与两个用量来源，切到 Claude 后只显示 `claude_issue`、
  不出现 `codex_issue` 和 "Codex 用量"，徽标为 "1 项提示"；
- 原有"stop 期间诊断挂起"用例里，post-stop 的 issue 改成 `claude`（当前选中的 Agent），
  `codex` 那条继续用来断言"不显示"。

## 回归门禁

`check` 0/0 · `unit` 130 · `frontend` 26 · `browser` **99** · `visual` 21。

## 仍然全局的部分（有意）

- 审批卡 / 通知：它们是**跨 Agent 的告警**，必须能打断当前页，否则核心功能失效；
- 总览窗口：本来就是"所有会话"视图；
- 内置终端：是工具不是数据，切换 Agent 不会杀掉正在跑的 CLI（会话归属 App，见终端那篇文档）。
