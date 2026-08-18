# 竞品调研与升级统一方案

> 调研时间：2026-08-18。对象：同类灵动岛竞品（EchoIsland、vibe-island、Dynamic-island-for-arch、claude-notch-tracker、lumos）与用量监控生态（Claude-Code-Usage-Monitor、abtop、token-monitor、ClaudeBar）及 UI 参考项目（Edge-Drop、widgetsack）。

## 附：美感与观感调研（2026-08-18 第二轮）

### 参考项目

| 项目 | 亮点 | 可借鉴 |
| --- | --- | --- |
| window-vibrancy（tauri 官方，1k★） | Windows Mica/Acrylic 原生材质 | 岛的透明背景换成原生玻璃材质 |
| Edge-Drop（422★） | Framer Motion 弹簧形变动画 | 展开/收起用 spring 曲线而非线性过渡 |
| Atoll（3.9k★）/ NotchDrop（2k★）/ SuperIsland（638★） | macOS 灵动岛交互细节 | 悬停预览、拖入交互、动画节奏 |
| clash-verge-rev（138k★） | Tauri 深色玻璃 UI 天花板 | 阴影分层、边框渐变、按钮细节 |
| cc-switch（128k★） | Tauri + React 最精致的 AI 工具界面 | 排版、卡片层级、配色 |
| lumos | Mica 材质 + 用量预测小组件 | 材质落地方式 |

### 落地清单（按风险排序）

- P0 纯 CSS：spring 弹性动画曲线（cubic-bezier 回弹）、阴影分层 + 状态色溢光、边框高光渐变、按钮 hover/active 微动效、focus-visible 焦点环、滚动条细化
- P1 材质：window-vibrancy 接入 Mica/Acrylic（注意 Win11 圆角黑角，需验证后作为主题选项）
- P2 图标：单色 SVG 图标替换字符按钮（▶◀× 等）

## 一、功能对比矩阵

| 能力 | Agent Island（我们） | EchoIsland | vibe-island | 用量监控群* | notch-tracker / lumos |
| --- | --- | --- | --- | --- | --- |
| 平台 | Windows | Windows 优先 + macOS 实验 | Win/macOS/Linux | 多平台 | macOS |
| 技术栈 | Tauri+Rust+原生 JS | Tauri+Rust | Rust | Python/Rust/JS | Swift |
| 进程监控（CPU/内存/状态） | ✅ sysinfo 轮询 | ✅ | ✅ | — | — |
| 日志/会话读取 | ✅ 轮询解析 Claude/Codex/OpenCode/Hermes | ✅ 扫描 + 自适应轮询 | ✅ | — | — |
| **实时事件（Hook 接入）** | ❌ 只有轮询 | ✅ hook-bridge + 本地 TCP 事件总线 | 部分 | — | ✅ |
| 通知卡（报错/完成/等待） | ✅ 分组折叠、自动收起 | ✅ 审批/提问/完成/消息队列 | — | — | — |
| **权限审批卡（岛上 Allow/Deny）** | ❌ | ✅ 阻塞式审批 | — | — | ✅（arch 版） |
| **用量/额度/限流监控** | ❌ | ❌ | ❌ | ✅ 全群主打 | ✅ 额度倒计时+预测 |
| **用量预测与预警** | ❌ | ❌ | ❌ | ✅ | ✅ |
| **终端跳回（一键回到终端窗口）** | ❌ | ✅ | ✅ | — | — |
| **会话快照自动恢复** | ❌（仅手动恢复命令） | ✅ 快照+恢复 | ❌ | — | — |
| 空闲会话自动清理 | ❌ | ✅ 30 分钟 | — | — | — |
| 发消息给 Agent | ✅ | — | — | — | — |
| 停止/重启/打开目录 | ✅ | — | — | — | — |
| 配置式适配器/插件市场 | ✅ | — | — | — | — |
| 统计报表（天/周） | ✅ | — | — | — | — |
| 隐私遮罩（录屏检测+远程打码） | ✅ | ❌ | — | — | — |
| 专注模式/勿扰 | ✅ | — | — | — | — |
| 主题定制（透明度/圆角/宽度） | ✅ | — | — | — | ✅ Mica |
| 远程查看（局域网+中继+PWA） | ✅ | ❌ | — | ✅ 多设备同步 | — |
| 多显示器/托盘/自启/快捷键 | ✅ | 部分 | — | — | — |

\* 用量监控群 = Claude-Code-Usage-Monitor(8.6k★) / abtop(3.4k★) / token-monitor(1.4k★) / ClaudeBar(1.4k★)

## 二、结论：我们领先什么、缺什么

### 我们领先（竞品没有）

1. 控制闭环完整：停止/重启/恢复/发消息/打开目录，竞品多为"只看不控"
2. 生态最全：插件市场、远程中继+PWA、隐私打码、统计报表
3. 交互打磨最深：堆叠卡片、通知分组折叠、勿扰时段、主题定制

### 我们缺（且市场已证明需求）

| 空位 | 市场证据 | 当前我们的状态 |
| --- | --- | --- |
| **用量/额度监控** | 4 个工具合计 1.5 万星，全是用户刚需 | 完全没有 |
| **Hook 实时事件** | EchoIsland 与 claude-code-hooks(1.5k★) 都走此路 | 全轮询，有延迟且拿不到"权限请求"事件 |
| **权限审批卡** | EchoIsland、arch 版均支持 | 无法感知 Claude 权限确认，用户必须切回终端 |
| **终端跳回** | EchoIsland 核心卖点 | 无 |
| 用量预测/预警 | lumos、Usage-Monitor 主打 | 无 |
| 会话快照恢复 | EchoIsland 有 | 只有手动恢复 |

## 三、统一升级方案

原则：不做复刻，做"补空位"；每个能力选最可靠的数据源，全部本地读取，不上云。

### v1.1：功能瘦身（已完成，2026-08-18）

按"鸡肋即砍"原则移除：远程查看整套、Windows 通知中心、插件市场 UI、Copilot/Cursor 适配器、语音播报、高对比度。

### v1.2：用量监控（已完成，2026-08-18）

**目标**：岛上新增"用量"层，一眼看到 token 消耗与额度剩余，超限预警。

| 子项 | 数据源 | 参考项目 |
| --- | --- | --- |
| Claude Code 5 小时窗口用量 | 解析 `~/.claude/projects` transcript 的 `usage` 字段汇总 | Claude-Code-Usage-Monitor、ClaudeBar |
| Codex 额度余额 | 解析 `~/.codex/sessions` 的 `token_count` 事件（`used_percent`/`resets_at`/`credits`） | token-monitor、ClaudeBar |
| 超限预警 | 用量条绿→黄→红变色 + 重置倒计时 | notch-tracker、lumos（预测） |
| 展示 | 胶囊底部用量条、展开面板"用量"行、总览页详情行 | 自设计 |

**工作量**：中。已实现：`UsageInfo` 采集（30 秒缓存）、`scan_claude_text`/`scan_codex_text` 解析 + 6 个单元测试。

### v1.3：Hook 事件接入 + 权限审批卡（已完成，2026-08-18）

**目标**：事件从"秒级轮询"升级到"毫秒级推送"，并解锁审批能力。

| 子项 | 方案 | 参考项目 |
| --- | --- | --- |
| Claude hooks 接入 | 写入 `~/.claude/settings.json` 全局 hooks（SessionStart/Stop/PreToolUse 等），经本地 HTTP/TCP 端口推给小岛；轮询保留为兜底 | EchoIsland hook-bridge、disler/claude-code-hooks |
| 权限审批卡 | PreToolUse 拦截 → 岛上弹审批卡 → Allow/Deny 通过 hook 输出返回给 Claude | EchoIsland、Dynamic-island-for-arch |
| 完成/报错即时事件 | SessionEnd/Error 事件直达通知卡，替代日志正则猜测 | EchoIsland |
| 终端跳回 | 记录 Agent 进程所属终端窗口句柄，点岛唤起窗口 | EchoIsland |

**工作量**：大。涉及 hooks 安装/卸载、本地事件服务、审批双向通信。风险：Claude hooks 协议变化；Codex 事件支持有限（保持轮询）。

### v2.0：智能化（可选、观察市场后再定）

- 用量预测（基于速率线性外推，lumos 思路）
- 会话快照自动恢复
- 空闲会话自动清理

## 四、明确不做

1. Linux 客户端（vibe-island 已有，受众小，Rust 核心可移植但暂不投入）
2. 云账号体系与跨设备同步（我们已有自建中继 + PWA，够用）
3. 移动端原生 App（PWA 已覆盖）
4. Electron 迁移（Tauri 内存优势是我们的护城河）

## 五、决策记录

- 2026-08-18：调研确认用量监控为最大差异化空位；同日先完成 v1.1 功能瘦身（用户决策），再完成 v1.2 用量监控（Claude transcript + Codex token_count 数据源实测可用），随后完成 v1.3（Claude hooks 审批卡 + 即时事件 + 终端跳回，含 E2E 实测：安全命令放行 / 危险命令阻塞审批 / 桥接脚本回退）。
