# 移除正文关键词状态判定

日期：2026-09-30
分支：`codex/agent-island-reliability`
范围：`src-tauri/src/lib.rs`

## 做了什么

- 删除四段已无调用点的文本判定代码（共 317 行）：`text_signal`、`claude_snapshot`、`codex_snapshot`、`opencode_snapshot_from_text`。
- `scan_agents` 的兼容 DTO 不再用 `log_status` 推导 `status`；改为只依据进程事实的 `legacy_process_status`（工作中 → `working`，高负载 → `high_load`，其余 → `idle`）。`display_status` 占位值与该结果同源，不再写死 `Idle`。
- 新增测试 `compatibility_status_placeholder_uses_process_facts_only`，用函数签名（不接受任何文本入参）钉住"文本不得决定状态"这条不变量。

## 为什么

- 结构性真相源早已是快照：默认前端读 `get_agent_snapshot` → `build_snapshot_with_acquisition` → `derive_display_status`。正文关键词只允许产生诊断提示（`adapters/opencode.rs` 只发 `DiagnosticHint` + `Confidence::Unknown`，并有测试断言文本不能结束一个 turn）。
- 上述四段函数已无任何调用点，且生产路径构造的 `LogSnapshot.log_status` 恒为 `None`：它们既不起作用，又会误导后来者以为文本判定仍在生效链路上。
- 风险在于可复活：一旦有人给它们接上调用点，陈旧日志里的 "failed to" 一类措辞会重新变成状态。

## 证据

- `cargo test --manifest-path src-tauri/Cargo.toml --lib` → 128 passed / 0 failed（改动前 127；新增 1 条即本次回归测试）。
- `npm run test:unit` → 123 passed；`npm run test:frontend` → 26 passed；`svelte-check` → 0 errors / 0 warnings。
- `cargo build --lib` → 无新增警告。
- 检索 `text_signal|检测到报错|log_status.as_deref|fn claude_snapshot|fn codex_snapshot` 在 `src-tauri/src/lib.rs` 中已无匹配。

## 选型缺口（本轮只确认，未改动）

- 无 `tauri-plugin-single-instance`，代码内也未发现 `CreateMutex` 一类互斥：常驻置顶应用被启动两次时的行为未定义。
- 无 `tauri-plugin-updater`（`main` 与分支都没有）：用户升级只能手动重新安装。
- 无通知插件：`ROADMAP.md` 在 v0.5 记为已实现、在 v1.1 明确记录已按"鸡肋即砍"移除，`Cargo.toml` 与"已移除"一致——这是时间线记录，不是文档漂移。
- 开机自启仍通过 spawn `reg.exe` 读写 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`。

## 未验证

- 原生窗口层级、虚化、真实目录与终端未做人工验收。
- 视觉回归在本轮稍后已跑通（见 `2026-09-30-visual-baseline-refresh.md`），本次改动未单独比对截图。
