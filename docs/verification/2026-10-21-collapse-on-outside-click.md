# 展开态点外面空白处收起

日期：2026-10-21
范围：`src/app/App.svelte`、`src/bridge/{types,tauri,mock}.ts`、`tests/browser/island-actions.test.ts`

## 需求

灵动岛展开时，点击岛外面的空白处（桌面、别的窗口）应该收起。

## 为什么用"窗口失焦"实现

岛窗口本身只有岛那么大，外面的区域根本不属于这个窗口——网页里收不到那里的点击。
要捕获"全局点击"只有两条路：

1. Windows 低级鼠标钩子（`WH_MOUSE_LL`）：能精确知道点在哪，但要引入 Rust 钩子、处理权限和误伤，代价大；
2. **窗口失焦**：用户点别的窗口/桌面时，岛窗口必然失焦。行为上等价，实现只有几行。

选了第 2 种。

## 实现

- bridge 新增可选方法 `onWindowFocusChange(handler)`，Tauri 侧用 `getCurrentWindow().onFocusChanged(...)`；
- App 订阅：失焦且当前是展开态 → 收起。收起走 `collapseToPeek()`（先回胶囊）再 `scheduleIdleHide()`（鼠标本来就在外面，480ms 后自动隐藏），终点是隐藏态，和手动收起的节奏一致；
- **220ms 防抖**：失焦后不立刻动，若期间重新获得焦点就取消（切换窗口、系统弹窗不会误收起）。

## 一个专门验证过的坑

原生 `<select>` 下拉弹层在 Windows 上可能短暂抢焦点。实测：展开面板 → 点设置 → 点「专注模式」下拉 → 弹层打开、岛保持 **420×620 不变**，说明 Chromium 的下拉层不会触发 Tauri 的窗口失焦事件，不需要额外白名单。

## 实机验证

```
start                      420 × 13   （隐藏）
悬停 + 点击胶囊             420 × 620  （展开）
点岛内 Agent 切换条          420 × 620  （不变，不误收起）
点外面空白处（别的窗口）      420 × 13   （收起 → 自动隐藏）
```

## 测试

`tests/browser/island-actions.test.ts` 新增 2 条：

1. `collapses the expanded island when the window loses focus` —— 失焦 → 收起成胶囊；重复失焦无副作用；
2. `keeps the island expanded while the window has focus` —— 获得焦点不收。

## 回归门禁

`check` 0/0 · `unit` 123 · `frontend` 26 · `browser` **95** · `visual` 14。
