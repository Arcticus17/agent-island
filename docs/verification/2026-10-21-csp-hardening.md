# CSP 收紧：从 `null` 到最小白名单

日期：2026-10-21
范围：`src-tauri/tauri.conf.json`、`src-tauri/src/tests.rs`

## 问题

`app.security.csp` 一直是 `null` —— 等于不设防：WebView 里任何被注入的脚本/外链都能跑，
唯一需要放行的其实只有两件事：**Tauri IPC** 和**一次 npm registry 的版本查询**。

## 现在的策略

default-src 'self'; object-src 'none'; base-uri 'self'; form-action 'none';
script-src 'self';
style-src 'self' 'unsafe-inline';
img-src 'self' data: asset: http://asset.localhost;
font-src 'self' data:;
connect-src 'self' ipc: http://ipc.localhost https://registry.npmjs.org ws://localhost:1420 http://localhost:1420
```

逐条理由：

| 指令 | 为什么 |
| --- | --- |
| `script-src 'self'` | Svelte 打包产物是同源模块；**没有** `unsafe-inline` / `unsafe-eval` |
| `style-src 'self' 'unsafe-inline'` | Svelte 把样式注入 `<style>`，且用量条等用内联 `style=` |
| `connect-src ipc: http://ipc.localhost` | Tauri 2 的 `invoke` 走这条通道（不加就整个应用失联） |
| `connect-src https://registry.npmjs.org` | 「检查更新」要查 registry，这是唯一的出站请求 |
| `connect-src ws://localhost:1420` | 仅 `tauri dev` 的 HMR；生产构建用不到 |

## 踩到的坑：BOM 让 Tauri 起不来

用 PowerShell 的 `Set-Content -Encoding UTF8` 改完配置后，`cargo` 的 build script 直接报：

```
unable to parse JSON Tauri config file ... because expected value at line 1 column 1
```

因为 PowerShell 5.1 会写 **UTF-8 BOM**，而 Tauri 的配置解析器不接受 BOM。
改成 `[System.IO.File]::WriteAllText(path, text, UTF8Encoding($false))` 后正常——
这和第三轮规则文件那个 BOM 问题是同一类毛病，以后改 JSON 一律走无 BOM 写入。

## 验证（实机，不是只看配置）

| 检查 | 结果 |
| --- | --- |
| 岛主窗口渲染 | 正常（终端主题、卡片、审批抽屉都在），说明 `style-src`/`script-src` 没问题 |
| `invoke` IPC | 正常：快照数据能显示，说明 `ipc: http://ipc.localhost` 放行成功 |
| registry 出站 | 更新按钮显示「已是最新」——`connect-src` 放行，请求真的发出去了 |
| 第二个 WebView（会话总览） | 正常渲染会话列表与历史详情 |

截图：`mac-island-analysis/csp-check.png`、`csp-overview.png`。

## 用测试钉住

```
tests::tauri_config_keeps_a_strict_csp
tests::tauri_config_disables_unused_capabilities
```

断言：CSP 非 null、含 `default-src 'self'`、`script-src 'self'` 且**不含** `unsafe-eval` / 脚本 inline、
放行 npm registry 与 ipc 通道；主窗口仍是 `decorations: false` + `alwaysOnTop: true`。
这样以后谁把 CSP 改回 `null`，CI 会先红。

## 门禁

`check` 0/0 · `unit` 167 · `browser` 109 · `visual` 21 · Rust **162**（+2）。

## 后续

- 生产构建还有 `ws://localhost:1420` 这条开发用的放行，理想做法是按 dev/build 生成不同 CSP（Tauri 支持 `devCsp`）；
- 纵深防御指令（`object-src 'none'`、`base-uri 'self'`、`form-action 'none'`）已一并加上，并被同一组测试断言。

