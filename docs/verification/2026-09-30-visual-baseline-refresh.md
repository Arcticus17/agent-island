# 视觉回归首次跑通与基线刷新

日期：2026-09-30
分支：`codex/agent-island-reliability`
范围：`tests/visual/__screenshots__/island.visual.test.ts/*.png`、`README.md`

## 背景

此前每次验收都记录"视觉回归被缺失的锁定版 Chromium 阻塞"。本次把它跑通了，并发现**已提交的展开态基线本身是陈旧的**。

## 浏览器为什么装不上

`npx playwright install chromium` 会从 `cdn.playwright.dev` 下载，该地址 307 跳转到 `storage.googleapis.com`。本机直连该跳转后速度只有 45–245 KB/s，且 Playwright 自己的下载器长时间停在 0 字节。

换用 npmmirror 镜像后同样 5 MB 分段的实测速度是 **6382 KB/s**（约为官方路径的 26 倍），195.6 MB 的包几秒下完。

安装方式（与 `npx playwright install --dry-run chromium` 给出的版本一致）：

1. 读取目标版本与目录：`Chrome for Testing 153.0.8010.12`（playwright chromium v1243）→ 目录 `%LOCALAPPDATA%\ms-playwright\chromium-1243`。
2. 下载 `https://cdn.npmmirror.com/binaries/chrome-for-testing/153.0.8010.12/win64/chrome-win64.zip`。
3. 解压到该目录，使 `chromium-1243\chrome-win64\chrome.exe` 存在。
4. 在 `chromium-1243\` 下创建空文件 `INSTALLATION_COMPLETE`（Playwright 用它判断安装完成）。

## 跑通后的结果与诊断

首次比对 14 项中 **6 通过 / 8 失败**。分布极其规律：

- 通过：`compact-idle`、`compact-working`、`compact-error`（两个主题各 3 项，共 6 项）。
- 失败：`monitoring`、`approval`、`long-log`、`single-column`（两个主题各 4 项，共 8 项），全部是展开面板。

差异不是抗锯齿，而是真实布局差异。对照图后确认：

- 基线：`状态 | 用量` → `会话卡片`（整行）→ `日志卡片`（整行），没有统计卡片。
- 实际：`状态 | 用量` → `日志卡片`（整行）→ `会话 | 统计`。

判定依据不是主观观感，而是测试自身的契约：`tests/visual/island.visual.test.ts:55` 明确写入 `DEFAULT_LAYOUTS.monitoring`，而 `src/layout/presets.ts:20-26` 定义 monitoring 为 `status → usage → log → session → stats`，统计卡片可见。**实际渲染与预设定义完全一致，基线是旧顺序、且缺统计卡片**——它们是在无法运行视觉回归的那段时间里留下的。

## 处理

按 README 的规定（"仅在人工检查确认界面变化后更新基线"），在逐张查看差异图、并用 `presets.ts` 交叉验证后，执行 `npm run test:visual:update`，随后 `npm run test:visual` 复跑 **14 / 14 通过**。

被刷新的文件恰好是 8 个展开态基线（4 场景 × 2 主题），4 个紧凑态基线字节未变——与"只有展开面板的卡片顺序与统计卡片变化"这一诊断吻合。

同一环境下 `npm run test:browser` 也首次用锁定版浏览器跑通：6 个文件 **77 / 77 通过**（此前记录为 71 项）。至此 `npm test` 的全部环节与 `cargo test --lib`、`svelte-check` 同时为绿。

## 未验证 / 仍需注意

- 视觉基线绑定 Windows + 锁定版 Playwright Chromium。本机是手动镜像安装，CI 若走官方下载路径仍会遇到同样的慢速/停滞问题，建议 CI 同样改用镜像或预置浏览器缓存。
- 基线只覆盖 14 个固定场景；原生窗口层级、虚化、真实多屏与 DPI 仍未验收。
- 更新基线是对界面的显式接受：若对卡片顺序或统计卡片的呈现有异议，应回退本次基线提交，而不是让比对长期处于失败状态。
