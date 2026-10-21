# 像素吉祥物：Agent 没在跑时也有动画

日期：2026-10-21
范围：`src/components/brand/PixelAgent.svelte`、`src/components/island/CompactIsland.svelte`、`tests/browser/island-actions.test.ts`

## 需求

Agent 没有运行时（已停止 / 空闲 / 已完成 / 报错），像素吉祥物也要有动画，不要像张静止贴图。

## 做法

`PixelAgent` 新增 `status` 入参，按状态决定是否加 `.resting`：

| 状态 | 表现 |
| --- | --- |
| working | 不动（紧凑条里已经在滚日志，别再加动效） |
| waiting | 沿用原来的 `attention-bob` 上下浮动 |
| stopped / idle / done / error | 新增 `.resting`：3.6s 一轮「轻微上浮 + 眨眼压缩」 |

关键帧（比例按 20px 精灵调的，位移不到 1px 所以不抢眼）：

```css
@keyframes mascot-rest {
  0%, 100% { transform: translateY(0) scaleY(1); }
  38%      { transform: translateY(-0.8px) scaleY(1.05); }  /* 吸气上浮 */
  62%      { transform: translateY(0) scaleY(1); }
  70%      { transform: translateY(0) scaleY(0.84); }       /* 眨眼压缩 */
  76%      { transform: translateY(0) scaleY(1.02); }
  84%, 100%{ transform: translateY(0) scaleY(1); }
}
```

- `transform-origin: 50% 92%`（脚底），压缩时是"往地面蹲"，不会飘起来；
- 尊重 `prefers-reduced-motion: reduce` → 关闭动画（视觉回归测试正是用这个设置跑的，所以 14 个基线不受影响）。

## 验证

实机（Codex CLI 已停止）连拍 6 帧，间隔 300ms：

```
mascot-frames2.png  → 能看到精灵在"上浮 / 回落 / 眨眼压缩"之间变化
```

DOM 断言（新增 5 条参数化测试）：

```
stopped / idle / done  → .pixel-agent 带 resting
working / waiting      → 不带
```

截图：`mac-island-analysis/mascot-frames.png`、`mascot-frames2.png`。

## 回归门禁

`check` 0/0 · `unit` 123 · `frontend` 26 · `browser` **93** · `visual` 14。
