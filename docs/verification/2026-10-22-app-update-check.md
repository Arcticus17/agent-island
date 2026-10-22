# 应用自更新：先做「看得见新版本」，自动安装留给发布链路

日期：2026-10-22
范围：`src/app/release.ts`（新）、`src-tauri/src/external.rs`（新）、`src/app/App.svelte`、`src/bridge/*`、`src-tauri/tauri.conf.json`、测试

## 结论先说

真正的自动更新（下载安装包 → 校验签名 → 静默替换）**没有做**，原因是硬性的：

1. 需要一对签名密钥和发布产物（`*.nsis.zip` + `*.sig`），当前仓库没有发布流水线；
2. `tauri-plugin-updater` 必须有可访问的更新清单，且安装必须签名，否则等于给应用开了个"任意代码执行"的口子；
3. 前两项都不是"写点代码"能补的，需要发布侧配合。

所以这轮做的是**能用的那一半**：看见版本、检查更新、一键打开下载页。剩下的一半留到发布链路就绪。

## 实现

| 能力 | 说明 |
| --- | --- |
| 版本显示 | `getAppVersion()`（Tauri 的 `getVersion()`），显示在「版本 v1.9.0」 |
| 检查更新 | 读 `https://api.github.com/repos/Arcticus17/agent-island/releases/latest`，解析 `tag_name` / `html_url` |
| 判定 | 复用 `compareVersions`：比当前新 → `有新版本 vX`；否则 `已是最新版本` |
| 打开下载页 | 新增 Rust 命令 `open_external`，`ShellExecuteW` 打开默认浏览器 |
| 超时 | 8s 无响应 → `检查超时：网络似乎不可达`，不会让按钮永远停在"检查中" |

**安全边界**：`open_external` 只接受 `https://`、必须有带点的主机名、长度 ≤ 2048、不允许空白与控制字符
（拒绝 `http:`、`file:`、`javascript:`、`https://localhost` 等）。有 2 条单测钉住。

CSP 相应放开 `connect-src https://api.github.com`，并加进 `tauri_config_keeps_a_strict_csp` 的断言。

## 实机验证（真网络）

以为这台机器连不上 GitHub，实测**可以**：

```
GET https://api.github.com/repos/Arcticus17/agent-island/releases/latest
→ 200 { tag_name: "v1.9.0", html_url: "…/releases/tag/v1.9.0" }
```

应用里点「检查更新」→ 显示 **已是最新版本**（当前 1.9.0，release 也是 1.9.0，判定正确）。
截图：`mac-island-analysis/about-row-live.png`、`about-timeout-live2.png`。

## 自动安装要补的三件事（下一步的清单）

1. 生成签名密钥（`tauri signer generate`），私钥进 CI secret，公钥写进 `tauri.conf.json` 的 `plugins.updater.pubkey`；
2. 每次发布产出 `latest.json`（版本、平台 URL、签名）并挂到 release；
3. 接入 `tauri-plugin-updater`，把现在的「打开下载页」换成「下载并安装」，安装前校验签名。

## 测试

| 层 | 用例 |
| --- | --- |
| 单测（7 条） | manifest 解析、无版本/非 https 拒绝、新/同/更旧版本判定、网络异常、HTTP 404、超时 |
| Rust（2 条） | https URL 放行；`http:`/`file:`/`javascript:`/无主机名/空白/超长全部拒绝 |
| 浏览器（1 条） | 版本号显示 → stub 新版本 → 「有新版本 v1.10.0」+ 打开下载页（记录到 mock）→ stub 失败 → 「检查失败…」 |

## 门禁

`check` 0/0 · `unit` **179** · `browser` **112** · `visual` 24 · Rust **169**。

## 后续

- 「有新版本」目前只在展开面板的版本行里提示，没有做"启动时自动检查 + 徽标"；
- 发布页 URL 写死在 `release.ts`，可以改成从 `tauri.conf.json` 读。
