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

`latest.json` 仍由 CI 的 `tauri-action` 生成，本地构建只产出 `.sig`。CI Secrets 更新后，发布 1.9.2 才能让 1.9.2+ 的自动更新链路工作。

## 干净机器验收清单（未执行）

以下项目需要一台干净的 Windows 11 x64 虚拟机；本次没有执行，不能标记为已通过：

- [ ] 未安装 WebView2 Runtime 时，安装器能否正确引导/安装
- [ ] 普通用户安装、开始菜单快捷方式、单实例启动
- [ ] 首次启动引导、模型/CLI 不存在时的空态
- [ ] 团队页打开、计划生成、取消、退出重开
- [ ] 安装目录含中文/空格路径
- [ ] 升级覆盖安装、设置与运行记录保留
- [ ] 卸载后残留文件与注册表清理
- [ ] 无代码签名时的 SmartScreen 提示（预期会出现）

## 混合 DPI / 多显示器清单（未执行）

本次环境只有单显示器 100% 缩放；以下项目未验证：

- [ ] 100% / 125% / 150% / 200% 缩放下的主岛与团队页
- [ ] 不同缩放显示器之间拖动，窗口位置与尺寸保持
- [ ] 主岛贴边/刘海位置在副屏与负坐标下的表现
- [ ] 团队条在 420px 宽度附近的换行/省略
- [ ] 原生 WebView2 截图与 Chromium 基线的差异检查

浏览器视觉基线固定 `deviceScaleFactor: 1`，不能替代上述 DPI 验收。

## 结论

- 团队页/团队条的关键视觉状态已有可回归基线，21 个视觉用例通过。
- 1.9.2 NSIS/MSI 可构建、MSI 可非破坏性展开、release exe 可启动。
- 使用新生产密钥后 `tauri build` 退出码 0，NSIS/MSI 两个 `.sig` 均生成；旧测试密钥退役。
- 1.9.2 更新签名公钥已切换；1.9.1 及更早版本需手动重装 1.9.2，CI Secrets 仍需人工更新。
- 安装包仍没有 Authenticode 签名，Windows 上可能出现 SmartScreen“未知发布者”。
- `latest.json` 由 CI 的 `tauri-action` 生成，本地构建不产出完整更新清单。
- 干净机器安装、混合 DPI、多显示器、原生 WebView2 均**未验证**，不能写成已通过。
