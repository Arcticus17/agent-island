# 验证记录索引

这里记录每次功能落地的验证过程：做了什么改动、**怎么验证的**、结论，以及仍未验证的部分。
按日期倒序；新增验证记录时请同时在这里补一行。

| 日期 | 主题 | 标题 |
| --- | --- | --- |
| 2026-10-28 | [agent-update-guard](2026-10-28-agent-update-guard.md) | Agent 更新保护：运行中拒绝 + 更新后校验 + 失败回滚 |
| 2026-10-28 | [inbox-and-approval-2](2026-10-28-inbox-and-approval-2.md) | 收件箱 + 审批 2.0（v1.10） |
| 2026-10-27 | [diagnostics-noise-and-hooked-identity](2026-10-27-diagnostics-noise-and-hooked-identity.md) | 诊断噪音与 Agent 自报身份（2026-10-27） |
| 2026-10-22 | [app-update-check](2026-10-22-app-update-check.md) | 应用自更新：先做「看得见新版本」，自动安装留给发布链路 |
| 2026-10-22 | [first-run-onboarding](2026-10-22-first-run-onboarding.md) | 首次引导卡片 |
| 2026-10-22 | [global-session-search](2026-10-22-global-session-search.md) | 跨会话全文搜索 |
| 2026-10-22 | [idle-cpu-attribution](2026-10-22-idle-cpu-attribution.md) | 空闲 CPU 归因：诊断轮询按需加载（9.3% → 7.0%） |
| 2026-10-22 | [palette-contract-and-strip-keys](2026-10-22-palette-contract-and-strip-keys.md) | 两处契约加固：命令面板处理表 + Agent 条键盘导航 |
| 2026-10-22 | [signed-auto-update](2026-10-22-signed-auto-update.md) | 自动更新（签名通道） |
| 2026-10-21 | [activation-purple](2026-10-21-activation-purple.md) | 激活紫 + 中性四阶（三套主题统一） |
| 2026-10-21 | [agent-installed-detection](2026-10-21-agent-installed-detection.md) | Agent 只在"真的存在"时才显示：可执行文件探测 |
| 2026-10-21 | [agent-scoped-diagnostics](2026-10-21-agent-scoped-diagnostics.md) | 诊断信息按 Agent 隔离（不混显示） |
| 2026-10-21 | [agent-update-button](2026-10-21-agent-update-button.md) | Agent 更新按钮（联网检查版本 + 岛内执行升级） |
| 2026-10-21 | [approval-allowlist](2026-10-21-approval-allowlist.md) | 审批允许清单 + 审计 |
| 2026-10-21 | [approval-remember-and-quota-warn](2026-10-21-approval-remember-and-quota-warn.md) | 审批卡「记住这条」+ 额度预警 |
| 2026-10-21 | [balanced-expanded-layout](2026-10-21-balanced-expanded-layout.md) | 展开面板改成纵横均衡（560 × ≤560） |
| 2026-10-21 | [collapse-on-outside-click](2026-10-21-collapse-on-outside-click.md) | 展开态点外面空白处收起 |
| 2026-10-21 | [command-palette](2026-10-21-command-palette.md) | 命令面板（Ctrl+K） |
| 2026-10-21 | [csp-hardening](2026-10-21-csp-hardening.md) | CSP 收紧：从 `null` 到最小白名单 |
| 2026-10-21 | [daily-spend-budget](2026-10-21-daily-spend-budget.md) | 今日花费 + 每日预算守卫 |
| 2026-10-21 | [expanded-panel-compression](2026-10-21-expanded-panel-compression.md) | 展开面板变长治理：收纳次要视图与按钮 |
| 2026-10-21 | [global-hotkey](2026-10-21-global-hotkey.md) | 全局快捷键：Ctrl+Shift+A 唤出 / 收起 |
| 2026-10-21 | [icon-style-pixel-baseline-refresh](2026-10-21-icon-style-pixel-baseline-refresh.md) | 图标风格切到像素吉祥物与视觉基线刷新 |
| 2026-10-21 | [incremental-session-index](2026-10-21-incremental-session-index.md) | 增量会话索引：把每轮全量重读改成只读变化的文件 |
| 2026-10-21 | [island-agent-interaction](2026-10-21-island-agent-interaction.md) | 灵动岛 ↔ Agent 双向交互：发指令 + 审批同步弹出 |
| 2026-10-21 | [island-builtin-terminal](2026-10-21-island-builtin-terminal.md) | 岛内内置终端：直接跑 Agent CLI |
| 2026-10-21 | [island-presence-hover](2026-10-21-island-presence-hover.md) | 灵动岛三态：隐藏 → 悬停显示 → 点击完全展开 |
| 2026-10-21 | [mascot-resting-animation](2026-10-21-mascot-resting-animation.md) | 像素吉祥物：Agent 没在跑时也有动画 |
| 2026-10-21 | [pending-approval-attention](2026-10-21-pending-approval-attention.md) | 审批不被埋没：「待确认」信号 + 不再自动收起 |
| 2026-10-21 | [perf-budget-and-idle-polling](2026-10-21-perf-budget-and-idle-polling.md) | 性能预算进 CI + 空闲轮询降频 |
| 2026-10-21 | [pinned-agent](2026-10-21-pinned-agent.md) | 固定 Agent |
| 2026-10-21 | [rolling-logs](2026-10-21-rolling-logs.md) | 滚动日志（`~/.agent-island/logs/`） |
| 2026-10-21 | [select-popup-dark-fix](2026-10-21-select-popup-dark-fix.md) | 修复：设置里的原生下拉菜单白底白字 |
| 2026-10-21 | [session-log-search](2026-10-21-session-log-search.md) | 会话内搜索 + 两个测试问题的修复 |
| 2026-10-21 | [settings-persistence](2026-10-21-settings-persistence.md) | 设置持久化：强杀也不丢（settings.json 原子写） |
| 2026-10-21 | [terminal-maximize](2026-10-21-terminal-maximize.md) | 终端放大（展开）功能 |
| 2026-10-21 | [terminal-session-survives-collapse](2026-10-21-terminal-session-survives-collapse.md) | 修复：收起/展开灵动岛会重启 Agent 终端 |
| 2026-10-21 | [terminal-theme](2026-10-21-terminal-theme.md) | 终端主题（换皮，不动布局） |
| 2026-09-30 | [legacy-text-status-removal](2026-09-30-legacy-text-status-removal.md) | 移除正文关键词状态判定 |
| 2026-09-30 | [panic-resilience](2026-09-30-panic-resilience.md) | 单个线程失败不再让整个岛消失 |
| 2026-09-30 | [single-instance](2026-09-30-single-instance.md) | 单实例守护 |
| 2026-09-30 | [v1.9.0-release](2026-09-30-v1.9.0-release.md) | v1.9.0 发布记录 |
| 2026-09-30 | [visual-baseline-refresh](2026-09-30-visual-baseline-refresh.md) | 视觉回归首次跑通与基线刷新 |
| 2026-09-28 | [explicit-session-view](2026-09-28-explicit-session-view.md) | 显式本地会话查看 |
| 2026-09-11 | [recording-followup](2026-09-11-recording-followup.md) | 录屏问题复查 |
| 2026-09-10 | [interaction-repair](2026-09-10-interaction-repair.md) | Windows 交互修复验证（2026-09-10） |
| 2026-09-04 | [adaptive-ui](2026-09-04-adaptive-ui.md) | Adaptive UI release verification |
| 2026-09-04 | [session-status](2026-09-04-session-status.md) | Windows session and status verification |
|  | [public-release-compatibility](public-release-compatibility.md) | 公开发布适配门槛 |
