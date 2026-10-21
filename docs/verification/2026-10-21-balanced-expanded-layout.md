# 展开面板改成纵横均衡（560 × ≤560）

日期：2026-10-21
范围：`src/app/App.svelte`、`src/bridge/tauri.ts`、`tests/browser/island-readonly.test.ts`

## 需求

展开后 420×620 太瘦长，希望纵横长度均衡一些。

## 做法

| | 之前 | 现在 |
| --- | --- | --- |
| 展开尺寸 | 420 × ≤620（比例 0.68） | **560 × ≤560**（接近正方形） |
| 卡片区 | 420 宽 → 单列，纵向滚很长 | 560 宽 → **两列**，纵向少滚一半 |
| 收起 | 保持原宽度 | 回到展开前的胶囊宽度（420） |

- 新增 `expandedWidth = 560` / `expandedPanelHeight = 560`；展开时若当前宽度更窄就放宽到 560，收起时还原；
- 展开时的宽度请求记在 `restingWidth` 里，收起还原，不会累积漂移；
- 终端放大（720×92%）优先，退出放大后回到展开宽度。

## 两个必须处理的坑

1. **窗口会往右漂**：`resizeWindow` 原来保持左边缘不动，420→560 就整体右移 70px。
   改成保持水平中心（`centeredX = position.x + (旧宽 - 新宽) / 2`），实机验证
   左边缘 750 → 680，中心始终 960。
2. **刚放宽又被缩回去**：`syncWindowSize` 用 `window.innerWidth` 取宽度，而 webview 的
   `innerWidth` 更新比窗口 resize 慢一帧，于是立刻用旧宽度（420）把窗口又缩回去了。
   加 `targetWindowWidth`：调整期间同步高度一律用目标宽度，命中真实宽度后清零，
   700ms 兜底清空（避免一直压着用户手动改的宽度）。

## 实机验证（1920 宽屏，屏幕中心 960）

```
隐藏        420 × 13   center 960
悬停        420 × 60   center 960
点击展开    560 × 560  center 960   ← 纵横均衡，卡片两列
点 ⌄ 收起   420 × 60   center 960
```

截图：`mac-island-analysis/balanced-final.png`。

## 测试

`island-readonly` 的高度契约用例同步更新：展开时窗口宽度改为 `max(当前宽度, 560)`，
高度上限 620 → 560，并断言确实发出过 560 宽的调整。

## 回归门禁

`check` 0/0 · `unit` 123 · `frontend` 26 · `browser` 95 · `visual` 14。
