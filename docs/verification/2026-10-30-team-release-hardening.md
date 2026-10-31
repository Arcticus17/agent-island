# 团队编排发布前硬化（2026-10-30）

## 范围

这次只做发布前硬化，不改团队编排核心逻辑：

1. 团队页/团队条关键状态的视觉基线
2. 当前 Windows 安装包构建与非破坏性校验
3. 干净机器、混合 DPI、多显示器的验收清单与诚实结论

## 环境

- Windows 11 Home China，10.0.26200，x64
- 单显示器 1920×1080，系统缩放 100%（AppliedDPI=96）
- Chromium：Playwright 1.63.0 / Chromium 153.0.8010.12（revision 1243）
- 应用版本：1.9.2（更新签名密钥轮换后）

## 视觉回归

`npm run test:visual`：**2 个测试文件、21 个用例全部通过**。

新增 `tests/visual/team.visual.test.ts`，固定 graphite 主题变量、固定 Date、禁用动画/光标，覆盖 5 个团队状态：

| 基线 | 覆盖 |
| --- | --- |
| `team-plan` | 计划预览：依赖 + 验收命令 + 并发/预算 |
| `team-verification` | 验收通过 / 验收失败、退出码、输出、合并与清理入口 |
| `team-risk` | 高风险验收命令的二次确认面板 |
| `team-dependency-merge` | A 已合并、B 待合并、依赖提示 |
| `team-strip-waiting` | 主岛团队条：1/3 完成、1 个待审批 |

基线位于 `tests/visual/__screenshots__/team.visual.test.ts/`，已人工检查：计划字段可读、验收徽章/命令/输出可见、风险面板列出命令与原因、依赖链的合并/丢弃按钮符合预期、团队条在 420×72 下不溢出。

限制：这是 Chromium + 行内主题变量的截图，不覆盖 overview 外壳 header，也不等于原生 WebView2 渲染。

## 密钥轮换

项目此前使用一把本地测试 minisign 密钥（key id `B48CEA496C22F38F`），口令写在仓库的旧验证记录里。由于公开仓库已经包含口令，正式发布不应继续使用它。

2026-10-30 完成轮换：

- 生成新的生产密钥：`C:\Users\LENOVO\.tauri\agent-island-prod.key`（key id `3B66FF8F0CD833C7`）
- 随机 64 位十六进制口令保存在 `C:\Users\LENOVO\.tauri\agent-island-prod.password`，两个文件只授予当前用户读写 ACL
- `tauri.conf.json` 的 `plugins.updater.pubkey` 已替换为新公钥
- 版本升到 1.9.2，避免覆盖已经发布的 1.9.1
- 1.9.1 及更早版本不能自动升级到新密钥；目前没有已知用户，因此不做桥接版本，发布说明要求手动重装 1.9.2
- GitHub Actions Secrets 仍需人工更新为**新私钥内容**和新口令；本机 `gh` token 已失效，未能代为更新

## 安装包

`npm run tauri build` 使用新生产密钥编译 release，并生成两个 1.9.2 安装包：

| 文件 | 大小 | SHA256 | Authenticode |
| --- | ---: | --- | --- |
| `Agent Island_1.9.2_x64-setup.exe` | 3,844,802 | `35323E1C959871792CF47493D567DFCE001251AC768A8A941D65726BC8A37035` | NotSigned |
| `Agent Island_1.9.2_x64_en-US.msi` | 5,349,376 | `F7539E0397205D05B8AAE2D050CDA81966432A4347744FCFFC48C3A274ABA3EB` | NotSigned |

两个 updater 签名（`.sig`，424 字节）也已生成：

- `Agent Island_1.9.2_x64-setup.exe.sig`
- `Agent Island_1.9.2_x64_en-US.msi.sig`

非破坏性校验：

- `msiexec /a <msi> /qn TARGETDIR=<temp>` 退出码 0，包内 `dynamic-island.exe` 版本 1.9.2。
- 从 MSI 管理安装目录直接启动 release exe，进程存活 6 秒后手动结束；说明 release 二进制能启动。
- **没有执行真正的系统安装**，避免改动当前机器；安装/卸载/升级仍需干净虚拟机。

### 构建结论

设置新密钥环境变量后，`tauri build` **退出码 0**，并生成两个 `.sig`：

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = "$env:USERPROFILE\.tauri\agent-island-prod.key"
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = "<新口令>"
npm run tauri build
```

旧测试密钥仍保留在本机用于回滚排查，但不再用于新的发布；1.9.1 的旧 `.sig` 不能用于 1.9.2。

`latest.json` 仍由 CI 的 `tauri-action` 生成，本地构建只产出 `.sig`。

## GitHub Release v1.9.2

Secrets 更新后推送 `v1.9.2` tag，Release workflow `41065219045` **成功完成**（13m41s）：

- 类型检查、Rust/旧前端/单元/浏览器回归通过
- 视觉比对在 CI runner 上报告了若干 4–5997 像素差异（字体渲染环境不同）；该步骤按设计是 report-only，不阻断发布
- 构建并发布了非 draft、非 prerelease 的 `v1.9.2` Release
- 资产：NSIS、MSI、两个 `.sig`、`latest.json`
- `latest.json` version=1.9.2；其 MSI 签名 key id 为 `c733d80c8fff663b`（minisign 小端字节序），对应新公钥 `3B66FF8F0CD833C7`
- 本地 1.9.2 安装包签名已用 `minisign-verify` 对新公钥做密码学校验，返回 `signature_ok`

Release 地址：`https://github.com/Arcticus17/agent-island/releases/tag/v1.9.2`。

## VM 实测：1.9.2 → 1.9.4

虚拟机与安装：

- VMware，Windows 11 Pro build 26300，1024×768，100% 缩放
- WebView2 Runtime 156.0.4314.50 已存在
- 以管理员账号在干净快照安装 1.9.2，随后覆盖安装 1.9.3、1.9.4；版本显示 `v1.9.4`
- 用户口述：卸载 1.9.4、重装 1.9.2、在应用内检查更新到 1.9.4 均成功；没有保留截图
- 手动用另一种安装包（NSIS/MSI 交叉）覆盖安装时，安装器要求先卸载旧版；属于两种安装器身份/升级机制不同，不是应用内更新失败
- 没有测试无 WebView2、普通用户、中文/空格安装目录和卸载残留

窗口缩放问题（用户录屏 + 实机）：

- 1.9.2 展开态可以被缩到约 200px，设置行和按钮互相压
- 1.9.3 加 360px 最小宽度后不再重叠，但到 360/744 边界时仍闪烁
- 原因：程序化 `setSize` 触发 `resize` → `syncWindowSize` → 再次 `setSize`，边界取整时形成反馈循环
- 1.9.4 忽略程序化 resize 回声、比较最后应用尺寸并防抖用户拖拽；本机采样最小 `360x560`、最大 `744x560` 各只有一种状态，VM 确认不再闪烁
- 收起后窗口回到胶囊宽度（实测约 108–168），最小宽度不会卡在 360

DPI（VM 原生 WebView2 截图，人工检查）：

| 缩放 | 物理宽度 | 逻辑宽度 | 结果 |
| --- | ---: | ---: | --- |
| 100% | ~360 | 360 | 无横向溢出 |
| 125% | ~440 | ~352 | 无横向溢出 |
| 150% | ~520 | ~347 | 无横向溢出 |
| 200% | ~728 | ~364 | 无横向溢出；有效桌面仅 512×384，面板高度约 300，内容滚动 |

200% 下的滚动是 1024×768 + 200% 的物理空间限制，不是布局溢出。多显示器/跨屏拖动仍未测。

## GitHub Release v1.9.4

VM 确认最小/最大不再闪烁后，推送 `v1.9.4` tag，Release workflow `41084326547` **成功完成**：

- tag 指向 `f6e2245`，包含 1.9.3 最小宽度修复、1.9.4 闪烁修复和验收文档
- 非 draft、非 prerelease；资产含 NSIS/MSI、两个 `.sig`、`latest.json`
- `latest.json` version=1.9.4，签名 key id 仍为新生产公钥 `3B66FF8F0CD833C7`
- 1.9.2 使用同一把新密钥，因此可以从 1.9.2 自动更新到 1.9.4；1.9.1 及更早仍需手动重装

Release 地址：`https://github.com/Arcticus17/agent-island/releases/tag/v1.9.4`。

## 干净机器验收清单（部分执行）

以下项目需要一台干净的 Windows 11 x64 虚拟机；本次已做覆盖安装和启动，其余仍未完成：

- [ ] 未安装 WebView2 Runtime 时，安装器能否正确引导/安装（VM 已有 WebView2）
- [ ] 普通用户安装、开始菜单快捷方式、单实例启动（本次是管理员账号）
- [ ] 首次启动引导、模型/CLI 不存在时的空态（已启动，但未逐项验收团队页）
- [ ] 团队页打开、计划生成、取消、退出重开
- [ ] 安装目录含中文/空格路径
- [x] 升级覆盖安装（1.9.2 → 1.9.3 → 1.9.4，版本正确）
- [x] 应用内自动更新（1.9.2 → 1.9.4，用户口述成功，无截图）
- [x] 卸载（1.9.4 卸载后重装 1.9.2，用户口述成功；残留未逐项检查）
- [ ] 卸载后残留文件与注册表清理
- [ ] 无代码签名时的 SmartScreen 提示（预期会出现）

## 混合 DPI / 多显示器清单（部分执行）

- [x] 100% / 125% / 150% / 200% 缩放下的主岛与设置面板（VM 原生截图）
- [ ] 不同缩放显示器之间拖动，窗口位置与尺寸保持
- [ ] 主岛贴边/刘海位置在副屏与负坐标下的表现
- [ ] 团队条在 420px 宽度附近的换行/省略
- [x] 原生 WebView2 截图与 Chromium 基线的差异检查（仅应用面板人工检查）

浏览器视觉基线固定 `deviceScaleFactor: 1`，不能替代上述 DPI 验收。多显示器仍未测。

## 结论

- 团队页/团队条的关键视觉状态已有可回归基线，21 个视觉用例通过。
- 1.9.2 NSIS/MSI 可构建、MSI 可非破坏性展开、release exe 可启动。
- 使用新生产密钥后 `tauri build` 退出码 0，NSIS/MSI 两个 `.sig` 均生成；旧测试密钥退役。
- 1.9.2 更新签名公钥已切换；1.9.1 及更早版本需手动重装 1.9.2。
- GitHub Release `v1.9.2` 已发布，CI Secrets 已更新，`latest.json` 签名 key id 与新公钥匹配。
- VM 覆盖安装 1.9.2 → 1.9.3 → 1.9.4 成功；1.9.4 修复展开态最小宽度与边界闪烁；100/125/150/200% 原生截图无横向溢出。
- GitHub Release `v1.9.4` 已发布，`latest.json` 签名 key id 与新公钥匹配；1.9.2 客户端自动更新到 1.9.4 已由用户口述验证。
- 手动混合 NSIS/MSI 安装包会要求先卸载旧版；这是两种安装器身份/升级机制不同导致的预期提示，不是应用内自动更新故障。
- 安装包仍没有 Authenticode 签名，Windows 上可能出现 SmartScreen“未知发布者”。
- `latest.json` 由 CI 的 `tauri-action` 生成，本地构建不产出完整更新清单。
- 未验证：无 WebView2、普通用户、中文/空格路径、团队页逐项、卸载残留、多显示器/跨屏拖动。
