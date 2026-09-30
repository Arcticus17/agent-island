# 单个线程失败不再让整个岛消失

日期：2026-09-30
分支：`codex/agent-island-reliability`
范围：`src-tauri/Cargo.toml`、`src-tauri/src/lib.rs`

## 做了什么

- `[profile.release]` 移除 `panic = "abort"`，并写明理由，避免以后被当作"优化"加回来。
- 新增 `LockRecover`：10 处 `.lock().unwrap()` 全部改为 `.lock_recover()`，锁中毒时取回内部值继续用，而不是把 panic 扩散出去。
- `fallback_send_command` 的第二个 `_ => unreachable!()` 改为返回明确错误：该函数依赖两个 `match name` 保持同步，一旦有人只给第一个加了分支，`unreachable!()` 会直接终止进程。
- 新增 2 条测试：锁在持有者 panic 后仍可恢复；发消息回退路径接受 4 个已知 Agent 并拒绝未知名字。

## 为什么

- 这个岛是常驻的通知与审批界面，同时运行 6 个线程（每个 hook 连接一个、日志扫描、Win32 贴顶线程等）。`panic = "abort"` 下任意一个线程 panic 就是整个应用消失——用户不会看到崩溃提示，只会发现岛不见了。
- 测试 profile 本来就是 unwind，所以 release 的 abort 语义在测试里永远暴露不出来：`catch_unwind` 类测试也写不了。
- 岛上还有一处同类脆弱点：两处按 Agent 名字分支的 `match` 必须人工保持同步，第二处的兜底写成了 `unreachable!()`。

## 取舍与边界

- 代价是二进制略大、不再 fail-fast；收益是单个后台线程失败被限制在它自己身上。
- 跨 FFI 的 panic 仍然会终止进程：Rust 2021 下 unwind 不允许穿过 `extern "C"`，Tauri command 与 Win32 回调都属于这类边界。因此这次改动只覆盖"我们自己线程内部的 panic"，不改变也不会引入未定义行为。
- 没有引入任何新依赖。

## 验证

- `cargo test --lib` → 132 passed / 0 failed（本次之前 130）。
- 定向测试通过：`tests::locks_recover_after_a_panicking_holder`、`tests::fallback_send_command_accepts_known_agents_and_rejects_unknown_ones`。
- 改写由带断言的脚本执行：改写前 `.lock().unwrap()` 恰为 10 处，改写后为 0 处。
- `cargo build --lib` 无新增警告。

## 未验证

- 未构建 release 安装包（`lto` + `codegen-units = 1` 耗时长），本次改动不涉及编译期行为，但 release 产物本身仍未验证。
- 未做真实 panic 注入的端到端演练（例如让某个 hook 连接线程 panic 后确认岛仍存活并继续轮询），目前只有单元层面的中毒恢复证明。
- 视觉回归在本轮稍后已跑通（见 `2026-09-30-visual-baseline-refresh.md`），本次改动未单独比对截图。
