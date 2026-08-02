# Agent Island

一个常驻桌面的灵动岛小工具，用来一眼看到正在运行的 AI 编程 Agent。

## 当前阶段（第 2 阶段：Agent 工作状态中心）

- 顶部胶囊：悬停展开、横向拖拽、记住上次位置
- 托盘菜单：显示/隐藏、退出
- 开机自启：托盘菜单里可以勾选/取消
- 多显示器：记住小岛停在哪个屏幕，重启后回到原位置
- 工作状态：工作中、等待中、高负载、已停止
- 一键控制：停止 Agent（二次确认）、用上次命令重启 Agent
- 使用统计：累计运行时间、报错次数、完成次数，退出时自动保存
- 会话信息：工作目录、进程数、最后活动时间
- 真实日志：读取 Claude Code / Codex CLI / OpenCode / Copilot 最近会话，显示最近输出与当前文件
- 会话切换：同一 Agent 有多个会话时，箭头优先在会话间切换，再切换到下一个 Agent
- 会话总览：小岛里点“总览”打开独立窗口，实时列出所有会话和最新输出
- 会话级操作：总览窗口里可直接打开终端、重启、停止指定会话
- 发消息：总览窗口可直接给 Claude Code / Codex CLI / OpenCode / Hermes 发送提示词
- 会话恢复：Claude Code / Codex CLI / OpenCode / Hermes 支持“恢复”到会话
- 日志滚动：支持滚轮查看，靠近底部时自动跟随最新输出
- 性能优化：主窗口与总览窗口共用短时缓存，避免重复扫描进程和日志
- 快捷操作：打开项目目录、在该目录打开终端
- Agent 监控：Claude Code、Codex CLI、OpenCode、Hermes、Copilot、Cursor

## 运行

```bash
npm install
npm run tauri dev
```

## 打包

```bash
npm run tauri build
```

日常使用直接运行 `src-tauri/target/release/dynamic-island.exe` 即可，不需要启动开发服务。
