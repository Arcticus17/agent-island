# 性能预算进 CI + 空闲轮询降频

日期：2026-10-21
范围：`src-tauri/src/tests.rs`、`package.json`、`src/app/App.svelte`

## 一、把"扫描性能"变成会失败的测试

新增 `session_scan_meets_the_performance_budget`：在临时目录造 **120 个会话文件**（每个 60 行消息），
用同一个 `SessionIndexCache` 连扫两遍，断言三件事：

| 断言 | 本次实测 |
| --- | --- |
| 冷扫 < 5s（绝对兜底） | **32ms**（另一次 252ms，磁盘冷热差异） |
| 热扫 < 500ms（绝对兜底） | **1.07ms**（另一次 9.6ms） |
| `热扫 × 3 < 冷扫`（相对，抗机器抖动） | 约 **26–30 倍** |
| 热扫 `misses == 0`（缓存真的命中） | 通过 |

**为什么用相对倍数**：绝对耗时在不同 CI 机器上能差十倍，写死阈值只会制造假失败。
相对约束（"缓存必须带来数量级收益"）才是这条测试真正要守的东西；绝对上限只兜"慢到离谱"。

随手加了 `npm run perf`：

```
[perf] cold=32.0423ms warm=1.074ms
```

## 二、空闲轮询降频

之前不管有没有 Agent 在跑，都是固定 1.5s 轮询一次。现在按状态自适应：

```ts
const anyAgentActive = …some(a => a.process === "running" && (working || waiting));
const effectivePollMs = base * (anyAgentActive ? 1 : 3);   // 静止时 4.5s
```

`$effect` 按 `effectivePollMs` 重建定时器，状态一变就立刻切回 1.5s。

**实机效果（同一台机器，静止态 15s 采样）**

| | 单核占用 |
| --- | --- |
| 改前（固定 1.5s） | **9.06%** |
| 改后（静止 4.5s） | **6.88%** |

## 三、测量时踩到的两个坑

1. **HMR 会让旧实例继续轮询**：改完代码没重启就测，得到 **40.5%** 的离谱数字——
   原因是热更新后新旧两份 App 同时挂着定时器。重启后回到 6.88%。以后测 CPU 一定先重启。
2. **`package.json` 又被写进 BOM**：`svelte-check` 直接报 `Unexpected token '﻿'`。
   老规矩——改 JSON 一律用无 BOM 写入。

## 四、剩下的 CPU 花在哪（未解决，留作下一步）

轮询从 1.5s 降到 4.5s 只省了约 1/4，说明**大头不在扫描**。当前最可疑的是
像素吉祥物的 `.resting` 呼吸动画：它常驻运行，持续触发合成帧。
下一步可以：动画降频（3.6s 周期改成 6s）、或在窗口不可见时 `animation-play-state: paused`。

## 门禁

`check` 0/0 · `unit` 167 · `browser` 109 · `visual` 21 · Rust **167**（+1）。
`npm run perf` 可单独跑扫描基准。
