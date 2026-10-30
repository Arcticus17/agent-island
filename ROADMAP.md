# Agent Island 开发路线图

## 产品定位

一个常驻 Windows 桌面的“Agent 灵动岛”：一眼看到正在运行的 AI 编程 Agent，并直接在桌面上控制它们。

## v1.1：功能瘦身（已完成）

按“鸡肋即砍”原则移除以下功能，把精力集中在核心体验与用量监控：

- 远程查看整套（局域网 8765 服务、relay-server.py 自建中继、手机 PWA）
- Windows 通知中心集成（小岛本身就是通知载体，避免三重打扰）
- 插件市场 UI（保留 `agents.json` 配置式适配器热加载）
- Cursor / Copilot 适配器（只有进程名匹配，无日志无操作，纯摆设）
- 语音播报、高对比度

动机与后续方向见 [竞品调研与升级统一方案](docs/research/2026-08-18-competitor-analysis-and-upgrade-plan.md)。

## v1.2：用量监控（已完成）

- Claude Code：解析 `~/.claude/projects` 会话记录，汇总近 5 小时 tokens（输入/输出/缓存），常见模型按定价表估算费用
- Codex：解析 `~/.codex/sessions` 的 token_count 事件，显示额度百分比、重置倒计时、余额与无限额度标记
- 展示：紧凑胶囊底部用量条（绿→黄→红随额度变化）、展开面板"用量"行、总览页详情用量行
- 数据全部本地读取，30 秒缓存，不产生任何额外 API 调用

## v1.3：Hook 事件 + 审批卡 + 终端跳回（已完成）

- Hook 接入：一键写入 `~/.claude/settings.json`（合并保留用户配置，自动备份），Node 桥接脚本经本地 127.0.0.1 事件服务器与岛通信，token 鉴权，状态跨重启持久化
- 权限审批：PreToolUse 拦截危险命令，岛上弹审批卡允许/拒绝；安全只读命令自动放行；120 秒无响应或岛离线时回退 Claude 原生确认
- 即时事件：Stop/Notification hook 秒级推送到通知卡（不再依赖轮询）
- 终端跳回：沿进程父链（node→cmd→Windows Terminal）查找窗口并聚焦
- 轮询保留为兜底；未接入 hooks 时行为完全不变

## v1.4：视觉打磨 P0（已完成）

- 弹簧动画曲线：展开/收起、通知卡入场、Agent 芯片切换用 Apple 风格 spring（cubic-bezier 回弹），替换线性过渡
- 阴影分层：环境光 + 主投影 + 内高光三层；等待/完成状态新增黄/绿状态色溢光
- 微交互：按钮 hover 提亮+投影、active 回缩、Agent 芯片 hover 右移、事件按钮接通态绿色高亮
- 细节：细滚动条 + hover 加深、用量条缓动曲线统一
- 参考：Edge-Drop 弹簧动画、Atoll/NotchDrop 交互节奏、clash-verge-rev 阴影分层（详见调研文档"美感与观感"附节）

## v1.5：Windows 适配 + Acrylic 质感（已完成）

- CSS Acrylic：背景模糊从 28px 提升到 40px + 饱和度 1.8 + 细微噪点纹理（feTurbulence），视觉接近 Windows Acrylic 但保形——实测原生 Mica（window-vibrancy）会破坏胶囊圆角透明，且官方文档提示 Acrylic 在 Win11 调整大小时性能差，故放弃原生材质方案
- 字体：Segoe UI Variable Text 优先（Win11 原生 UI 字体，Win10 自动回退 Segoe UI）
- 已验证 Windows 适配项：深浅色跟随系统（prefers-color-scheme）、减弱动画尊重系统设置（prefers-reduced-motion）、DPI 缩放感知、任务栏隐藏

## v1.6：MacBook 灵动岛风格（已完成）

- 新增"风格"预设：外观面板可切换 玻璃 / Mac 灵动岛，默认 Mac 灵动岛
- Mac 风格：纯黑胶囊（参考 MacBook 刘海），去除玻璃渐变与噪点纹理，保留状态色溢光、spring 动画与全部交互
- 玻璃风格保留为备选，切换即时生效并持久化

## v1.6.1：GitHub 发版（已完成）

- 推送 main + v1.1.0~v1.6.0 tag 到 GitHub，Actions 自动构建 EXE/MSI
- 发现安装包版本号仍为 1.0.0（package.json/tauri.conf.json/Cargo.toml 未同步），修复为 1.6.1 并重发，删除 6 个命名错误的 Release
- 现有 Release：v0.1.0、v1.0.0、v1.6.1（安装包 `Agent.Island_1.6.1_x64-setup.exe` / `_en-US.msi`）
- 开发约定新增：**发版前必须同步三处版本号**（package.json、src-tauri/tauri.conf.json、src-tauri/Cargo.toml）

## v1.7.0：界面细节打磨（已完成）

- 状态展示：emoji（🟢🟡🔴）换成 圆点 + 状态色文字，与全项目圆点语言统一
- 数值 tabular-nums：CPU/内存/时长等数字刷新不再跳动
- 字符图标换统一 SVG：紧凑条箭头、会话切换 ◀▶、通知折叠箭头（旋转动画）、关闭 ×
- 日志面板：等宽字体（Cascadia Mono，日志语义）+ 最新一行蓝色渐变高亮
- 章节标签：accent 刻度线；紧凑条 Agent 名加粗加强存在感

## v1.8.0：MacBook 灵动岛动态（已完成）

- 弹簧物理升级：全局 spring 曲线加强回弹（cubic-bezier 0.34/1.36）
- 展开编排：头部→Agent 条→通知区→主体→底栏依次弹簧滑入（错峰 40~60ms），形变与内容流动同步（苹果灵动岛的核心动态签名）
- 圆角色变：展开时底部圆角 +8px，随高度一起弹簧过渡
- 状态微动态：状态变化时圆点 ping 脉冲 + 状态文字颜色渐变交叉淡入
- 切换 Agent：展开面板标题同步淡入切换

## v1.8.1：动态补全 + IPv4 修复（已完成）

- 通知强调呼吸：通知卡弹入时岛体轻微横向脉动 + 顶部高光线闪光（苹果"收到事件"的动态签名）
- 修复 bug：Vite dev server 只监听 IPv6 ::1，WebView2 用 IPv4 访问导致窗口样式全空——强制绑定 127.0.0.1
- 发版记录：v1.8.1 构建中因 tauri.conf.json 被 PowerShell ANSI 写入损坏而失败（中文乱码控制字符），修复后以 v1.8.2 发布
- 开发约定补充：**不得用 PowerShell 读写含中文的 JSON 配置文件**（PS5.1 默认 ANSI 编码），统一用 node/edit 工具

## v1.9.0：状态正确性、Svelte 前端与单实例（已完成）

- 状态真相源重构：进行中/已完成/报错/等待改由结构化事件（Claude hooks、官方会话事件）结合会话绑定判定，正文关键词不再参与状态；进程、回合、等待三维状态与"done 高亮 8 秒、error 高亮 15 秒"规则由单元测试固定
- 会话归属：只展示可靠匹配的当前会话，无法唯一确定时明确显示"无法确认当前会话"；历史会话只出现在总览或手动"查看会话"入口，所选记录消失时提示重新选择而不是换成其他对话
- 兼容层清理：删除四段已无调用点的正文关键词判定代码（317 行），兼容 DTO 的状态只依据进程事实并由快照投影覆盖；新增回归测试钉住"文本不得决定状态"
- 前端迁移：Svelte 5 + TypeScript 成为默认界面，`src/bridge` 是唯一 Tauri 调用边界；自适应模块岛布局（卡片排序、紧凑/标准/宽版、隐藏与恢复、极简/监控/调试预设）、石墨玻璃与纯黑主题、诊断面板与脱敏导出
- 单实例：重复启动不再出现第二个岛，改为把已在运行的窗口唤到前台；用现有 windows crate 的命名互斥体实现，未引入新依赖
- 线程韧性：移除 `panic = "abort"`，共享状态改为锁中毒可恢复，单个后台线程失败不再让整个岛消失
- 测试门禁：Rust 132、单元 123、浏览器交互 77、视觉比对 14、旧前端 26、svelte-check 0 错误 0 警告；视觉回归改用锁定版 Chromium，官方 CDN 过慢时可用 npmmirror 镜像安装
- 待验收：混合 DPI、多显示器、安装/卸载与原生窗口层级仍需实机人工验收，未实测的系统与架构不列为已支持
- 发布记录：`v1.9.0` 由 GitHub Actions 自动构建 EXE/MSI 并发布（`Agent.Island_1.9.0_x64-setup.exe` / `_en-US.msi`），安装包哈希与流水线明细见 [发布记录](docs/verification/2026-09-30-v1.9.0-release.md)；CI 的视觉比对改为报告而不阻塞发布，原因见 [视觉基线刷新记录](docs/verification/2026-09-30-visual-baseline-refresh.md)

## v1.10：收件箱与审批 2.0（已完成，待发布）

- 收件箱：审批/通知/完成事件持久化到 `~/.agent-island/inbox.json`，展开态「待你处理」、收起态数字徽标、离开摘要；新条目到达时从隐藏 peek 成胶囊
- 审批 2.0：本地风险分级、仓库/分支、Edit/Write diff 预览、拒绝并说明、仅本次/10 分钟/本会话授权、低风险批量；审批 120 秒过期后自动移除
- Hook 协议：新增 `PermissionRequest`（Edit/Write/MultiEdit），bridge 改为独立 `hook_bridge.mjs`；旧客户端只发顶层 command 时回填 `tool_input`
- Agent 更新保护：运行中拒绝更新；命令带 `--include=optional --foreground-scripts`；更新后强制校验 `<cli> --version`，失败自动回滚到更新前版本
- Agent 检测：进程匹配只看可执行文件/入口，不再把提示词里的 `opencode`/`hermes` 当成 Agent；可执行文件探测覆盖 `.npmrc` 自定义 prefix、环境变量（如 `CLAUDE_CODE_PATH`/`CODEX_HOME`/`OPENCODE_HOME`，含自定义 `*_AGENT_HOME` 候选）、Scoop/Chocolatey/Volta/Bun/pnpm 与 PATH；诊断抽屉显示每个 Agent 的实际路径与来源，未安装的明确标「未检测到」
- Agent 添加：Agent 条末尾「+」打开检测面板；列出本机已安装的配置 Agent + 内置候选目录，一键添加；「如没有想要的，可自己添加」支持名称 + 命令/路径，写入 `agents.json` 前校验可执行文件真实存在
- 验证：Rust 228、Node 29（含 3 条 bridge 协议测试）、浏览器 136、视觉 24；真机 CDP 验证 allow / 10 分钟 grant / deny reason / inbox peek，并核对 Agent 检测路径与本机文件一致
- 记录：[docs/verification/2026-10-28-inbox-and-approval-2.md](docs/verification/2026-10-28-inbox-and-approval-2.md)、[docs/verification/2026-10-28-agent-update-guard.md](docs/verification/2026-10-28-agent-update-guard.md)、[docs/verification/2026-10-28-agent-detection-exactness.md](docs/verification/2026-10-28-agent-detection-exactness.md)

## v1.11：团队编排（审批 + 预算 + worktree + 并行 + 多轮 + 依赖链 + 执行级验收已完成，下一步 Codex/视觉）

- 主 Agent 规划：Claude `--json-schema` 输出结构化计划，先预览/可编辑，再确认执行
- 次级 Agent 执行：独立 `--session-id`、`acceptEdits` 写文件、10 分钟超时、取消杀进程树
- 并行：互不依赖的任务按并发上限（1–4，默认 2）同时执行；预算按槽位预留；合并仍逐个确认
- 依赖链：任务可声明单个依赖，依赖任务从依赖分支 tip 创建 worktree，能看到前一个任务的改动；同链串行，独立根任务仍并行；合并必须先合并依赖；丢弃依赖级联丢弃整条链；不支持分叉/多依赖
- 执行级验收：计划可带验收命令；worker 结束后在 worktree 内执行，记录退出码/输出/耗时；退出码 0 才允许合并，失败保留改动但不能合并；120 秒超时、输出截断、进程可被取消；删除/推送/下载执行等高风险命令在确认计划时二次确认
- 多轮：上一轮结束且改动都合并/丢弃后，可带上一轮目标、汇总、任务结果再规划下一轮；新一轮 worktree 从当前 HEAD 创建
- worktree：每个任务在独立 git worktree/分支执行；主工作区不动；团队页查看 diff、手动合并或丢弃；非 git 工作区拒绝；主工作区脏时拒绝合并；冲突 abort 并保留分支；Edit/Write 只允许写 worktree 内（绝对路径/`..` 逃逸拒绝）
- 审批：文件编辑/只读 Bash 自动放行；危险 Bash 进入 `waiting_approval`，小岛审批卡允许/拒绝，5 分钟超时按拒绝；只读命令链/安全管道不再反复弹审批
- 完成判定：worker 必须返回结构化 `completed / blocked / failed`；说 blocked/failed 或没返回结构化结果都标失败，避免退出码 0 的假成功；配置了验收命令时还要验收通过
- 恢复：失败/跳过的任务可"重试未完成任务"，已成功的不重跑；重试前清理旧 worktree/分支和验收状态
- 预算：每次运行设置 USD 上限（默认 $2），规划/worker/汇总成本累加并显示；超预算停止派发，重试也受剩余预算约束
- 入口：主岛展开态团队条（进度/待审批）→ 总览团队页；`open_team` 命令
- 汇总与持久化：主 Agent 汇总、`~/.agent-island/team-runs.json` 原子写、重启把中断任务标失败
- 进程隔离：团队 worker/验收 pid 从主岛快照过滤，应用退出 `/T /F` 清理
- 真实端到端：允许→完成、拒绝→失败→重试→完成、worktree 隔离→merge、双任务并发→逐个 merge、两轮（A→合并→B）、依赖链（A→B）、验收通过/失败均跑通；详情见 [验证记录](docs/verification/2026-10-30-team-orchestration-spike.md)
- 设计依据：[第一步设计：审批、状态与入口](docs/research/2026-10-30-team-orchestration-first-step.md)
- 下一步：Codex/自定义 Agent 适配、视觉基线、自动合并（可选）

## 当前已完成

- 常驻灵动岛：悬停展开、拖拽记忆、多显示器、托盘、开机自启
- 三态呈现：**隐藏 →（鼠标移上去）胶囊 →（点一下）完全展开**；隐藏态只留一条 68×6 的热区（深色胶囊 + 状态色小圆点），窗口压到 10px；悬停 60ms 防抖进胶囊，离开 480ms 回到隐藏；展开态不会因为鼠标移开而收起（不打断阅读和终端输入），点 ⌄ 或按 Esc 收起
- Agent 监控：Claude Code、Codex CLI、OpenCode、Hermes（Copilot / Cursor 适配器已按 v1.1“鸡肋即砍”移除）
- 状态展示：工作中、等待中、高负载、已完成、报错、已停止
- 图标风格：像素吉祥物（默认，8×8 单色精灵跟随状态色 + 右翼数值徽标，等待确认时变 `?`）/ 圆点，设置面板可切换；徽标优先取额度百分比，无用数据时回退本会话事件数
- Agent 显示模式：全部检测（运行中 + 已停止/有本地记录，默认）/ 仅运行中（有几个运行显示几个），设置面板切换并持久化；切到仅运行中且无进程时给"当前没有运行中的 Agent"空态
- Agent 存在性探测：「全部检测」只显示**系统上确实还有可执行文件**的 Agent（`where.exe` + 常见安装目录兜底，60s 缓存）；装了又卸载、只剩日志的 Agent 不再显示，也不再把它的残留日志计入诊断
- 展开面板收纳：诊断收成一行可展开、「窗口宽度」并入设置、动作条单行横滚、高度上限 620px（超出岛内滚动）、卡片布局预设下拉常显
- 真实日志：读取各 Agent 最近会话输出，滚动查看、自动跟随
- 会话切换：顶部箭头切 Agent，展开面板底部箭头切会话
- 会话总览：独立窗口实时列出所有会话、状态、最新输出
- 会话操作：打开终端、打开目录、恢复、停止、重启
- 发消息：给 Claude Code / Codex CLI / OpenCode / Hermes 发送提示词；**岛上展开面板新增指令输入框**，发送后流式回执直接显示在岛内（不用切总览），无当前会话或查看历史会话时禁用
- Agent 请求同步弹出：`PreToolUse`/`Notification` 经 hook bridge（`127.0.0.1:8799`）实时推岛并自动展开；审批卡显示来源 Agent，点允许/拒绝把决定回传给 Agent；修掉 HTTP 头解析大小写敏感导致非 undici 客户端审批被静默丢弃的 bug
- 内置终端：展开面板点「内置终端」用 ConPTY（`portable-pty`）+ xterm.js 在岛内起当前 Agent 的**交互式 CLI**（`claude`/`codex`/`opencode`，带会话目录），也可落回 `cmd.exe`；键盘输入写 PTY、尺寸变化同步行列；**会话归 App 持有，收起岛/收起面板都不会重启 Agent**，输出在收起期间继续累积、展开后回放，只有「关闭」或「收起终端」才回收进程；程序白名单 + shim 包装
- 终端放大：终端头部一个图标按钮（Lucide `maximize-2` / `minimize-2` 对角箭头，`aria-pressed` 高亮）——窗口从 420 拉到 720 宽、高度上限放宽到可用高度 92%，终端区 240 → 560px，诊断/输入框/卡片临时让位；还原回到打开终端前的宽度
- 使用统计：累计运行时间、报错次数、完成次数
- 全局快捷键：Ctrl+Alt+I 呼出/隐藏小岛，Ctrl+Alt+O 打开总览
- 安装包：Windows EXE 安装器 + MSI，已挂 GitHub Release
- GitHub：私有仓库，Release 已由 Actions 发布至 v1.9.0（v0.1.0 为首发）

## 阶段计划

### v0.2：把岛做“聪明”（已完成）

- 事件驱动状态：日志/进程变化时岛才亮起，平时保持安静
- 报错/完成通知卡：短暂弹出，几秒后自动收起
- 快捷键：滚轮切 Agent、快捷键展开/隐藏、数字键直达 Agent
- 隐私遮罩：录屏或共享桌面时自动模糊日志、路径、会话内容
- 专注模式：只显示报错 / 只显示指定 Agent / 勿扰时段

已实现：事件通知卡、事件自动展开、滚轮/数字键/方向键快捷键、隐私按钮与录屏软件自动检测、专注模式（仅报错/仅固定 Agent/静音）、勿扰时段（Q 开关，默认 22:00-08:00）。

### v0.3：多 Agent 聚合岛（已完成）

- 多任务分层展示：多个 Agent 状态叠成一叠，展开逐个查看
- 实时输出浮层：展开会话时日志流式呈现
- 自定义显示字段：用户自行勾选 CPU、内存、日志、统计等
- 会话快捷入口：点击文件路径打开文件，点击目录打开资源管理器

已实现：紧凑胶囊图标叠层与数量角标、展开页 Agent 快捷条（点击直达）、日志逐行流入动画与实时标记、字段面板（按 `V` 或点“字段”开关）、目录/文件路径点击打开（`open_path`）。

### v0.4：可扩展的 Agent 平台（已完成）

- 插件式 Agent 适配器：新增 Agent 只需写一个适配器
- 会话恢复补全：Claude、OpenCode、Hermes 与 Codex 对齐
- 统计报表：按天/周看各 Agent 用时、完成数、报错数
- 远程查看：手机或另一台电脑查看 Agent 状态

已实现：`~/.agent-island/agents.json` 配置式适配器（内置默认值，参考 `agent-adapters.example.json`），恢复命令按 CLI 实测对齐（Codex `exec resume`、OpenCode `run -s`），总览页新增按天/7 天统计表与每日统计存档，内置局域网远程查看服务 `http://<电脑IP>:8765`（总览页“远程”按钮显示地址）。

### v0.5：体验与生态（P0 已完成，详见 UPGRADE-RESEARCH.md）

- P0-1：事件驱动出现/收起：无事件只留紧凑胶囊，有事件自动展开，空闲自动收起
- P0-2：通知卡分组折叠：多 Agent 事件叠放，点开看详情
- P0-3：右键菜单/长按：快速打开总览、隐私、勿扰
- P0-4：远程页隐私打码：远程查看时同样隐藏日志和路径
- P0-5：专注模式配置界面：勿扰时段可视化设置

已实现：事件自动展开/通知清空后自动收起、按 Agent 分组折叠的通知卡、右键/长按快捷菜单（总览/隐私/专注/勿扰）、远程接口隐私打码（`?privacy=1` 或本地隐私开启时返回 `[已隐藏]`）、专注配置面板（模式切换 + 勿扰时段时间选择）。

### v0.5：P1（已完成）

- P1-6 多任务堆叠交互：堆叠卡片、滑动切换、长按预览、拖拽排序
- P1-7 系统级融入：Windows 通知中心、任务栏显示开关、开始菜单快捷方式
- P1-8 主题定制：透明度、圆角、宽度、位置吸附
- P1-9 无障碍：状态朗读、高对比度、键盘完整操作

已实现：Agent 堆叠卡片（拖拽排序 + 长按输出预览 + 滚轮切换）、外观面板（透明度/圆角/宽度/吸附/任务栏显示/高对比度/语音播报/系统通知）、Windows 通知中心插件（`tauri-plugin-notification`）、`M` 键打开快捷菜单等键盘入口。

### v0.6：P2（已完成）

- 插件市场：内置 Aider、Gemini CLI、Cline、Qwen Code、Windsurf 适配器，一键安装并热加载
- 公网远程：自建中继服务 + 设备令牌 + 应用定时推送状态
- 移动端：中继服务提供手机网页与 PWA 清单，可添加到主屏幕

已实现：总览页“市场”视图、`~/.agent-island/agents.json` 热刷新、`relay-server.py` 自建中继、总览“远程”面板（中继地址/设备ID/令牌）、中继状态接口与手机页、后端单元测试。

### v1.0：正式发布（已完成）

- GitHub Actions 自动构建：推送 `v*` 标签后自动编译、测试并发布 EXE/MSI
- 版本号统一为 1.0.0
- README 完善：功能、快捷键、配置、远程/中继部署、隐私说明
- Release v1.0.0 已由流水线自动创建并带上安装包

## 长期要求与开发约定

- 阶段顺序：先 1 后 2，按 v0.2 -> v0.3 -> v0.4 推进
- 界面要美观、避免冗余，切换要丝滑、不能闪烁
- 文案和沟通使用中文
- 每次改动后：构建、启动、提交；用户很在意能否回退版本
- 上传或发布前，必须检查 API Key、Token、凭据等敏感文件
- 安装包配置已启用：NSIS EXE + MSI
- 默认不公开仓库；如果公开，需用户确认
