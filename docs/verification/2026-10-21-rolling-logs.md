# 滚动日志（`~/.agent-island/logs/`）

日期：2026-10-21
范围：`src-tauri/src/logging.rs`（新）、`src-tauri/src/lib.rs`、`src-tauri/src/hooks.rs`、`src-tauri/src/tests.rs`

## 为什么

出问题时只能靠用户口述："它刚才自己抖了一下"——没有时间线，没法复现，也没法判断是扫描失败、hook 判定还是窗口纠偏。
所以补一份**只记事件、不记内容**的日志。

## 设计

```
~/.agent-island/logs/agent-island.log     当前
                        agent-island.1.log 旧
                        agent-island.2.log 更旧
```

- 单文件上限 **2 MB**，满了先归档再写新的，最多保留 **3 个文件**（当前 + 2 个归档），不会无限长；
- 每行格式：`2026-10-21T13:05:00.996Z [INFO] hook: auto-allowed tool=Bash rule=… program=npm len=65`；
- 时间戳是手写的 UTC 格式化（含毫秒、闰日），不引第三方日期库；
- 写日志失败**绝不影响主流程**（返回值调用方可以忽略）。

## 已接入的事件

| scope | 事件 |
| --- | --- |
| `app` | 启动（带版本号） |
| `hook` | 允许清单命中并自动放行 |

## 隐私：不记完整命令

第一版把 `command=…` 整行写进了日志，随即改掉——命令行里可能有 `--token=…` 这类密钥。
现在只记 **工具 / 命中的规则 / 程序名（命令的第一个词）/ 命令长度**，足够定位"哪类命令被自动放行"，又不会把机密落盘。

## 验证

**实机**

```
2026-10-21T13:00:37.091Z [INFO] app: started version=1.9.0
2026-10-21T13:05:00.996Z [INFO] hook: auto-allowed tool=Bash rule=rule-log-check program=npm len=65
```

第二次特意用 `npm install left-pad --registry=https://secret.example/?token=abc` 触发，
日志里**没有**出现命令原文（只有 `program=npm len=65`）。

**测试（4 条）**

| 用例 | 断言 |
| --- | --- |
| 时间戳 | epoch 0 / 普通时间 / 毫秒 / 闰日（2024-02-29） |
| 追加 | 两行都带 `[LEVEL] scope:` 前缀 |
| 轮换 | 写满 2 MB 后归档 `.1`，当前文件只剩刚写的一行；反复轮换后归档数 ≤ 3 |
| 读取 | 最近的在前、条数受限、文件不存在返回空 |

## 门禁

`check` 0/0 · `unit` 167 · `browser` 109 · `visual` 21 · Rust **166**（+4）。

## 后续

- 还没有 UI：`read_recent_logs` 命令已就绪，可以接进「诊断信息」抽屉或"导出诊断"；
- 只在少数几个点埋了事件（启动、自动放行），终端、更新探测、扫描失败还没记；
- 轮换是"写前检查大小"，极端并发下两个线程同时轮换可能丢一个归档——单进程写入实际不会触发。
