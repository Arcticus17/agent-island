# 增量会话索引：把每轮全量重读改成只读变化的文件

日期：2026-10-21
范围：`src-tauri/src/session_scan.rs`、`src-tauri/src/process_scan.rs`、`src-tauri/src/lib.rs`、`src-tauri/src/tests.rs`

## 问题（实测）

岛每 1.5s 轮询一次快照，而 `get_agent_snapshot` 是**全量重扫**：遍历 `~/.claude/projects`、`~/.codex/sessions` 下所有 `.jsonl`，每个文件都重新打开读头尾。

| 指标 | 改前 | 改后 |
| --- | --- | --- |
| 冷启动首个快照 | 5717 ms | 3739–4282 ms |
| 稳态每轮扫描 | ~80 ms | **~41 ms** |
| 其中进程刷新 | 21 ms | 21 ms |
| 其中会话解析 | ~59 ms | **~20 ms** |
| 缓存命中 | — | 稳态 **8/8 全部命中**，0 次重读 |

（用 `AGENT_ISLAND_SCAN_DEBUG=1` 打点测得，同一台机器、同一批会话文件。）

## 做法

新增 `SessionIndexCache`：按 `(路径, mtime_ms, 文件大小)` 缓存两份结果。

| 缓存 | 内容 | 失效条件 |
| --- | --- | --- |
| `index` | `index_session_file` 的身份索引（每个文件都做） | mtime 或大小变化 |
| `detail` | `session_scan_from_file` 的详情解析（活跃 + 最近 3 条历史） | mtime 或大小变化 |

- 每轮 `begin_scan()` 清空"本轮出现过的路径"集合，`finish_scan()` 把消失的文件从缓存里剔除，长期运行不会无限增长；
- 目录遍历和 `stat` 仍然每轮做（这是发现新会话的必要成本），但**不再打开没变化的文件**；
- 未命中计数会区分 index 与 detail 两类，测试里可以直接断言"第二轮 0 次未命中"。

## 保留的调试口子

设 `AGENT_ISLAND_SCAN_DEBUG=1` 启动，stderr 会打出：

```
[scan] files=4 hits=8 misses=0 elapsed=41ms
[scan] processes=21ms agents=41ms
[pin] correcting top=510 left=750
```

排查"为什么 CPU 高"时这三行足够定位到进程刷新 / 会话解析 / 窗口纠偏中的哪一段。

## 测试

`src-tauri/src/tests.rs` 新增 2 条：

1. `incremental_index_reuses_unchanged_session_files` —— 5 个文件冷扫后 `misses>=5`、`hits==0`；同一缓存再扫一轮 `misses==0`、`hits>=5`，候选人数量与 session id 保持一致；
2. `incremental_index_reindexes_a_changed_file_and_prunes_deleted_ones` —— 改一个文件、删一个文件后再扫：被改的文件重新解析并带出新内容，缓存条目从 3 降到 2，候选人也变 2。

Rust 全量：**148 passed**（146 + 2）。

## 还没做的

冷启动那 3.7s 仍在主线程上，首屏数据要等它跑完。下一步打算把首轮扫描挪到后台线程、先渲染"读取中"，或把 detail 的读取预算分档（先读尾部 128KB 出摘要，滚动了再补）。

## 门禁

`cargo test` 148 passed；前端未改动，`check` / `unit` / `browser` / `visual` 见本轮结尾的统一验证。
