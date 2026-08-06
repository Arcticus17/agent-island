# Agent Island

一个常驻 Windows 桌面的“Agent 灵动岛”：一眼看到正在运行的 AI 编程 Agent，并直接在桌面上控制它们。

## 功能

- 常驻灵动岛：悬停展开、拖拽记忆、多显示器、托盘、开机自启
- Agent 监控：Claude Code、Codex CLI、OpenCode、Hermes、Copilot、Cursor
- 事件通知：报错/完成/等待通知卡，按 Agent 分组折叠，自动展开与收起
- 会话总览：独立窗口实时列出会话、日志、统计，可发消息、恢复、停止、重启
- 全局快捷键：`Ctrl+Alt+I` 呼出/隐藏小岛，`Ctrl+Alt+O` 打开总览
- 隐私遮罩：录屏/共享时自动模糊日志与路径
- 专注模式：仅报错、仅固定 Agent、静音、勿扰时段
- 插件市场：一键安装 Aider、Gemini CLI、Cline、Qwen Code、Windsurf 等适配器
- 远程查看：局域网状态页 + 自建中继公网访问 + 手机 PWA 页面
- 主题定制：透明度、圆角、宽度、位置吸附、高对比度、语音播报
- 统计报表：按天/周查看各 Agent 用时、报错数、完成数

## 安装

从 GitHub Release 下载：

- `Agent.Island_1.0.0_x64-setup.exe`：Windows 安装器
- `Agent.Island_1.0.0_x64_en-US.msi`：MSI 安装包

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

## 配置

- `~/.agent-island/agents.json`：Agent 适配器配置，新增 Agent 无需改代码
- `~/.agent-island/stats.json`：累计统计
- `~/.agent-island/stats-daily.json`：每日统计
- `~/.agent-island/relay.json`：公网中继配置

适配器格式参考 [agent-adapters.example.json](agent-adapters.example.json)。

## 远程查看

### 局域网

打开总览窗口，点“远程”，会显示类似 `http://192.168.x.x:8765` 的地址。同一网络下的手机或电脑打开即可查看。

### 公网（自建中继）

1. 在服务器上运行：

```bash
python relay-server.py
```

2. 在应用总览的“远程”面板填入：

- 中继地址：`http://服务器IP:8787`
- 设备 ID：任意标识，例如 `my-pc`
- 令牌：自己设置的一串密码

3. 手机打开：

```text
http://服务器IP:8787/device/<设备ID>?token=<令牌>
```

手机浏览器支持“添加到主屏幕”，可作为轻量 App 使用。

## 隐私说明

- Agent 状态、日志、统计都保存在本机，不上传任何密钥
- 远程/中继数据受令牌保护
- 自建中继是原型实现，默认使用明文 HTTP，公网部署建议套一层 HTTPS 反代

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
