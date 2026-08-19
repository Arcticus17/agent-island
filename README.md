# Agent Island

一个常驻 Windows 桌面的“Agent 灵动岛”：一眼看到正在运行的 AI 编程 Agent，并直接在桌面上控制它们。

## 功能

- 常驻灵动岛：悬停展开、拖拽记忆、多显示器、托盘、开机自启
- Agent 监控：Claude Code、Codex CLI、OpenCode、Hermes
- 事件通知：报错/完成/等待通知卡，按 Agent 分组折叠，自动展开与收起
- 用量监控：Claude Code 5 小时窗口 tokens/费用、Codex 额度百分比与重置倒计时，超限变色预警
- Hook 事件：Claude Code 会话完成/通知秒级直达；危险命令在岛上弹审批卡，允许/拒绝/回退原生确认
- 终端跳回：一键跳回 Agent 所在终端窗口
- 会话总览：独立窗口实时列出会话、日志、统计，可发消息、恢复、停止、重启
- 全局快捷键：`Ctrl+Alt+I` 呼出/隐藏小岛，`Ctrl+Alt+O` 打开总览
- 隐私遮罩：录屏/共享时自动模糊日志与路径
- 专注模式：仅报错、仅固定 Agent、静音、勿扰时段
- 主题定制：透明度、圆角、宽度、位置吸附、任务栏显示
- 统计报表：按天/周查看各 Agent 用时、报错数、完成数

## 安装

从 GitHub Release 下载：

- `Agent.Island_1.6.1_x64-setup.exe`：Windows 安装器
- `Agent.Island_1.6.1_x64_en-US.msi`：MSI 安装包

日常使用直接运行安装后的 `Agent Island` 即可。

## 快捷键

| 按键 | 功能 |
| --- | --- |
| 滚轮 | 切换 Agent |
| `空格` | 展开/收起 |
| `1-9` | 直达指定 Agent |
| `方向键` | 切换 Agent / 会话 |
| `P` | 隐私遮罩 |
| `F` | 专注模式 |
| `Q` | 自动勿扰开关 |
| `M` | 快捷菜单 |
| `V` | 显示字段 |
| `Ctrl+Alt+I` | 全局呼出/隐藏小岛 |
| `Ctrl+Alt+O` | 全局打开总览 |

右键或长按小岛也会弹出快捷菜单。

点击展开面板的“事件”按钮即可接入 Claude hooks（会写入 `~/.claude/settings.json`，原文件自动备份为 `settings.json.agent-island.bak`）。

## 配置

- `~/.agent-island/agents.json`：Agent 适配器配置，新增 Agent 无需改代码
- `~/.agent-island/stats.json`：累计统计
- `~/.agent-island/stats-daily.json`：每日统计

适配器格式参考 [agent-adapters.example.json](agent-adapters.example.json)。

## 隐私说明

- Agent 状态、日志、统计都保存在本机，不上传任何密钥

## 开发

```bash
npm install
npm run tauri dev
```

打包与测试：

```bash
npm run tauri build
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

GitHub Actions 会在推送 `v*` 标签时自动构建 EXE/MSI 并发布到 Release。

## 文档

- [开发路线图](ROADMAP.md)
- [与苹果灵动岛的差异研究与升级清单](UPGRADE-RESEARCH.md)
