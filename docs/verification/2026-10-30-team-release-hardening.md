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
- 应用版本：1.9.1

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

## 安装包

`npm run tauri build` 已编译 release 并生成两个 1.9.1 安装包：

| 文件 | 大小 | SHA256 | Authenticode |
| --- | ---: | --- | --- |
| `Agent Island_1.9.1_x64-setup.exe` | 3,846,039 | `2795977BAA258A319A3CEBA6109846C84167B811A2A4FAACD52195C5FF219114` | NotSigned |
| `Agent Island_1.9.1_x64_en-US.msi` | 5,345,280 | `366E12292550FF1A8D622DC2F8DD75C3779584B11953B7D50F45EB1318BCBEE4` | NotSigned |

非破坏性校验：

- `msiexec /a <msi> /qn TARGETDIR=<temp>` 退出码 0，包内 `dynamic-island.exe` 版本 1.9.1、大小 13,870,080 字节。
- 从 MSI 管理安装目录直接启动 release exe，进程存活 6 秒后手动结束；说明 release 二进制能启动。
- **没有执行真正的系统安装**，避免改动当前机器；安装/卸载/升级仍需干净虚拟机。

### 构建结论

安装包生成成功，但 `tauri build` 整体退出码为 1：

> A public key has been found, but no private key. Make sure to set `TAURI_SIGNING_PRIVATE_KEY` environment variable.

即：NSIS/MSI 已产出，但 **updater 签名产物没有生成**。当前安装包也都没有 Authenticode 签名。发布自动更新前必须提供签名私钥并重新构建；这是发布 blocker，不是团队功能失败。

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
- 1.9.1 NSIS/MSI 可构建、MSI 可非破坏性展开、release exe 可启动。
- 安装包未签名，updater 私钥缺失导致 `tauri build` 退出码 1；这是发布前必须解决的 blocker。
- 干净机器安装、混合 DPI、多显示器、原生 WebView2 均**未验证**，不能写成已通过。
