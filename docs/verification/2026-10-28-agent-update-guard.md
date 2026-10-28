# Agent 更新保护：运行中拒绝 + 更新后校验 + 失败回滚

日期：2026-10-28
范围：`src/agent/updates.ts`、`src/app/App.svelte`、`tests/updates.test.ts`、`tests/browser/island-actions.test.ts`、`docs/verification/README.md`。

## 一、背景：一次真实的「Claude 不显示了」

验收时发现岛里 Claude 消失。根因不是岛丢数据，而是 Claude Code 安装坏了：

- `D:\node_global\node_modules\@anthropic-ai\claude-code\bin\claude.exe` 只有 **500 字节**，内容是 `Error: claude native binary not installed.` 的占位脚本；
- 真正的原生包 `@anthropic-ai/claude-code-win32-x64` 已下载（109MB），但 postinstall 没有把它复制到 `bin\claude.exe`；
- 全局 shim `D:\node_global\claude.cmd` 也缺失，只剩 npm 临时文件；
- 岛在「全部检测」模式下只显示还有可执行文件的 Agent，Claude 进程一停就被过滤掉。

用 `npm install -g @anthropic-ai/claude-code@2.1.312 --include=optional --foreground-scripts` 重装修复，`claude --version` 恢复为 `2.1.312`。

这次事故暴露了应用「更新 Agent」按钮的两个缺口：**运行中更新会锁文件**、**npm 退出后没有验证安装是否真的可用**。

## 二、做了什么

### 1. 更新命令补上原生依赖与可见脚本

```text
npm install -g <pkg>@<version> --include=optional --foreground-scripts
```

- `--include=optional`：Claude Code 的 native binary 在 optional dependency 里，不能省；
- `--foreground-scripts`：postinstall 输出进入岛内终端，不再被 npm 吞掉；
- `version` 只接受 `x.y.z` / `x.y.z-prerelease`，非法值退回 `latest`，避免拼进 shell 命令。

### 2. 运行中直接拒绝

`runAgentUpdate` 在启动 npm 前检查当前 Agent 的 `state.process`：

```text
请先关闭 Claude Code 再更新：Windows 会锁住 claude 的可执行文件，更新可能只装一半。
```

不启动终端、不改任何文件；消息显示在更新按钮下方（`data-testid="agent-update-message"`），不再只藏在 tooltip 里。

### 3. 更新后强制校验 + 失败回滚

更新命令的 PTY 退出后，不直接相信 npm 的退出状态，而是：

1. 跑 `<cli> --version`；
2. 能拿到版本 → `已更新到 vX` / `更新已结束，但仍是 vX（未到 vY）`；
3. 拿不到版本，且更新前记录过旧版本 → 自动执行
   `npm install -g <pkg>@<旧版本> --include=optional --foreground-scripts`；
4. 回滚后再校验一次：
   - 成功 → `已回滚到 vX`
   - 仍失败 → `回滚后仍无法运行，请手动重装`

回滚只做一次，避免网络持续失败时无限循环。

### 4. 冻结更新目标

更新开始时把 `{ name, package, cli }` 存进 `updateTarget`；更新期间切换 Agent 不会把校验命令换成另一个 CLI。更新期间自动版本检查也会暂停，避免 `updateMessage` 被覆盖。

## 三、验证

| 层 | 结果 |
| --- | --- |
| 单元（`tests/updates.test.ts`） | 6 passed：命令包含 `--include=optional --foreground-scripts`；指定版本；非法版本退回 `latest` |
| 浏览器（`agent update button` 组） | 5 passed：正常更新、运行中拒绝、更新失败自动回滚、已是最新、registry 失败 |
| `svelte-check` | 0 errors / 0 warnings |

回滚用例的模拟顺序：

1. 初始 `claude --version` → `0.1.0`；
2. 更新后探测 → `ok:false`；
3. 自动发起回滚命令 → `npm install -g @anthropic-ai/claude-code@0.1.0 --include=optional --foreground-scripts`；
4. 回滚后探测 → `0.1.0`；
5. 界面显示「已回滚到 v0.1.0」，按钮回到可用状态。

## 四、仍未验证 / 限制

1. **没有真实跑一次「有新版」的更新**：当前本机 Claude 已是 npm 最新 `2.1.312`，为避免再次改动全局环境，本次只验证了命令构造、运行中拒绝和模拟回滚；真实更新还需在下一次有新版时跑一次。
2. **PTY 没有退出码**：现状用 `<cli> --version` 判定成功，而不是 npm 的 exit code；如果 npm 失败但旧版本仍可用，会显示「仍是 vX」而不是「更新失败」。
3. **回滚依赖网络**：回滚本身也是一次 npm install；网络不可用时回滚会失败，只能提示手动重装。
4. **本地版本未知时不回滚**：没有可回滚的版本，失败后只提示手动重装。
5. **进程检测范围**：运行中判断来自当前快照里选中 Agent 的 `state.process`；同一 CLI 在别处启动但未被快照绑定的进程不一定能拦截。
6. **Claude 原生包**：`--include=optional` 能避免「只装 JS 壳」，但如果 npm 配置了 `--omit=optional` 或 `--ignore-scripts`，仍可能留下占位文件；应用会在更新后校验时发现并尝试回滚。

## 五、后续

- 在「更新」前要求二次确认，并显示当前版本与目标版本；
- 把更新结果写进 `~/.agent-island/` 的更新日志，方便排查；
- 如果 npm 支持，读取真实 exit code 并区分「npm 失败」与「安装成功但 CLI 不可运行」。
