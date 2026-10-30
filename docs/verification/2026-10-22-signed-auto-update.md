# 自动更新（签名通道）

日期：2026-10-22
范围：`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`、`src-tauri/capabilities/default.json`、`src-tauri/src/lib.rs`、`src/app/release.ts`、`src/app/App.svelte`、`src/bridge/*`、`.github/workflows/release.yml`、测试

## 一、依赖：先把"顺手升级框架"挡住

`cargo add tauri-plugin-updater` 默认会把整条链抬一档（tauri 2.11.5 → 2.12.5、wry 0.55.1 → 0.57.0，lock 变动 900+ 行）。
上一轮（通知插件）就是因为这个放弃的，这次用**钉版本**解决：

1. `cargo add "tauri-plugin-updater@=2.2.0"`（精确版本，不用 `^`）；
2. 还原 `Cargo.lock` 后重新 `cargo build`，让 cargo **最小化**更新，只加插件及其独有依赖。

最终实际变动：`tauri-plugin 2.7.2`、`tauri-plugin-updater 2.2.0`、`tauri-utils 2.9.3→2.10.2`、`ctor`、`urlpattern`、`zip` 等传递依赖；
**tauri 2.11.5 与 wry 0.55.1 保持不变**——运行时没被动过，风险可控。

## 二、签名与配置

生成本地密钥（私钥不进仓库）：

```
npx tauri signer generate -w ~/.tauri/agent-island.key --ci
```

`tauri.conf.json` 增加：

```json
"bundle": { "createUpdaterArtifacts": true },
"plugins": {
  "updater": {
    "endpoints": ["https://github.com/Arcticus17/agent-island/releases/latest/download/latest.json"],
    "pubkey": "<生成的公钥>",
    "windows": { "installMode": "passive" }
  }
}
```

`capabilities/default.json` 加 `"updater:default"`——权限收紧到只允许更新相关命令。

## 三、界面与逻辑

- 「检查更新」**优先走 updater 插件**（带签名校验）：有新版本 → `有新版本 vX（可一键安装）`；没有 → `已是最新版本（已校验签名）`；
- 插件不可用（浏览器预览、旧宿主）→ 退回上一轮的 GitHub release 提示 + 「打开下载页」；
- 点「下载并安装」→ 显示下载百分比，完成后提示「安装完成，重启应用后生效」；失败显示「安装失败：…」，**不会假装成功**；
- 安装按钮在安装过程中禁用，安装完成后隐藏。

## 四、发布流水线

仓库里本来就有一套 `release.yml`（打 tag → 全量回归 → tauri-action 发布安装包）。这次只补两点：

- `env` 增加 `TAURI_SIGNING_PRIVATE_KEY` / `_PASSWORD`（来自仓库 Secrets）；
- `with` 增加 `includeUpdaterJson: true`，让 release 附上 `latest.json`（内含安装包 URL 与签名）。

客户端读的就是 `releases/latest/download/latest.json`——因此**发布必须是非 draft、非 prerelease**，否则 `latest` 指不到。

## 五、验证

| 层 | 用例 |
| --- | --- |
| Rust（1 条） | `tauri_config_wires_the_signed_updater`：`createUpdaterArtifacts=true`、公钥长度足够、endpoint 是 https 且以 `latest.json` 结尾 |
| 单测（1 条） | `appUpdateFromPlugin`：null → 已是最新；有版本 → available 且文案含版本号 |
| 单测（1 条） | `withTimeout`：挂起 → 超时、正常值透传、原生错误透传 |
| 浏览器（2 条） | 签名通道：检查 → 「下载并安装」→ 完成提示、安装按钮消失；安装失败 → 文案以「安装失败」开头。另有 1 条覆盖插件缺席时退回「打开下载页」 |

本地还跑了一次**签名发布构建**（见下节结论），确认 `createUpdaterArtifacts` 能产出 `.sig`。

## 六、本地签名构建：已跑通

```
$env:TAURI_SIGNING_PRIVATE_KEY=<~/.tauri/agent-island.key 全文>
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD=<本地测试口令，2026-10-30 后该测试密钥已退役，不再记录>
npx tauri build --bundles nsis -- --no-default-features
```

> 2026-10-30：这篇文章里的本地测试密钥已退役，更新签名已轮换到新的生产密钥。旧口令不再适用于发布；轮换记录见 [团队编排发布前硬化](2026-10-30-team-release-hardening.md)。

产物：

| 文件 | 大小 |
| --- | --- |
| `src-tauri/target/release/bundle/nsis/Agent Island_1.9.0_x64-setup.exe` | 3,515,593 |
| `src-tauri/target/release/bundle/nsis/Agent Island_1.9.0_x64-setup.exe.sig` | 424（minisign 格式） |

踩到的坑：**未加密的私钥仍会让 CLI 停在 "expect a prompt for password"**，在无交互的 shell 里就是永久挂起。
所以本地这把密钥是带口令生成的；CI 用仓库 Secrets 里的私钥与口令，行为一致。

`latest.json` 不在这里生成——它由 CI 的 `tauri-action`（`includeUpdaterJson: true`）产出并挂到 release。

## 后续

- 真机上的"下载并安装"要等第一个**已签名**的 release 出现才能完整验证（本地只有安装包与签名，没有可拉取的 `latest.json`）；
- 安装后目前提示"重启应用后生效"，没有自动 relaunch（需要 `tauri-plugin-process` 的 `relaunch`，可以下一步加）；
- 本地这把密钥的口令是 `agentisland-dev`（**只用于本机构建**）；正式发布请另外生成密钥、设置强口令，私钥与口令都进 CI Secrets，公钥写进配置。
## 七、实机发现：清单不存在时检查会"冻住"

发布 `latest.json` 之前先点了「检查更新」，界面停在「检查中…」十几秒没有反应。
原因：清单地址（`releases/latest/download/latest.json`）此刻不存在，请求被重定向后挂起，
而 updater 插件的 `check()` 自己不超时。

修法：给插件检查套 10 秒超时（`withTimeout`），超时后显示
`检查失败：检查超时：网络似乎不可达`。实机已确认（截图 `mac-island-analysis/updater-timeout-live.png`）。
单测覆盖了"超时 / 正常 / 原生 reject"三条路径。

这也说明：**在发布第一个签名版本之前**，检查更新必然失败——这是预期行为，不是 bug。

