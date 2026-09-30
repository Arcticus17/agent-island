# 单实例守护

日期：2026-09-30
分支：`codex/agent-island-reliability`
范围：`src-tauri/src/single_instance.rs`、`src-tauri/src/lib.rs`、`src-tauri/Cargo.toml`

## 做了什么

- 新增命名互斥体守护：`run()` 在创建窗口、托盘、轮询与 hook 服务之前先取 `Local\AgentIsland.SingleInstance`；已被占用时尝试把已在运行的窗口唤到前台，然后直接返回，进程退出。
- `Local\` 前缀把名字限定在当前登录会话，因此同一台机器上的两个登录用户可以各跑一个岛。
- 守护对象在事件循环期间一直存活，进程退出时由系统释放（`panic = "abort"` 下 Drop 不执行也不影响）。

## 为什么

- hook 服务固定绑定 `127.0.0.1:8799`，且绑定失败只打印一行错误后返回（`start_hook_server`）。重复实例因此会：显示第二个常驻置顶窗口、跑第二份轮询、写同一批统计文件，而且永远收不到 hook 事件——一个"看得见但聋"的岛。
- 之前没有任何单实例处理：既没有该插件，也没有互斥体，重复启动的行为未定义。

## 实现取舍

- 没有引入新依赖，也没有使用 `tauri-plugin-single-instance`：仓库已依赖 `windows` crate，直接调用 `CreateMutexW` 即可，符合本项目"依赖极简"的取向。
- `Cargo.toml` 为既有依赖增加了一个 feature：`Win32_Security`（`CreateMutexW` 的签名引用 `SECURITY_ATTRIBUTES`，因此被该 feature 门控）。没有新增 crate。
- 唤前是尽力而为：`FindWindowW` 按 `tauri.conf.json` 里 `main` 窗口的标题 `Agent Island` 查找，随后 `ShowWindow(SW_SHOW)` + `SetForegroundWindow`。标题字符串在源码里是字面量（`w!` 宏只接受字面量），所以两处必须人工保持一致，已在注释中写明。

## 验证

- 单元测试 2 条通过：同名单第二次获取返回"已在运行"、释放后可再次获取；不同名字互不阻塞（`cargo test --lib single_instance` → 2 passed）。
- 端到端双启动实测（debug 二进制，今日重建，2026-09-30 12:22）：
  - 第一个进程 5 秒后仍存活；
  - 第二个进程 5 秒内自行退出，退出码 0，第一个进程继续存活；
  - 第三个进程同样自行退出；
  - 期间名为 `dynamic-island` 的进程数始终为 1，清理后为 0。
- `cargo test --lib` → 130 passed / 0 failed（本次之前为 128）。
- `cargo build`（含二进制）与 `cargo build --lib` 均无新增警告。

## 未验证

- 唤前是否真的置顶未被观测确认：`SetForegroundWindow` 可能被系统拒绝（既有代码对同类调用也是失败即报错、不假装成功）。
- 多登录会话（`Local\` 作用域）未实测。
- 托盘、原生窗口层级与虚化、安装/卸载验收仍未做。
- 视觉回归在本轮稍后已跑通（见 `2026-09-30-visual-baseline-refresh.md`），本次改动未单独比对截图。
