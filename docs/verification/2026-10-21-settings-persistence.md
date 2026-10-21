# 设置持久化：强杀也不丢（settings.json 原子写）

日期：2026-10-21
范围：`src-tauri/src/settings_store.rs`（新）、`src/settings/persistence.ts`（新）、`src/bootstrap.ts`、`src/bridge/{types,tauri,mock}.ts`、`tests/settings-persistence.test.ts`

## 问题

主题、图标风格、显示模式、隐私、专注模式、布局、窗口位置……全部只在 WebView 的 localStorage 里。
**进程被强杀时最后一次写入可能还没落盘**——本会话实测丢过一次主题（重启回到石墨玻璃）。

## 设计：localStorage 仍是入口，磁盘只做镜像

不把设置搬去磁盘，是因为 localStorage 是同步的、组件里到处在读；改成异步存储要动几十处、
还会让首屏闪默认值。所以：

```
组件 setXxx → localStorage.setItem（照旧，同步生效）
                    │
       每 1.5s 比对一次指纹（不 patch localStorage，也不要求每个 setter 都记得调用）
                    ▼
   bridge.saveAppSettings(json) → Rust 原子写 ~/.agent-island/settings.json
                    │
启动时 bootstrap：先 loadAppSettings() 镜像回 localStorage，再挂载界面
```

- 写入：先写 `settings.json.tmp` 再 `rename`，被杀也只会剩一个旧版本或临时文件，不会留半个 JSON；
- 读取：文件不存在 / 损坏 / 超过 256 KB 一律当成"没有设置"，绝不让应用起不来；
- 首次运行：磁盘没有文件时，把当前 localStorage 里的 `agent-island*` 键整体落盘（迁移）；
- 只同步 `agent-island` 前缀的键，避免把别的东西写进去。

架构上遵守既有约束：**Tauri API 只在 `bridge/tauri.ts` 里 import**（`tests/bridge.test.ts` 会检查），
持久化模块只依赖 `SettingsBridge` 的两个可选方法，测试里可以注入假实现。

## 验证

**实机（强杀 → 重启）**

```
1. 展开岛 → 设置 → 外观 → 终端
2. 等 2s：~/.agent-island/settings.json =
   {"agent-island-window-v1":"{...560x560 at 680,0}","agent-island-theme-style-v2":"terminal"}
3. Stop-Process -Force（不给收尾机会）
4. 重新启动 → 窗口回到 560 宽、主题仍是「终端」（截图 persist-after-kill.png）
```

改动前同样的流程会回到石墨玻璃。

**测试**

| 层 | 用例 |
| --- | --- |
| Rust（4 条） | 落盘往返、二次写入不留临时文件、非法/超大 JSON 被拒且不留文件、缺失/损坏文件读成 None |
| 前端单测（8 条） | 只收集 `agent-island*`、只写变化的键、指纹与键顺序无关、JSON 往返、首次运行迁移、镜像恢复、损坏文件/异常 bridge 不炸、按需落盘 |

## 门禁

`check` 0/0 · `unit` **138** · `browser` 99 · `visual` 21 · Rust **152**。
