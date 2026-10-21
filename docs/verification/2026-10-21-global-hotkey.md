# 全局快捷键：Ctrl+Shift+A 唤出 / 收起

日期：2026-10-21
范围：`src-tauri/src/hotkey.rs`（新）、`src-tauri/src/lib.rs`、`src/app/App.svelte`、`src/bridge/{types,tauri,mock}.ts`、`tests/browser/island-actions.test.ts`

## 为什么

岛默认是隐藏态，只能靠鼠标蹭到屏幕顶部才出得来；多显示器/全屏写代码时，"召唤"没有键盘入口很难用。

## 实现（不引入新依赖）

用已有的 `windows` crate 直接 `RegisterHotKey`，不装 `tauri-plugin-global-shortcut`：

```rust
RegisterHotKey(None, 1, HOT_KEY_MODIFIERS(MOD_CONTROL|MOD_SHIFT|MOD_NOREPEAT), 'A' as u32)
// hwnd 传 null → WM_HOTKEY 投递到本线程消息队列
loop { GetMessageW(...); if message.message == WM_HOTKEY { app.emit("island-hotkey", ()) } }
```

- 独立线程跑消息循环，不占 Tauri 主线程；
- 注册失败（组合键被别的程序占用）就静默退出，不影响应用本体；
- 组合键解析是 30 行手写代码（`Ctrl/Shift/Alt + 单字母或数字`），带单测；不额外引入解析库。

前端在 App 里订阅：**展开态再按一次直接隐藏，隐藏/胶囊态按一次展开**。

## 验证

**实机**

```
隐藏态 560 × 13  --Ctrl+Shift+A-->  560 × 560（展开）
展开态 560 × 560 --Ctrl+Shift+A-->  560 × 13（隐藏）
```

（用 `keybd_event` 真的发了一次系统级组合键，不是直接调函数。）

**测试**

| 层 | 用例 |
| --- | --- |
| Rust（2 条） | 解析 `Ctrl+Shift+A` / `ctrl+alt+1` / 裸 `A`；拒绝空串、缺键、双主键、`F1` |
| 浏览器（1 条） | 展开态收快捷键 → `data-presence=hidden` 出现热区；再收一次 → 回到 `expanded` |

## 已知限制

- 组合键目前是写死的（`Ctrl+Shift+A`），没有做"自定义快捷键"的设置项；
- 非 Windows 平台是空实现（应用本身目前只发 Windows）。

## 门禁

`check` 0/0 · `unit` 138 · `browser` **100** · `visual` 21 · Rust **154**。
