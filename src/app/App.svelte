<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import "../themes/tokens.css";
  import "../themes/dark.css";
  import "../themes/light.css";
  import { APPEARANCE_STORAGE_KEY, storedAppearance, type Appearance } from "../themes/theme-style";
  import CompactIsland from "../components/island/CompactIsland.svelte";
  import ApprovalStack from "../components/approvals/ApprovalStack.svelte";
  import ApprovalRules from "../components/approvals/ApprovalRules.svelte";
  import CommandPalette from "../components/palette/CommandPalette.svelte";
  import WelcomeCard from "../components/onboarding/WelcomeCard.svelte";
  import GlobalSearch from "../components/search/GlobalSearch.svelte";
  import { markOnboarded, shouldShowOnboarding } from "../onboarding/state";
  import { appUpdateFromPlugin, checkAppUpdate, initialAppUpdateState, withTimeout, type AppUpdateState } from "./release";
  import { buildPaletteCommands, type PaletteActionId, type PaletteCommand } from "../palette/commands";
  import LogCard from "../components/cards/LogCard.svelte";
  import SessionCard from "../components/cards/SessionCard.svelte";
  import StatusCard from "../components/cards/StatusCard.svelte";
  import UsageCard from "../components/cards/UsageCard.svelte";
  import CardBoundary from "../components/common/CardBoundary.svelte";
  import ActionBar from "../components/controls/ActionBar.svelte";
  import PromptComposer from "../components/controls/PromptComposer.svelte";
  import TerminalPanel from "../components/terminal/TerminalPanel.svelte";
  import DiagnosticsPanel from "../components/diagnostics/DiagnosticsPanel.svelte";
  import LayoutEditor from "../components/layout/LayoutEditor.svelte";
  import NotificationStack from "../components/notifications/NotificationStack.svelte";
import TeamStrip from "../components/team/TeamStrip.svelte";
  import InboxPanel from "../components/inbox/InboxPanel.svelte";
  import type { ApprovalItem, CommandFailure, CommandUiState, NotificationItem } from "../components/events/types";
  import { tauriBridge } from "../bridge/tauri";
  import { tauriDiagnosticsBridge, type DiagnosticsBridge } from "../bridge/diagnostics";
  import type { AgentIslandBridge, AgentView, CommandResult, DiagnosticView, HookEvent, InboxItem, SessionView, SourceHealth, AgentDetection, AgentCandidate } from "../bridge/types";
  import { loadLayout, saveLayout } from "../layout/persistence";
  import type { CardId, CardPlacement, LayoutConfigV1 } from "../layout/schema";
  import { createAgentStore, type AgentStoreState } from "../stores/agent-store";
  import { preferredAgent } from "../models/agent-priority";
  import { PINNED_AGENT_KEY, pinnedSelection, storedPinnedAgent } from "../models/pinned-agent";
  import { quotaMessage, quotaWarnMark, QUOTA_WARN_KEY, shouldWarnQuota } from "../usage/quota";
  import {
    BUDGET_OPTIONS,
    BUDGET_STORAGE_KEY,
    SPEND_STORAGE_KEY,
    budgetStatus,
    dailyTotal,
    emptySpendState,
    formatUsd,
    recordCost,
    storedBudget,
    type SpendState,
  } from "../usage/spend";
  import {
    compareVersions,
    fetchLatestVersion,
    packageForAgent,
    parseVersion,
    updateCommand,
  } from "../agent/updates";

  /**
   * 图标风格：默认像素吉祥物；把 localStorage 的 agent-island-icon-style 设成 "dot"
   * 即回到原来的圆点语言（设置面板里的"图标"就是这个开关）。
   */
  function storedIconStyle(): "dot" | "pixel" {
    try {
      return localStorage.getItem("agent-island-icon-style") === "dot" ? "dot" : "pixel";
    } catch {
      return "pixel";
    }
  }

  let {
    bridge = tauriBridge,
    diagnosticsBridge = tauriDiagnosticsBridge,
    pollIntervalMs = 1_500,
    initiallyExpanded = false,
  initiallyPeeked = true,
    iconStyle: initialIconStyle = storedIconStyle(),
  }: {
    bridge?: AgentIslandBridge;
    diagnosticsBridge?: DiagnosticsBridge;
    pollIntervalMs?: number;
    initiallyExpanded?: boolean;
    /** 预览/测试用：直接从「悬停显示」的胶囊态起步，跳过隐藏态 */
    initiallyPeeked?: boolean;
    iconStyle?: "dot" | "pixel";
  } = $props();

  let iconStyle = $state<"dot" | "pixel">(untrack(() => initialIconStyle));

  const store = createAgentStore(untrack(() => bridge));
  let viewState: AgentStoreState = $state(store.current());
  let expanded = $state(untrack(() => initiallyExpanded));
  /** 隐藏 → 悬停显示胶囊 → 点击完全展开。peeked 只在与 expanded 互斥时有效。 */
  let peeked = $state(untrack(() => initiallyExpanded || initiallyPeeked));
  const idleHidden = $derived(!expanded && !peeked);
  /** 有 Agent 在干活就勤快轮询，全部静止时降频（空闲 CPU 减半以上） */
  const anyAgentActive = $derived((viewState.snapshot?.agents ?? []).some(
    (agent) => agent.state.process === "running" && (agent.display_status === "working" || agent.display_status === "waiting"),
  ));
  const effectivePollMs = $derived(pollIntervalMs > 0 ? (anyAgentActive ? pollIntervalMs : pollIntervalMs * 3) : 0);
  let islandElement = $state<HTMLElement>();
  let eventLayerElement = $state<HTMLElement>();
  let approvals = $state<ApprovalItem[]>([]);
  /** 有审批在等：隐藏热区要脉冲、且不要自动收起 */
  const hasPendingApprovals = $derived(approvals.length > 0);
  let notifications = $state<NotificationItem[]>([]);
  /** 收件箱：后端持久化的审批/通知/完成队列。 */
  let inboxItems = $state<InboxItem[]>([]);
  let inboxInitialized = $state(false);
  const inboxCount = $derived(inboxItems.filter((item) => item.state !== "resolved").length);
  const INBOX_SEEN_KEY = "agent-island-inbox-seen";
  let inboxSeenAt = $state((() => {
    try {
      const stored = Number(localStorage.getItem(INBOX_SEEN_KEY));
      return Number.isFinite(stored) && stored > 0 ? stored : Date.now();
    } catch {
      return Date.now();
    }
  })());
  let digestDismissed = $state(false);
  const inboxDigest = $derived.by(() => {
    if (digestDismissed) return null;
    const open = inboxItems.filter((item) => item.state !== "resolved");
    if (open.length < 2) return null;
    const awayMs = Date.now() - inboxSeenAt;
    if (awayMs < 10 * 60_000) return null;
    return {
      minutes: Math.floor(awayMs / 60_000),
      total: open.length,
      approvals: open.filter((item) => item.kind === "approval").length,
      done: open.filter((item) => item.kind === "done").length,
      notifications: open.filter((item) => item.kind === "notification").length,
    };
  });
  let privacy = $state(localStorage.getItem("agent-island-privacy") === "1");
  /** 今日花费（岛在线期间累计）与每日预算，预算 0 表示关闭 */
  let spend = $state<SpendState>(loadStoredSpend());
  let dailyBudget = $state<number>(storedBudget((key) => localStorage.getItem(key)));
  let focusMode = $state<"off" | "errors" | "quiet">(
    localStorage.getItem("agent-island-focus") === "errors"
      ? "errors"
      : localStorage.getItem("agent-island-focus") === "quiet"
        ? "quiet"
        : "off",
  );
  let appearance = $state<Appearance>(storedAppearance());
  let layout = $state<LayoutConfigV1>(loadLayout(localStorage));
  let layoutPersistenceFailed = $state(false);
  let releaseLayoutDrag: (() => void) | null = null;
  let hookEnabled = $state<boolean | null>(null);
  let hookState = $state<CommandUiState>("idle");
  let hookError = $state<CommandFailure | null>(null);
  let listenerError = $state(false);
  let diagnosticIssues = $state<DiagnosticView[]>([]);
  let diagnosticSources = $state<SourceHealth[]>([]);
  let diagnosticStatus = $state<"loading" | "ready" | "failed">("loading");
  /** 命令面板（Ctrl+K） */
  let paletteOpen = $state(false);
  /** 诊断抽屉是否展开：只有展开时才每次轮询都去取诊断 */
  let diagnosticsOpen = $state(false);
  let diagnosticsLoadedOnce = false;
  /** Agent 检测诊断：每个配置的 Agent 是否真的能在本机找到。 */
  let agentDetections = $state<AgentDetection[]>([]);
  let detectionStatus = $state<"idle" | "loading" | "ready" | "failed">("idle");
  let detectionLoadedOnce = false;
  /** “+” 面板：检测本机并添加 Agent */
  let addAgentOpen = $state(false);
  let addAgentCandidates = $state<AgentCandidate[]>([]);
  let addAgentStatus = $state<"idle" | "loading" | "ready" | "failed">("idle");
  let addAgentName = $state("");
  let addAgentProgram = $state("");
  let addAgentError = $state<string | null>(null);
  let addAgentSaving = $state(false);
  /** 首次引导：看过一次就不再出现，命令面板里可以重看 */
  let showWelcome = $state(shouldShowOnboarding((key) => localStorage.getItem(key)));
  /** 关于/更新：只提示新版本，自动安装留给发布链路 */
  let appVersion = $state("");
  let appUpdate = $state<AppUpdateState>(initialAppUpdateState(""));
  /** 固定盯住的 Agent（重启后仍然生效） */
  let pinnedAgentId = $state<string | null>(storedPinnedAgent((key) => localStorage.getItem(key)));
  let diagnosticRequest = 0;
  let diagnosticInFlight: Promise<void> | null = null;
  let diagnosticRefreshQueued = false;
  let disposed = false;
  let resizeRevision = 0;
  let windowPlacementReady = untrack(() => !bridge.initializeWindowPlacement);
  let windowError = $state<string | null>(null);
  let notificationSequence = 0;
  let notificationAutoExpanded = false;
  const notificationTimers = new Map<string, { handle: number; remainingMs: number; startedAt: number; pauseCount: number }>();
  /** 折叠态：贴边刘海的高度。 */
  const collapsedWindowHeight = 40;
  /** 隐藏态：只留一条很薄的"热区"，鼠标扫过就回到胶囊态。 */
  const hiddenWindowHeight = 8;
  /** 展开态的目标宽度：和高度上限 560 一起构成接近正方形的面板。 */
  const expandedWidth = 560;
  const expandedPanelHeight = 560;
  /** 展开态最小宽度：再窄设置行和卡片就会互相挤压。收起态仍允许回到胶囊宽度。 */
  const expandedMinWidth = 360;
  const compactMinWidth = 80;
  /** 没有展开时的窗口宽度：由刘海内容量出来，窗口只比它大一圈。 */
  let compactWindowWidth = 168;
  let restingWidth = 0;
  /** 正在请求中的窗口宽度：webview 的 innerWidth 更新滞后，同步高度时不能拿旧宽度把窗口缩回去 */
  let targetWindowWidth = 0;
  /** 程序化 setSize 期间忽略 onresize，避免 resize → sync → resize 的反馈循环。 */
  let applyingWindowSize = false;
  let lastAppliedWindowSize: { width: number; height: number } | null = null;
  let resizeSyncTimer: number | undefined;
  /** Agent CLI 更新检查：本地版本走 Rust，最新版本走 npm registry（WebView fetch）。 */
  type UpdateStatus = "idle" | "checking" | "ready" | "error";
  type UpdatePhase = "idle" | "installing" | "verifying" | "rolling_back" | "done" | "failed";
  let updateStatus = $state<UpdateStatus>("idle");
  let updateInstalled = $state<string | null>(null);
  let updateLatest = $state<string | null>(null);
  let updateMessage = $state<string | null>(null);
  let updateCheckedFor = "";
  let updateRunning = $state(false);
  let updatePhase = $state<UpdatePhase>("idle");
  let updateRollbackVersion = $state<string | null>(null);
  let updateAwaitingExit = $state(false);
  /** 冻结本次更新的目标：更新期间切换 Agent 不能把校验命令换成另一个 CLI。 */
  let updateTarget = $state<{ name: string; package: string; cli: string } | null>(null);
  let peekTimer: number | undefined;

  /**
   * Agent 显示模式：
   * - "running"：只显示正在跑的（有几个运行显示几个）
   * - "all"：显示系统里检测到的全部（运行中 + 已停止/有本地记录的）
   */
  type AgentMode = "all" | "running";
  const BUDGET_WARN_KEY = "agent-island-budget-warned";
  const AGENT_MODE_KEY = "agent-island-agent-mode";
  function storedAgentMode(): AgentMode {
    try {
      return localStorage.getItem(AGENT_MODE_KEY) === "running" ? "running" : "all";
    } catch {
      return "all";
    }
  }

  /** 今日花费从 localStorage 读回；解析失败就当没有（等价于今天从零开始）。 */
  function loadStoredSpend(): SpendState {
    try {
      const raw = localStorage.getItem(SPEND_STORAGE_KEY);
      if (!raw) return emptySpendState(Date.now());
      const parsed = JSON.parse(raw) as Partial<SpendState>;
      if (!parsed || typeof parsed.date !== "string") return emptySpendState(Date.now());
      return {
        date: parsed.date,
        totals: typeof parsed.totals === "object" && parsed.totals ? { ...parsed.totals } : {},
        baselines: typeof parsed.baselines === "object" && parsed.baselines ? { ...parsed.baselines } : {},
      };
    } catch {
      return emptySpendState(Date.now());
    }
  }

  function saveSpend(next: SpendState): void {
    try {
      localStorage.setItem(SPEND_STORAGE_KEY, JSON.stringify(next));
    } catch {
      // 存不下就算了，不影响界面
    }
  }
  let agentMode = $state<AgentMode>(storedAgentMode());
  let terminalOpen = $state(false);
  let terminalMax = $state(false);
  let widthBeforeMax = 0;
  let terminalId = $state<string | null>(null);
  let terminalProgram = $state<string | null>(null);
  let terminalText = $state("");
  let terminalStarting = $state(false);
  let terminalExited = $state(false);
  let terminalError = $state<string | null>(null);
  let terminalSize = { cols: 80, rows: 20 };
  /** PTY 当前的尺寸（null = 还没开或已关闭）；窗口拖动不应该改变它 */
  let ptySize: { cols: number; rows: number } | null = null;
  /** 岛内终端要启动什么：内置 Agent 跑它自己的 CLI，其它（自定义）Agent 只开系统 shell。 */
  const TERMINAL_TARGETS: Record<string, { program: string; label: string }> = {
    "Claude Code": { program: "claude", label: "Claude Code" },
    "Codex CLI": { program: "codex", label: "Codex CLI" },
    OpenCode: { program: "opencode", label: "OpenCode" },
    Hermes: { program: "hermes", label: "Hermes" },
  };
  function terminalTarget(agent: { name: string; program?: string } | null): { program: string; label: string } {
    const known = agent ? TERMINAL_TARGETS[agent.name] : undefined;
    if (known) return known;
    // 自定义 / 自动发现的 Agent：直接启动它自己的命令，而不是退回 shell
    const program = agent?.program?.trim();
    if (agent && program) return { program, label: agent.name };
    return { program: "powershell", label: "PowerShell" };
  }
  function setAgentMode(value: AgentMode): void {
    agentMode = value;
    try { localStorage.setItem(AGENT_MODE_KEY, value); } catch { /* 存储不可用时只影响持久化 */ }
    void syncWindowSize();
  }
  /** 自动发现的 Agent 只有"运行中 / 固定 / 最近用过"才默认进岛，其余收在「+」面板。 */
  const RECENT_WINDOW_MS = 7 * 24 * 60 * 60 * 1000;
  function recentlyUsed(agent: AgentView): boolean {
    const times = [
      agent.active_session?.last_active_at_ms ?? 0,
      ...agent.history_sessions.map((session) => session.last_active_at_ms),
    ];
    return Math.max(...times) >= Date.now() - RECENT_WINDOW_MS;
  }
  const visibleAgents = $derived.by(() => {
    const agents = viewState.snapshot?.agents ?? [];
    if (agentMode === "running") {
      return agents.filter((agent) => agent.state.process === "running");
    }
    return agents.filter(
      (agent) =>
        agent.source !== "detected" ||
        agent.state.process === "running" ||
        agent.id === pinnedAgentId ||
        recentlyUsed(agent),
    );
  });
  let selectedAgent = $derived.by(() => {
    const current = viewState.selectedAgent;
    if (!current || !visibleAgents.some((agent) => agent.id === current.id)) return null;
    return current;
  });
  let inspectedSession = $state<{ agentId: string; sessionId: string } | null>(null);
  /** 命令面板的命令列表：随快照和当前选中的 Agent 变化 */
  const paletteCommands = $derived(buildPaletteCommands({
    agents: visibleAgents,
    selectedAgentId: selectedAgent?.id ?? null,
  }));
  let inspectingHistory = $derived(inspectedSession?.agentId === selectedAgent?.id && inspectedSession !== null);
  /** 搜索结果里点开的历史会话（不在当前快照的 history 列表里） */
  let searchedSession = $state<SessionView | null>(null);
  let searchedMeta = $state<{ agent: string; at_ms: number } | null>(null);
  let viewingArchive = $derived(inspectingHistory || searchedSession !== null);
  let selectedSession = $derived(
    searchedSession
      ?? (inspectingHistory
        ? selectedAgent?.history_sessions.find((session) => session.id === inspectedSession?.sessionId) ?? null
        : selectedAgent?.active_session ?? null),
  );
  /**
   * 冷启动：有日志适配器、但还没有任何会话（第一次用这个 Agent 时最常见）。
   * 这不等于"检测失败"——需要引导用户先跑一轮，而不是甩一堆"未确认"。
   */
  const coldStart = $derived(
    selectedAgent !== null && selectedAgent.has_log_adapter !== false && selectedSession === null,
  );
  /** 空态只是"还没有数据"（冷启动或没有日志适配器），不该长得像告警。 */
  const quietEmpty = $derived(coldStart || selectedAgent?.has_log_adapter === false);

  // 显示模式过滤掉当前选中的 Agent 时，自动落到第一个可见的（都不可见则保持空态）
  $effect(() => {
    if (visibleAgents.length === 0 || selectedAgent) return;
    store.selectAgent(visibleAgents[0].id);
  });

  // 原生控件（下拉弹层、滚动条、输入光标）跟随明暗，而不是永远深色
  $effect(() => {
    if (typeof document !== "undefined") document.documentElement.style.colorScheme = appearance;
  });

  // 原生窗口的 min size 跟着展开/收起切换：展开态不允许拖到 360 以下，
  // 收起态必须能缩回胶囊宽度，所以不能把全局 minWidth 固定成 360。
  $effect(() => {
    void bridge.setWindowMinWidth?.(expanded ? expandedMinWidth : compactMinWidth);
  });

  // 今日花费：把每个 Agent 的窗口累计花费按增量累加，并检查每日预算
  $effect(() => {
    const agents = viewState.snapshot?.agents ?? [];
    if (agents.length === 0) return;
    const now = Date.now();
    let next = spend;
    let changed = false;
    for (const agent of agents) {
      const cost = agent.usage?.cost_usd ?? null;
      if (cost === null) continue;
      const result = recordCost(next, agent.id, cost, now);
      next = result.state;
      changed ||= result.delta > 0;
    }
    if (changed || next.date !== spend.date) {
      spend = next;
      saveSpend(next);
    }
    const budget = dailyBudget;
    if (budget <= 0) return;
    const status = budgetStatus(dailyTotal(next, now), budget);
    if (status.state !== "near" && status.state !== "over") return;
    const mark = `warned-${next.date}`;
    if (localStorage.getItem(BUDGET_WARN_KEY) === mark) return;
    localStorage.setItem(BUDGET_WARN_KEY, mark);
    pushLocalNotice(
      selectedAgent?.name ?? "Agent",
      selectedAgent?.active_session?.id ?? "",
      `今日花费 ${formatUsd(status.spent)} / 预算 ${formatUsd(budget)}（${Math.round(status.ratio * 100)}%）`,
    );
  });

  // 额度预警：同一额度窗口只提醒一次（用量数据过期时不提醒）
  $effect(() => {
    const agent = selectedAgent;
    if (!agent) return;
    const usage = agent.usage ?? null;
    if (!usage || usage.used_percent === null || usage.used_percent === undefined) return;
    const now = Date.now();
    const lastWarned = localStorage.getItem(QUOTA_WARN_KEY);
    if (!shouldWarnQuota({ agentId: agent.id, usage, nowMs: now, lastWarned })) return;
    const mark = quotaWarnMark(agent.id, usage, now);
    if (!mark) return;
    localStorage.setItem(QUOTA_WARN_KEY, mark);
    pushLocalNotice(
      agent.name,
      agent.active_session?.id ?? "",
      `${agent.name} ${quotaMessage(usage.used_percent, usage.resets_at_secs ?? null, now)}`,
    );
  });

  /**
   * 诊断只显示"当前 Agent"的：后端返回的是所有 adapter 的汇总，
   * 混着显示会让人误以为当前的 Agent 有问题。
   */
  const adapterForName: Record<string, string> = {
    "claude code": "claude",
    "codex cli": "codex",
    opencode: "opencode",
    hermes: "hermes",
  };
  const selectedAdapter = $derived.by(() => {
    const fromDiagnostic = selectedAgent?.diagnostic?.adapter;
    if (fromDiagnostic) return fromDiagnostic.toLowerCase();
    const name = (selectedAgent?.name ?? "").toLowerCase();
    return adapterForName[name] ?? name;
  });
  const agentDiagnosticIssues = $derived(
    diagnosticIssues.filter((issue) => issue.adapter.toLowerCase() === selectedAdapter),
  );
  const agentDiagnosticSources = $derived(
    diagnosticSources.filter((source) => source.source.toLowerCase().startsWith(selectedAdapter)),
  );

  // 展开时自动检查一次更新（每个 Agent 一次；网络失败不重试，点按钮可手动重试）
  $effect(() => {
    if (!expanded) return;
    const name = selectedAgent?.name ?? "";
    if (!name || !agentPackage || updateCheckedFor === name) return;
    untrack(() => { void checkAgentUpdate(); });
  });
  // 更新命令跑完（PTY 退出）后：先校验 CLI 是否真的能运行；不能运行就回滚。
  $effect(() => {
    if (!updateRunning || !updateAwaitingExit || !terminalExited || terminalId) return;
    updateAwaitingExit = false;
    untrack(() => { void finishAgentUpdate(); });
  });

  function addAgentErrorMessage(error: unknown): string {
    const message = error instanceof Error ? error.message : String(error);
    if (message.includes("program_not_found")) return "没找到这个命令，请确认已安装，或填写完整路径。";
    if (message.includes("agent_already_exists")) return "这个 Agent 已经在列表里了。";
    if (message.includes("agent_name_empty")) return "请填写 Agent 名称。";
    if (message.includes("program_empty")) return "请填写命令或可执行文件路径。";
    if (message.includes("program_contains_unsafe_characters")) return "路径里包含不支持的字符。";
    return `添加失败：${message}`;
  }

  async function toggleAddAgent(): Promise<void> {
    addAgentOpen = !addAgentOpen;
    addAgentError = null;
    if (addAgentOpen) {
      await loadAddAgentCandidates();
    }
    void syncWindowSize();
  }

  async function loadAddAgentCandidates(): Promise<void> {
    if (!bridge.detectAgentCandidates) {
      addAgentStatus = "failed";
      addAgentError = "当前宿主不支持 Agent 检测";
      return;
    }
    addAgentStatus = "loading";
    try {
      addAgentCandidates = await bridge.detectAgentCandidates();
      addAgentStatus = "ready";
    } catch (error) {
      addAgentStatus = "failed";
      addAgentError = addAgentErrorMessage(error);
    }
    void syncWindowSize();
  }

  async function addDetectedAgent(candidate: AgentCandidate): Promise<void> {
    if (!bridge.addAgentDefinition || addAgentSaving) return;
    addAgentSaving = true;
    addAgentError = null;
    try {
      await bridge.addAgentDefinition(candidate.name, candidate.program);
      addAgentOpen = false;
      await refreshAll();
      const added = store.current().snapshot?.agents.find((agent) => agent.name === candidate.name);
      if (added) store.selectAgent(added.id);
    } catch (error) {
      addAgentError = addAgentErrorMessage(error);
      await loadAddAgentCandidates();
    } finally {
      addAgentSaving = false;
      void syncWindowSize();
    }
  }

  async function addManualAgent(): Promise<void> {
    if (!bridge.addAgentDefinition || addAgentSaving) return;
    const name = addAgentName.trim();
    const program = addAgentProgram.trim();
    if (!name || !program) {
      addAgentError = !name ? "请填写 Agent 名称。" : "请填写命令或可执行文件路径。";
      return;
    }
    addAgentSaving = true;
    addAgentError = null;
    try {
      await bridge.addAgentDefinition(name, program);
      addAgentName = "";
      addAgentProgram = "";
      addAgentOpen = false;
      await refreshAll();
      const added = store.current().snapshot?.agents.find((agent) => agent.name === name);
      if (added) store.selectAgent(added.id);
    } catch (error) {
      addAgentError = addAgentErrorMessage(error);
    } finally {
      addAgentSaving = false;
      void syncWindowSize();
    }
  }

  function detectionTag(detection: AgentDetection): string {
    if (detection.running) return "运行中";
    if (!detection.installed) return "未检测到";
    if (detection.source.startsWith("env:")) return "环境变量";
    return "已安装";
  }

  function detectionTitle(detection: AgentDetection): string {
    if (detection.source.startsWith("env:")) {
      return `来源：环境变量 ${detection.source.slice(4)}`;
    }
    if (detection.running) return "来源：正在运行的进程";
    return detection.installed ? "来源：PATH / 常见安装目录" : "本机未找到可执行文件";
  }

  async function loadAgentDetections(force = false): Promise<void> {
    if (!bridge.detectAgents && !bridge.refreshAgentDetection) return;
    if (detectionStatus === "loading") return;
    if (!force && detectionLoadedOnce && detectionStatus === "ready") return;
    detectionStatus = "loading";
    try {
      const next = force && bridge.refreshAgentDetection
        ? await bridge.refreshAgentDetection()
        : bridge.detectAgents
          ? await bridge.detectAgents()
          : [];
      agentDetections = next;
      detectionStatus = "ready";
      detectionLoadedOnce = true;
    } catch {
      detectionStatus = "failed";
      agentDetections = [];
    }
    void syncWindowSize();
  }

  async function loadDiagnostics(): Promise<void> {
    if (disposed) return;
    if (diagnosticInFlight) {
      diagnosticRefreshQueued = true;
      diagnosticStatus = "loading";
      void syncWindowSize();
      return;
    }
    diagnosticRefreshQueued = false;
    const request = ++diagnosticRequest;
    if (diagnosticStatus !== "ready") diagnosticStatus = "loading";
    const operation = (async () => {
      try {
        const response = await diagnosticsBridge.getDiagnostics();
        if (disposed || request !== diagnosticRequest || diagnosticRefreshQueued) return;
        diagnosticIssues = response.issues;
        diagnosticSources = response.sources;
        diagnosticStatus = "ready";
      } catch {
        if (disposed || request !== diagnosticRequest || diagnosticRefreshQueued) return;
        diagnosticIssues = [];
        diagnosticSources = [];
        diagnosticStatus = "failed";
      }
      void syncWindowSize();
    })();
    diagnosticInFlight = operation;
    try {
      await operation;
    } finally {
      if (diagnosticInFlight === operation) diagnosticInFlight = null;
      if (diagnosticRefreshQueued && !disposed) void loadDiagnostics();
    }
  }

  async function refreshAll(): Promise<void> {
    await store.refresh();
    // 首轮取一次（摘要徽标要用），之后只在抽屉展开时才跟着刷新
    if (!disposed && (diagnosticsOpen || !diagnosticsLoadedOnce)) {
      diagnosticsLoadedOnce = true;
      void loadDiagnostics();
    }
  }

  async function syncWindowSize(): Promise<void> {
    if (!windowPlacementReady || disposed) return;
    const revision = ++resizeRevision;
    const targetExpanded = expanded;
    await tick();
    if (revision !== resizeRevision) return;
    const islandHeight = islandElement
      ? Math.max(islandElement.scrollHeight, islandElement.getBoundingClientRect().height)
      : 0;
    const eventHeight = targetExpanded && eventLayerElement
      ? eventLayerElement.scrollHeight + 74
      : 0;
    const renderedHeight = Math.ceil(Math.max(islandHeight, eventHeight));
    const availableHeight = window.screen?.availHeight || renderedHeight;
    // 展开面板高度上限：内容再多也不让岛无限变长，超出部分在岛内滚动。
    // 终端放大模式特殊：要留出足够高度给 PTY，只留一点点余量。
    const expandedHeightCap = Math.max(
      collapsedWindowHeight,
      terminalOpen && terminalMax
        ? Math.floor(availableHeight * 0.92)
        : Math.min(expandedPanelHeight, Math.floor(availableHeight * 0.78)),
    );
    const height = targetExpanded
      ? Math.min(
          Math.max(collapsedWindowHeight, renderedHeight),
          expandedHeightCap,
        )
      : peeked
        ? collapsedWindowHeight
        : hiddenWindowHeight;
    // 宽度优先用正在请求的目标值：webview 的 innerWidth 会滞后一帧，
    // 直接用旧宽度同步高度会把刚放宽的窗口又缩回去。
    if (targetWindowWidth > 0 && Math.abs(Math.round(window.innerWidth) - targetWindowWidth) < 1) {
      targetWindowWidth = 0;
    }
    // 折叠态：窗口宽度跟着刘海内容走（fit-content），不再用用户拖出来的宽度。
    if (!targetExpanded) {
      const measured = islandElement ? Math.ceil(islandElement.getBoundingClientRect().width) + 2 : 0;
      if (measured > 40) compactWindowWidth = Math.min(360, Math.max(80, measured));
    }
    const width = targetWindowWidth > 0
      ? targetWindowWidth
      : targetExpanded
        ? Math.round(window.innerWidth)
        : compactWindowWidth;
    try {
      applyingWindowSize = true;
      await bridge.resizeWindow?.({
        width: Math.max(1, width),
        height,
        minWidth: targetExpanded ? expandedMinWidth : compactMinWidth,
      });
    } catch {
      // A browser preview or closing Tauri window may not support resizing.
    } finally {
      await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
      lastAppliedWindowSize = { width: window.innerWidth, height: window.innerHeight };
      applyingWindowSize = false;
    }
  }

  /** onresize 只处理用户拖拽；程序化改尺寸产生的同一尺寸事件直接忽略。 */
  function handleWindowResize(): void {
    if (applyingWindowSize) return;
    if (
      lastAppliedWindowSize &&
      Math.abs(window.innerWidth - lastAppliedWindowSize.width) < 1.5 &&
      Math.abs(window.innerHeight - lastAppliedWindowSize.height) < 1.5
    ) {
      return;
    }
    if (resizeSyncTimer !== undefined) window.clearTimeout(resizeSyncTimer);
    resizeSyncTimer = window.setTimeout(() => {
      resizeSyncTimer = undefined;
      void syncWindowSize();
    }, 80);
  }

  /** 终端放大：连窗口一起放大，还原时回到原来的宽度。 */
  async function toggleTerminalMax(): Promise<void> {
    const next = !terminalMax;
    terminalMax = next;
    if (next) {
      widthBeforeMax = Math.round(window.innerWidth);
      // 窄岛（默认 420）放大时把窗口一起放宽；本来就够宽就只加高，不缩窗口。
      const target = Math.min(760, Math.max(widthBeforeMax, 720));
      targetWindowWidth = target;
      try {
        await bridge.resizeWindow?.({ width: target, height: Math.round(window.innerHeight), minWidth: expandedMinWidth });
      } catch {
        // 浏览器预览不支持真实窗口尺寸，忽略即可
      }
    } else if (widthBeforeMax > 0) {
      const restore = widthBeforeMax;
      widthBeforeMax = 0;
      targetWindowWidth = restore;
      try {
        await bridge.resizeWindow?.({ width: restore, height: Math.round(window.innerHeight), minWidth: expandedMinWidth });
      } catch {
        // 同上
      }
    }
    // 等一帧让 window.innerWidth 反映新的窗口宽度，再按新尺寸算高度
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    await syncWindowSize();
  }

  /**
   * 更新检查：本地版本走 Rust 的 `probe_cli_version`（跑 `<cli> --version`），
   * 最新版本直接 fetch npm registry —— registry 带 CORS 头，WebView 里可以跨域拿。
   */
  const agentPackage = $derived(packageForAgent(selectedAgent?.name ?? null));
  const updateAvailable = $derived(
    updateStatus === "ready" &&
      !!updateInstalled &&
      !!updateLatest &&
      compareVersions(updateLatest, updateInstalled) > 0,
  );
  const updateLabel = $derived.by(() => {
    if (!agentPackage) return "不支持更新";
    if (updateRunning) {
      if (updatePhase === "verifying") return "校验中…";
      if (updatePhase === "rolling_back") return "回滚中…";
      return "更新中…";
    }
    if (updateStatus === "checking") return "检查中…";
    if (updateStatus === "error") return "重试检查";
    if (updateStatus === "ready") {
      if (updateAvailable) return `更新 v${updateLatest}`;
      return updateInstalled ? `已是最新` : "检查更新";
    }
    return "检查更新";
  });

  async function checkAgentUpdate(force = false): Promise<void> {
    if (updateRunning) return;
    const pkg = agentPackage;
    const name = selectedAgent?.name ?? "";
    if (!pkg || !bridge.probeCliVersion) {
      updateStatus = "error";
      updateMessage = "当前 Agent 不支持自动更新";
      return;
    }
    if (updateStatus === "checking") return;
    // 同一个 Agent 已经查过就不再重复；换 Agent 必须重查
    if (!force && updateCheckedFor === name && updateStatus === "ready") return;
    updateStatus = "checking";
    updateMessage = null;
    updateCheckedFor = name;
    try {
      const probe = await bridge.probeCliVersion(pkg.cli);
      updateInstalled = probe.version ?? parseVersion(probe.raw);
      updateLatest = await fetchLatestVersion(pkg.package);
      updateStatus = "ready";
      if (!updateInstalled) updateMessage = "本地版本未知，按 npm 最新版本提示";
    } catch (cause) {
      updateStatus = "error";
      updateMessage = cause instanceof Error ? `检查失败：${cause.message}` : "检查失败";
    }
  }

  /** 更新：把 `npm install -g …` 丢进岛内终端跑，过程直接可见。
   *
   * 两道保护：
   * 1. Agent 还在跑时直接拒绝——Windows 会锁住 claude.exe，postinstall 复制
   *    原生二进制会失败，留下 "native binary not installed" 的占位文件；
   * 2. 命令结束后强制跑 `<cli> --version`；失败则用更新前的版本回滚一次。
   */
  async function runAgentUpdate(): Promise<void> {
    const pkg = agentPackage;
    const agent = selectedAgent;
    if (!pkg || updateRunning) return;
    if (!updateAvailable) {
      await checkAgentUpdate(true);
      return;
    }
    if (agent?.state.process === "running") {
      updateMessage = `请先关闭 ${agent.name} 再更新：Windows 会锁住 ${pkg.cli} 的可执行文件，更新可能只装一半。`;
      void syncWindowSize();
      return;
    }
    updateTarget = { name: agent?.name ?? pkg.package, package: pkg.package, cli: pkg.cli };
    updateRollbackVersion = updateInstalled;
    updateRunning = true;
    updatePhase = "installing";
    updateAwaitingExit = false;
    updateMessage = `正在更新 ${agent?.name ?? pkg.package}…`;
    terminalOpen = true;
    await syncWindowSize();
    await startTerminal("cmd.exe", ["/C", updateCommand(pkg.package)]);
    if (terminalError) {
      failAgentUpdate(`无法启动更新命令：${terminalError}`);
      return;
    }
    updateAwaitingExit = true;
  }

  function failAgentUpdate(message: string): void {
    updateRunning = false;
    updateAwaitingExit = false;
    updatePhase = "failed";
    updateStatus = "error";
    updateMessage = message;
    void syncWindowSize();
  }

  async function finishAgentUpdate(): Promise<void> {
    const target = updateTarget;
    if (!target || !bridge.probeCliVersion) {
      failAgentUpdate("当前宿主无法校验更新结果，请手动运行命令行检查");
      return;
    }
    const phase = updatePhase;
    updatePhase = "verifying";
    updateMessage = phase === "rolling_back" ? "正在确认回滚结果…" : "正在确认新版本…";
    void syncWindowSize();

    let version: string | null = null;
    try {
      const probe = await bridge.probeCliVersion(target.cli);
      version = probe.ok ? (probe.version ?? parseVersion(probe.raw)) : null;
    } catch {
      version = null;
    }

    if (version) {
      updateInstalled = version;
      updateStatus = "ready";
      updateRunning = false;
      updateAwaitingExit = false;
      updatePhase = "done";
      if (phase === "rolling_back") {
        updateMessage = `已回滚到 v${version}`;
      } else if (updateLatest && version !== updateLatest) {
        updateMessage = `更新已结束，但仍是 v${version}（未到 v${updateLatest}）`;
      } else {
        updateMessage = `已更新到 v${version}`;
      }
      void syncWindowSize();
      return;
    }

    if (phase !== "rolling_back" && updateRollbackVersion) {
      updatePhase = "rolling_back";
      updateMessage = `新版本无法运行 ${target.cli}，正在回滚到 v${updateRollbackVersion}…`;
      updateAwaitingExit = false;
      await startTerminal("cmd.exe", ["/C", updateCommand(target.package, updateRollbackVersion)]);
      if (terminalError) {
        failAgentUpdate(`回滚命令启动失败：${terminalError}`);
        return;
      }
      updateAwaitingExit = true;
      return;
    }

    failAgentUpdate(
      phase === "rolling_back"
        ? "回滚后仍无法运行，请手动重装"
        : "更新后无法运行，请手动重装",
    );
  }

  /**
   * 内置终端会话归 App 持有：收起面板/收起岛都不会重启 Agent，
   * 只有点「关闭」或「收起终端」才真正结束 PTY。
   */
  async function startTerminal(program?: string, args: string[] = []): Promise<void> {
    const target = program ?? terminalTarget(selectedAgent).program;
    if (!bridge.terminalOpen) {
      terminalError = "当前宿主不支持内置终端";
      return;
    }
    if (terminalId) await stopTerminal();
    terminalStarting = true;
    terminalError = null;
    terminalExited = false;
    terminalText = "";
    try {
      const id = await bridge.terminalOpen({
        program: target,
        args,
        cwd: selectedAgent?.active_session?.cwd ?? null,
        cols: terminalSize.cols,
        rows: terminalSize.rows,
      });
      if (disposed) {
        void bridge.terminalClose?.(id).catch(() => undefined);
        return;
      }
      terminalId = id;
      // 记录下来：PTY 从这一刻起就是这个尺寸，之后只有真正的尺寸变化才通知它
      ptySize = { cols: terminalSize.cols, rows: terminalSize.rows };
      terminalProgram = target;
    } catch (cause) {
      terminalError = cause instanceof Error ? cause.message : String(cause);
    } finally {
      terminalStarting = false;
      void syncWindowSize();
    }
  }

  async function stopTerminal(): Promise<void> {
    const current = terminalId;
    terminalId = null;
    ptySize = null;
    terminalProgram = null;
    terminalText = "";
    terminalExited = false;
    terminalError = null;
    if (current) {
      try {
        await bridge.terminalClose?.(current);
      } catch {
        // 会话可能已经被外部结束
      }
    }
    void syncWindowSize();
  }

  async function toggleTerminal(): Promise<void> {
    if (terminalOpen) {
      if (terminalMax) await toggleTerminalMax();
      await stopTerminal();
      terminalOpen = false;
      await syncWindowSize();
      return;
    }
    terminalOpen = true;
    await syncWindowSize();
    if (!terminalId) await startTerminal();
  }

  /** 冷启动 CTA：打开岛内终端，并在里面启动这个 Agent 的 CLI。 */
  async function launchAgentInTerminal(): Promise<void> {
    if (!selectedAgent) return;
    if (!terminalOpen) {
      await toggleTerminal();
      return;
    }
    if (!terminalId) await startTerminal(terminalTarget(selectedAgent).program);
  }

  function windowFailure(error: unknown): void {
    if (disposed) return;    windowError = `窗口调整或保存失败：${error instanceof Error ? error.message : String(error)}`;
    expanded = true;
  }

  async function windowAction(action: (() => Promise<void>) | undefined): Promise<void> {
    if (!action) return;
    windowError = null;
    try { await action(); } catch (error) { windowFailure(error); }
  }

  function toggleExpanded(): void {
    notificationAutoExpanded = false;
    if (!expanded) {
      expandIsland();
      return;
    }
    collapseToPeek();
  }

  /**
   * 展开时把窗口放宽成面板（560），收起时回到刘海自己的宽度。
   * 刘海不是"用户拖出来的宽度"：它由内容决定，窗口只比内容大一圈。
   */
  function expandIsland(): void {
    selectDefaultAgent();
    restingWidth = compactWindowWidth;
    expanded = true;
    peeked = true;
    void (async () => {
      try {
        await bridge.setWindowMinWidth?.(expandedMinWidth);
      } finally {
        if (!terminalMax && restingWidth < expandedWidth) {
          await applyWindowWidth(expandedWidth, expandedMinWidth);
        }
        await syncWindowSize();
      }
    })();
  }

  async function applyWindowWidth(width: number, minWidth = compactMinWidth): Promise<void> {
    targetWindowWidth = width;
    // 目标宽度只在这次调整期间有效，避免之后一直压着用户手动改的宽度
    window.setTimeout(() => { if (targetWindowWidth === width) targetWindowWidth = 0; }, 700);
    try {
      await bridge.resizeWindow?.({ width, height: Math.round(window.innerHeight), minWidth });
    } catch {
      // 浏览器预览不支持真实窗口尺寸
    }
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    if (!disposed) void syncWindowSize();
  }

  /** 完全展开 → 收起成刘海（鼠标移开后再自动隐藏）。 */
  function collapseToPeek(): void {
    notificationAutoExpanded = false;
    const restore = restingWidth || compactWindowWidth;
    restingWidth = 0;
    // 收起岛时退出终端放大，别让收起后的刘海还挂着 720 宽
    if (terminalMax) {
      terminalMax = false;
      widthBeforeMax = 0;
    }
    expanded = false;
    peeked = true;
    // 先把窗口最小宽度降回胶囊尺寸，再缩窗口；否则展开态的 360 min 会顶住。
    void (async () => {
      try {
        await bridge.setWindowMinWidth?.(compactMinWidth);
      } finally {
        if (restore > 0 && Math.round(window.innerWidth) !== restore) {
          await applyWindowWidth(restore, compactMinWidth);
        }
        await syncWindowSize();
      }
    })();
  }

  function peekFromHidden(): void {
    if (expanded || peeked) return;
    peekTimer = window.setTimeout(() => {
      peekTimer = undefined;
      if (expanded || peeked || disposed) return;
      peeked = true;
      void syncWindowSize();
    }, 60);
  }

  function clearPeekTimer(): void {
    if (peekTimer !== undefined) {
      window.clearTimeout(peekTimer);
      peekTimer = undefined;
    }
  }

  /**
   * 刘海是常驻的：鼠标离开不再自动隐藏（隐藏只保留给 Esc 与全局快捷键）。
   * 悬停仍然能把隐藏态唤回刘海。
   */
  function handleIslandPointerEnter(): void {
    if (expanded) return;
    if (!peeked) peekFromHidden();
  }

  function hideIsland(): void {
    clearPeekTimer();
    if (terminalMax) void toggleTerminalMax();
    expanded = false;
    peeked = false;
    void syncWindowSize();
  }

  async function checkAppRelease(): Promise<void> {
    if (appUpdate.status === "checking") return;
    appUpdate = { ...appUpdate, status: "checking", message: "正在检查…" };
    // 优先走 updater 插件（带签名校验的内置通道）；不可用时退回 GitHub release 提示
    if (bridge.checkAppUpdate) {
      try {
        const info = await withTimeout(bridge.checkAppUpdate(), 10_000, "检查超时：网络似乎不可达");
        appUpdate = appUpdateFromPlugin(appVersion, info);
      } catch (cause) {
        appUpdate = {
          ...appUpdate,
          status: "failed",
          message: `检查失败：${cause instanceof Error ? cause.message : String(cause)}`,
        };
      }
      return;
    }
    appUpdate = await checkAppUpdate(appVersion);
  }

  async function installAppRelease(): Promise<void> {
    if (!bridge.installAppUpdate || appUpdate.status === "installing") return;
    appUpdate = { ...appUpdate, status: "installing", message: "正在下载更新…", progress: 0 };
    try {
      await bridge.installAppUpdate((downloaded, total) => {
        const progress = total && total > 0 ? Math.min(100, Math.round((downloaded / total) * 100)) : null;
        appUpdate = {
          ...appUpdate,
          progress,
          message: progress === null ? "正在下载更新…" : `正在下载更新… ${progress}%`,
        };
      });
      appUpdate = { ...appUpdate, status: "installed", message: "安装完成，重启应用后生效", progress: 100 };
    } catch (cause) {
      appUpdate = {
        ...appUpdate,
        status: "failed",
        message: `安装失败：${cause instanceof Error ? cause.message : String(cause)}`,
      };
    }
  }

  async function openReleasePage(): Promise<void> {
    const url = appUpdate.latest?.url;
    if (!url || !bridge.openExternal) return;
    try {
      await bridge.openExternal(url);
    } catch (cause) {
      appUpdate = { ...appUpdate, message: `打开链接失败：${cause instanceof Error ? cause.message : String(cause)}` };
    }
  }
  /** 搜索结果 → 打开对应会话（只读展示，不影响当前任务的会话） */
  /** 把当前看到的会话绑定给这个 Agent：岛外启动的 Agent 也能获得确定身份 */
  async function bindSelectedSession(): Promise<void> {
    if (!bridge.bindSession || !selectedAgent || !selectedSession) return;
    try {
      await bridge.bindSession(selectedAgent.id, selectedSession.id);
      await refreshAll();
    } catch (cause) {
      pushLocalNotice(
        selectedAgent.name,
        selectedSession.id,
        `绑定失败：${cause instanceof Error ? cause.message : String(cause)}`,
      );
    }
  }

  async function unbindSelectedSession(): Promise<void> {
    if (!bridge.unbindSession || !selectedAgent) return;
    try {
      await bridge.unbindSession(selectedAgent.id);
      await refreshAll();
    } catch (cause) {
      pushLocalNotice(
        selectedAgent.name,
        selectedAgent.active_session?.id ?? "",
        `解除绑定失败：${cause instanceof Error ? cause.message : String(cause)}`,
      );
    }
  }

  async function openSearchedSession(hit: { agent: string; at_ms: number; path: string }): Promise<void> {
    if (!bridge.loadSessionByPath) return;
    try {
      const session = await bridge.loadSessionByPath(hit.path);
      inspectedSession = null;
      searchedSession = session;
      searchedMeta = { agent: hit.agent, at_ms: hit.at_ms };
      if (!expanded) expandIsland();
      void syncWindowSize();
    } catch (cause) {
      pushLocalNotice(
        selectedAgent?.name ?? "Agent",
        selectedAgent?.active_session?.id ?? "",
        `打开会话失败：${cause instanceof Error ? cause.message : String(cause)}`,
      );
    }
  }

  function closeSearchedSession(): void {
    searchedSession = null;
    searchedMeta = null;
    void syncWindowSize();
  }
  /** Agent 条的方向键导航：左右循环、Home/End 跳到两端 */
  function navigateAgentStrip(event: KeyboardEvent): void {
    if (visibleAgents.length === 0) return;
    const current = visibleAgents.findIndex((agent) => agent.id === selectedAgent?.id);
    const last = visibleAgents.length - 1;
    let next: number | null = null;
    if (event.key === "ArrowRight") next = current >= last || current < 0 ? 0 : current + 1;
    else if (event.key === "ArrowLeft") next = current <= 0 ? last : current - 1;
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = last;
    if (next === null) return;
    event.preventDefault();
    const target = visibleAgents[next];
    inspectedSession = null;
    store.selectAgent(target.id);
    const strip = (event.currentTarget as HTMLElement).parentElement;
    strip?.querySelector<HTMLButtonElement>(`[data-agent-id="${target.id}"]`)?.focus();
  }

  function dismissWelcome(): void {
    showWelcome = false;
    markOnboarded((key, value) => localStorage.setItem(key, value));
  }

  function setPinnedAgent(id: string | null): void {
    pinnedAgentId = id;
    try {
      if (id) localStorage.setItem(PINNED_AGENT_KEY, id);
      else localStorage.removeItem(PINNED_AGENT_KEY);
    } catch {
      // 存储不可用时只影响持久化
    }
  }

  function selectDefaultAgent(): void {
    const current = store.current();
    // Explicit error focus keeps its existing behavior. Failed refreshes do not
    // provide new evidence for automatically switching the displayed agent.
    if (focusMode === "errors" || current.refreshError || !current.snapshot) return;
    if (visibleAgents.length === 0) return;
    // 固定了某个 Agent 就一直停在它上面；它消失时才解除固定并回退到自动选择
    if (pinnedAgentId) {
      const { agent, stalePin } = pinnedSelection(visibleAgents, pinnedAgentId);
      if (stalePin) {
        setPinnedAgent(null);
      } else if (agent) {
        if (current.selectedAgentId !== agent.id) store.selectAgent(agent.id);
        return;
      }
    }
    const preferred = preferredAgent({ ...current.snapshot, agents: visibleAgents }, current.selectedAgentId);
    if (preferred) store.selectAgent(preferred.id);
  }

  function notificationAgent(event: Extract<HookEvent, { kind: "notification" }>) {
    if (store.current().refreshError) return null;
    const agents = store.current().snapshot?.agents ?? [];
    const eligible = agents.filter((agent) =>
      !agent.freshness.stale && agent.active_session !== null,
    );
    if (event.session) {
      const matches = eligible.filter((agent) => agent.active_session?.id === event.session);
      return matches.length === 1 && matches[0].id === "claude" ? matches[0] : null;
    }
    const claude = eligible.filter((agent) => agent.id === "claude");
    return claude.length === 1 ? claude[0] : null;
  }

  function pushNotification(event: Extract<HookEvent, { kind: "notification" }>): void {
    // hook 事件可以带任意 session，必须先反查到真实的 Agent（防伪造）
    const agent = notificationAgent(event);
    if (!agent?.active_session) return;
    pushLocalNotice(agent.name, agent.active_session.id, event.message);
  }

  /** 岛自己产生的提示（额度预警、规则保存回执）：来源可信，不走 hook 的会话校验。 */
  function pushLocalNotice(agentName: string, sessionId: string, message: string): void {
    if (focusMode === "quiet" || focusMode === "errors") return;
    const now = Date.now();
    const duplicate = notifications.find((item) =>
      item.sessionId === sessionId && item.message === message && now - item.receivedAt <= 20_000,
    );
    if (duplicate) {
      notifications = notifications.map((item) => item.id === duplicate.id
        ? { ...item, receivedAt: now, count: item.count + 1 }
        : item);
      scheduleNotificationDismiss(duplicate.id, 6_500);
      void syncWindowSize();
      return;
    }
    const id = `notification-${++notificationSequence}`;
    const retained = notifications.slice(-2);
    for (const item of notifications) {
      if (!retained.some((candidate) => candidate.id === item.id)) clearNotificationTimer(item.id);
    }
    notifications = [
      ...retained,
      { id, agentName, sessionId, message, receivedAt: now, count: 1 },
    ];
    scheduleNotificationDismiss(id, 6_500);
    notificationAutoExpanded ||= !expanded;
    expanded = true;
    void syncWindowSize();
  }

  function clearNotificationTimer(id: string): void {
    const timer = notificationTimers.get(id);
    if (timer) window.clearTimeout(timer.handle);
    notificationTimers.delete(id);
  }

  function scheduleNotificationDismiss(id: string, delayMs: number): void {
    clearNotificationTimer(id);
    const startedAt = Date.now();
    const handle = window.setTimeout(() => dismissNotification(id), delayMs);
    notificationTimers.set(id, { handle, remainingMs: delayMs, startedAt, pauseCount: 0 });
  }

  function dismissNotification(id: string): void {
    clearNotificationTimer(id);
    notifications = notifications.filter((item) => item.id !== id);
    if (notifications.length === 0 && approvals.length === 0 && notificationAutoExpanded) {
      expanded = false;
      notificationAutoExpanded = false;
    }
    void syncWindowSize();
  }

  function pauseNotification(id: string, paused: boolean): void {
    const timer = notificationTimers.get(id);
    if (!timer) return;
    if (paused) {
      if (timer.pauseCount === 0) {
        window.clearTimeout(timer.handle);
        timer.remainingMs = Math.max(0, timer.remainingMs - (Date.now() - timer.startedAt));
        timer.handle = 0;
      }
      timer.pauseCount += 1;
    } else {
      timer.pauseCount = Math.max(0, timer.pauseCount - 1);
      if (timer.pauseCount === 0 && timer.handle === 0) {
        scheduleNotificationDismiss(id, Math.max(250, timer.remainingMs));
      }
    }
  }

  async function loadInbox(): Promise<void> {
    if (!bridge.listInbox) return;
    try {
      const next = await bridge.listInbox();
      const open = (items: readonly InboxItem[]) => items.filter((item) => item.state !== "resolved").length;
      const previous = open(inboxItems);
      inboxItems = next;
      // A new item arriving while the island is fully hidden should at least
      // peek the capsule; otherwise an unmatched notification would be stored
      // but never visible until the user happens to hover.
      if (inboxInitialized && open(next) > previous && idleHidden) {
        peeked = true;
        void syncWindowSize();
      }
      inboxInitialized = true;
    } catch {
      // 收件箱读失败不影响主流程；下一次事件会再试。
    }
  }

  function markInboxSeen(): void {
    inboxSeenAt = Date.now();
    try {
      localStorage.setItem(INBOX_SEEN_KEY, String(inboxSeenAt));
    } catch {
      // localStorage 不可用时只影响摘要判断。
    }
  }

  function dismissInboxDigest(): void {
    digestDismissed = true;
    markInboxSeen();
  }

  async function resolveInbox(id: string): Promise<void> {
    if (!bridge.resolveInboxItem) return;
    try {
      inboxItems = await bridge.resolveInboxItem(id);
      markInboxSeen();
    } catch {
      // 保持现状，让用户重试。
    }
  }

  async function snoozeInbox(id: string, minutes: number): Promise<void> {
    if (!bridge.snoozeInboxItem) return;
    try {
      inboxItems = await bridge.snoozeInboxItem(id, minutes);
      markInboxSeen();
    } catch {
      // 保持现状，让用户重试。
    }
  }

  function focusApproval(id: string): void {
    const selector = `[data-approval-id="${id.replace(/"/g, '\\"')}"]`;
    const node = document.querySelector<HTMLElement>(selector);
    node?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }

  function pushApproval(event: Extract<HookEvent, { kind: "approval" }>): void {
    if (approvals.some((item) => item.approval.id === event.approval.id)) return;
    // 审批只带 session，不带上是谁；这里按会话反查一次，卡片上就能说清"哪个 Agent 在请求"。
    const owner = viewState.snapshot?.agents.find(
      (agent) => agent.active_session?.id === event.approval.session,
    );
    approvals = [...approvals, {
      approval: event.approval,
      agentName: owner?.name,
      state: "idle",
      decision: null,
      error: null,
    } satisfies ApprovalItem];
    notificationAutoExpanded = false;
    expanded = true;
    void syncWindowSize();
  }

  function handleHookEvent(event: HookEvent): void {
    if (event.kind === "stop") {
      void refreshAll();
    } else if (event.kind === "notification") {
      pushNotification(event);
    } else if (event.kind === "approval_expired") {
      approvals = approvals.filter((item) => !event.ids.includes(item.approval.id));
      void loadInbox();
      void syncWindowSize();
    } else {
      pushApproval(event);
    }
  }

  /** 审批卡上的「记住这条」：把 (工具, 建议模式) 加进允许清单，并给出一次可见反馈。 */
  async function rememberApprovalRule(tool: string, pattern: string): Promise<void> {
    if (!bridge.addApprovalRule || !pattern.trim()) return;
    const agentName = selectedAgent?.name ?? "Agent";
    const sessionId = selectedAgent?.active_session?.id ?? "";
    try {
      await bridge.addApprovalRule(tool, pattern);
      pushLocalNotice(agentName, sessionId, `已加入允许清单：${tool || "任意工具"} · ${pattern}`);
    } catch (cause) {
      pushLocalNotice(
        agentName,
        sessionId,
        `加入允许清单失败：${cause instanceof Error ? cause.message : String(cause)}`,
      );
    }
  }

  async function respondToApproval(
    id: string,
    allow: boolean,
    scope?: "once" | "ten_minutes" | "session",
    reason?: string,
  ): Promise<void> {
    const current = approvals.find((item) => item.approval.id === id);
    if (!current || current.state === "pending") return;
    approvals = approvals.map((item) => item.approval.id === id
      ? { ...item, state: "pending", decision: allow, error: null }
      : item);
    const release = store.beginInteraction("approval");
    let result: CommandResult;
    try {
      result = await bridge.runCommand({
        name: "respond_hook_approval",
        id,
        allow,
        ...(scope && scope !== "once" ? { scope } : {}),
        ...(reason && reason.trim() ? { reason: reason.trim() } : {}),
      });
    } catch (error) {
      result = {
        ok: false,
        code: "command_failed",
        message: error instanceof Error ? error.message : String(error),
        retryable: false,
      };
    }
    if (result.ok) {
      approvals = approvals.filter((item) => item.approval.id !== id);
    } else {
      approvals = approvals.map((item) => item.approval.id === id
        ? { ...item, state: "failed", error: result }
        : item);
    }
    release();
    if (result.ok) {
      void refreshAll();
      void loadInbox();
    }
    void syncWindowSize();
  }

  async function allowLowRiskApprovals(): Promise<void> {
    const lows = approvals.filter(
      (item) => item.approval.risk === "low" && item.state !== "pending",
    );
    for (const item of lows) {
      // 逐条发送，避免一次失败后前端状态与后端不一致。
      if (!approvals.some((current) => current.approval.id === item.approval.id)) continue;
      await respondToApproval(item.approval.id, true);
    }
  }

  /** 岛内任务模式的输出：key 是任务 id（`codex exec --json` 的事件流累积） */
  const taskOutputs = new Map<string, { lines: string[]; done: boolean; failed: boolean }>();

  async function runActionCommand(command: Parameters<AgentIslandBridge["runCommand"]>[0]): Promise<CommandResult> {
    if (command.name === "run_task") {
      if (!bridge.startAgentTask) {
        return { ok: false, code: "task_mode_unsupported", message: "当前宿主不支持岛内任务模式", retryable: false };
      }
      try {
        const id = await bridge.startAgentTask(command.prompt);
        taskOutputs.set(id, { lines: [], done: false, failed: false });
        return { ok: true, value: id };
      } catch (cause) {
        return { ok: false, code: "task_start_failed", message: cause instanceof Error ? cause.message : String(cause), retryable: true };
      }
    }
    if (command.name === "get_send_output") {
      const task = taskOutputs.get(command.taskId);
      if (task) return { ok: true, value: { done: task.done, lines: [...task.lines], failed: task.failed } };
    }
    const result = await bridge.runCommand(command);
    if (result.ok && (command.name === "stop_agent" || command.name === "restart_agent")) {
      await store.refresh();
      if (!disposed) void loadDiagnostics();
    }
    return result;
  }

  function setPrivacy(value: boolean): void {
    privacy = value;
    localStorage.setItem("agent-island-privacy", value ? "1" : "0");
  }

  /** 执行命令面板里选中的一条：Agent / 会话 / 固定动作 */
  /** 固定动作的处理表：用 Record<PaletteActionId, …> 让"漏掉一条命令"变成编译错误。 */
  const paletteHandlers: Record<PaletteActionId, () => void> = {
    "action:terminal": () => void toggleTerminal(),
    "action:refresh": () => void refreshAll(),
    "action:privacy": () => setPrivacy(!privacy),
    "action:focus": () => setFocusMode(focusMode === "off" ? "errors" : focusMode === "errors" ? "quiet" : "off"),
    "action:pin": () => setPinnedAgent(pinnedAgentId ? null : selectedAgent?.id ?? null),
    "action:onboarding": () => {
      showWelcome = true;
    },
    "action:hide": () => hideIsland(),
  };

  function runPaletteCommand(command: PaletteCommand): void {
    paletteOpen = false;
    if (command.kind === "agent") {
      const id = command.id.slice("agent:".length);
      inspectedSession = null;
      store.selectAgent(id);
      return;
    }
    if (command.kind === "session") {
      if (!selectedAgent) return;
      const sessionId = command.id.slice("session:".length);
      inspectedSession = sessionId === selectedAgent.active_session?.id
        ? null
        : { agentId: selectedAgent.id, sessionId };
      return;
    }
    paletteHandlers[command.id as PaletteActionId]?.();
  }

  function setFocusMode(value: "off" | "errors" | "quiet"): void {
    focusMode = value;
    localStorage.setItem("agent-island-focus", value);
    if (value === "errors") {
      const errorAgent = store.current().snapshot?.agents.find((agent) =>
        agent.display_status === "error" && !agent.freshness.stale,
      );
      if (errorAgent) store.selectAgent(errorAgent.id);
    }
  }

  function setAppearance(value: Appearance): void {
    appearance = value;
    localStorage.setItem(APPEARANCE_STORAGE_KEY, value);
    void syncWindowSize();
  }

  function setIconStyle(value: "dot" | "pixel"): void {
    iconStyle = value;
    localStorage.setItem("agent-island-icon-style", value);
    void syncWindowSize();
  }

  function beginLayoutInteraction(): void {
    if (releaseLayoutDrag) return;
    releaseLayoutDrag = store.beginInteraction("drag");
  }

  function endLayoutInteraction(): void {
    const release = releaseLayoutDrag;
    releaseLayoutDrag = null;
    release?.();
  }

  function persistLayout(next: LayoutConfigV1): void {
    layout = next;
    const result = saveLayout(localStorage, next);
    layoutPersistenceFailed = !result.ok;
    if (result.ok) layout = result.value;
    void syncWindowSize();
  }

  async function safeRunCommand(command: Parameters<AgentIslandBridge["runCommand"]>[0]): Promise<CommandResult> {
    try {
      return await bridge.runCommand(command);
    } catch (error) {
      return {
        ok: false,
        code: "command_failed",
        message: error instanceof Error ? error.message : String(error),
        retryable: true,
      };
    }
  }

  async function loadHookStatus(): Promise<void> {
    if (hookState === "pending" || hookEnabled !== null) return;
    hookState = "pending";
    hookError = null;
    const result = await safeRunCommand({ name: "get_hook_status" });
    if (result.ok && typeof result.value === "boolean") {
      hookEnabled = result.value;
      hookState = "succeeded";
    } else {
      hookState = "failed";
      hookError = result.ok
        ? { code: "invalid_hook_status", message: "事件状态返回格式无效", retryable: true }
        : result;
    }
    void syncWindowSize();
  }

  async function toggleHook(): Promise<void> {
    if (hookState === "pending") return;
    if (hookEnabled === null) {
      await loadHookStatus();
      return;
    }
    const next = !hookEnabled;
    hookState = "pending";
    hookError = null;
    const result = await safeRunCommand({ name: "set_hook_enabled", enabled: next });
    if (result.ok) {
      hookEnabled = next;
      hookState = "succeeded";
    } else {
      hookState = "failed";
      hookError = result;
    }
    void syncWindowSize();
  }

  onMount(() => {
    // 回执里展开一条长记录会改变面板高度：让窗口重新量一次
    const relayout = () => { void syncWindowSize(); };
    window.addEventListener("agent-island-layout", relayout);
    try {
      if (!localStorage.getItem(INBOX_SEEN_KEY)) {
        localStorage.setItem(INBOX_SEEN_KEY, String(inboxSeenAt));
      }
    } catch {
      // localStorage 不可用时只影响摘要判断。
    }
    // 版本号来自 Tauri；拿不到就保持"未知"，不阻塞任何功能
    if (bridge.getAppVersion) {
      void bridge.getAppVersion().then((version) => {
        appVersion = version;
        appUpdate = initialAppUpdateState(version);
      }).catch(() => undefined);
    }
    let stopListening: (() => void) | undefined;
    let stopInbox: (() => void) | undefined;
    let stopPlacement: (() => void) | undefined;
    if (bridge.initializeWindowPlacement) {
      void bridge.initializeWindowPlacement(windowFailure).then((release) => {
        if (disposed) release(); else stopPlacement = release;
      }).catch(windowFailure).finally(() => {
        windowPlacementReady = true;
        if (!disposed) void syncWindowSize();
      });
    }
    const unsubscribe = store.subscribe((next) => {      const firstAgents = !viewState.snapshot?.agents.length && !!next.snapshot?.agents.length;
      if (focusMode === "errors") {
        const errorAgent = next.snapshot?.agents.find((agent) =>
          agent.display_status === "error" && !agent.freshness.stale,
        );
        if (errorAgent && errorAgent.id !== next.selectedAgentId) {
          store.selectAgent(errorAgent.id);
          return;
        }
      }
      viewState = next;
      if (firstAgents) selectDefaultAgent();
      void syncWindowSize();
    });
    void bridge.listenHookEvents(handleHookEvent).then((unlisten) => {
      if (disposed) unlisten();
      else stopListening = unlisten;
    }).catch(() => {
      if (disposed) return;
      listenerError = true;
      expanded = true;
      void syncWindowSize();
    });
    void loadInbox();
    if (bridge.listenInboxChanges) {
      void bridge.listenInboxChanges(() => { if (!disposed) void loadInbox(); }).then((unlisten) => {
        if (disposed) unlisten();
        else stopInbox = unlisten;
      }).catch(() => undefined);
    }
    // 收起岛会卸载发送框：任务记录要能从后端补回来，否则一收一开就丢了
    if (bridge.listAgentTasks) {
      void bridge.listAgentTasks().then((tasks) => {
        for (const task of tasks) {
          taskOutputs.set(task.id, {
            lines: task.message ? [task.message] : [],
            done: task.state !== "running",
            failed: task.state === "failed",
          });
        }
      }).catch(() => undefined);
    }
    void refreshAll();
    // 终端输出由 App 常驻订阅：收起面板/收起岛期间也不会丢字节，重新展开接得上
    let stopTerminalEvents: (() => void) | undefined;
    if (bridge.listenTerminalEvents) {
      void bridge.listenTerminalEvents((event) => {
        if (event.id !== terminalId) return;
        if (event.kind === "data") {
          terminalText += event.data;
          if (terminalText.length > 200_000) terminalText = terminalText.slice(-150_000);
        } else {
          terminalExited = true;
          terminalId = null;
    ptySize = null;
          void syncWindowSize();
        }
      }).then((unlisten) => {
        if (disposed) unlisten();
        else stopTerminalEvents = unlisten;
      }).catch(() => undefined);
    }
    // 展开时点了外面（窗口失焦）就收起：先回到胶囊，鼠标本来就在外面，很快自动隐藏。
    // 延迟 220ms 并复查 document.hasFocus()：原生下拉弹层会短暂抢焦点，不能因此误收起。
    let blurTimer: number | undefined;
    const cancelBlur = () => {
      if (blurTimer !== undefined) {
        window.clearTimeout(blurTimer);
        blurTimer = undefined;
      }
    };
    let stopWindowFocus: (() => void) | undefined;
    if (bridge.onWindowFocusChange) {
      void bridge.onWindowFocusChange((focused) => {
        if (focused) {
          cancelBlur();
          return;
        }
        if (disposed || !expanded) return;
        cancelBlur();
        blurTimer = window.setTimeout(() => {
          blurTimer = undefined;
          if (disposed || !expanded) return;
          collapseToPeek();
        }, 220);
      }).then((unlisten) => {
        if (disposed) unlisten();
        else stopWindowFocus = unlisten;
      }).catch(() => undefined);
    }
    // 全局快捷键（Ctrl+Shift+A）：展开态再按一次直接隐藏，隐藏/胶囊态按一次展开
    let stopHotkey: (() => void) | undefined;
    let stopAgentTask: (() => void) | undefined;
    if (bridge.listenIslandHotkey) {
      void bridge.listenIslandHotkey(() => {
        if (disposed) return;
        if (expanded) hideIsland();
        else expandIsland();
      }).then((unlisten) => {
        if (disposed) unlisten();
        else stopHotkey = unlisten;
      }).catch(() => undefined);
    }
    // 任务事件：把 codex 的 JSONL 事件累积成"发送回执"，结束时刷新一次快照
    if (bridge.onAgentTask) {
      void bridge.onAgentTask((task) => {
        const entry = taskOutputs.get(task.id) ?? { lines: [], done: false, failed: false };
        if (task.message) entry.lines = [...entry.lines, task.message];
        entry.done = task.state !== "running";
        taskOutputs.set(task.id, entry);
        if (entry.done) void refreshAll();
        void syncWindowSize();
      }).then((unlisten) => {
        if (disposed) unlisten();
        else stopAgentTask = unlisten;
      }).catch(() => undefined);
    }
    return () => {
      disposed = true;
      if (resizeSyncTimer !== undefined) window.clearTimeout(resizeSyncTimer);
      window.removeEventListener("agent-island-layout", relayout);
      diagnosticRequest += 1;
      diagnosticRefreshQueued = false;
      resizeRevision += 1;
      stopListening?.();
      stopInbox?.();
      stopPlacement?.();
      stopTerminalEvents?.();
      stopWindowFocus?.();
      stopHotkey?.();
      stopAgentTask?.();
      cancelBlur();
      for (const timer of notificationTimers.values()) window.clearTimeout(timer.handle);
      notificationTimers.clear();
      endLayoutInteraction();
      clearPeekTimer();
      unsubscribe();
    };
  });

  // 轮询：有 Agent 在干活用基础间隔，全部静止时降到 3 倍（配合增量索引，空闲 CPU 明显下降）
  $effect(() => {
    const interval = effectivePollMs;
    if (interval <= 0) return;
    const handle = window.setInterval(() => void refreshAll(), interval);
    return () => window.clearInterval(handle);
  });
</script>
<svelte:window
  onresize={handleWindowResize}
  onkeydown={(event) => {
    // Ctrl/Cmd+K：命令面板（岛可能还是隐藏态，先展开再打开）
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
      event.preventDefault();
      if (!expanded) expandIsland();
      paletteOpen = !paletteOpen;
      return;
    }
    if (event.key === "Escape" && paletteOpen) { event.preventDefault(); paletteOpen = false; return; }
    if (event.key === "Escape" && (expanded || peeked)) { event.preventDefault(); hideIsland(); }
  }}
/>
<!-- 悬停检测放在 body 上：pointerenter 不冒泡，挂在 window 上收不到 -->
<svelte:body
  onpointerenter={handleIslandPointerEnter}
/>

{#if viewState.snapshot}
  <main
    class={`agent-island status-${selectedAgent?.display_status ?? "idle"}`}
    class:expanded
    class:peeked
    class:idle-hidden={idleHidden}
    class:privacy
    data-testid="agent-island"
    data-presence={idleHidden ? "hidden" : expanded ? "expanded" : "peek"}
    data-theme-style={appearance}
    aria-label="Agent Island"
    bind:this={islandElement}
  >
    <span class="sr-only" data-testid="selected-agent-id">{viewState.selectedAgentId ?? ""}</span>
    {#if idleHidden}
      <div class="idle-notch" data-testid="idle-notch" data-pending={hasPendingApprovals || inboxCount > 0 ? "true" : "false"} aria-hidden="true"><span class="idle-notch-dot"></span></div>
    {:else}
    <div class="compact-header" class:floating={!expanded}>
      <CompactIsland agent={selectedAgent} agents={visibleAgents} {expanded} onToggle={toggleExpanded} {iconStyle} pendingApprovals={hasPendingApprovals} inboxCount={inboxCount} binding={selectedAgent?.binding ?? "inferred"} />
      {#if expanded && bridge.startWindowDrag}<button class="window-handle" aria-label="拖动窗口" title="按住并拖动窗口" onpointerdown={(event) => { if (event.button === 0) { event.preventDefault(); void windowAction(() => bridge.startWindowDrag!()); } }}>
        <svg viewBox="0 0 12 12" width="12" height="12" fill="currentColor" aria-hidden="true"><circle cx="3" cy="3" r="1"/><circle cx="6" cy="3" r="1"/><circle cx="9" cy="3" r="1"/><circle cx="3" cy="6" r="1"/><circle cx="6" cy="6" r="1"/><circle cx="9" cy="6" r="1"/></svg>
      </button>{/if}
    </div>
    {/if}
    {#if expanded}
      {#if windowError}<p class="refresh-note" role="alert">{windowError}</p>{/if}
      <section id="expanded-island" class="adaptive-details" class:terminal-max={terminalOpen && terminalMax} aria-label="Agent 详情">
        {#if showWelcome}
          <WelcomeCard onDismiss={dismissWelcome} />
        {/if}
        <InboxPanel
          items={inboxItems}
          digest={inboxDigest}
          onDismissDigest={dismissInboxDigest}
          onResolve={(id) => void resolveInbox(id)}
          onSnooze={(id, minutes) => void snoozeInbox(id, minutes)}
          onOpenApproval={focusApproval}
        />
        <TeamStrip {bridge} />
        <nav class="agent-strip" data-testid="agent-strip" aria-label="切换 Agent">
          {#each visibleAgents as agent (agent.id)}
            <button
              type="button"
              class:active={agent.id === selectedAgent?.id}
              data-agent-id={agent.id}
              aria-pressed={agent.id === selectedAgent?.id}
              onclick={() => { inspectedSession = null; searchedSession = null; searchedMeta = null; store.selectAgent(agent.id); }}
              onkeydown={(event) => navigateAgentStrip(event)}
            ><span class={`mini-dot status-${agent.display_status}`} aria-hidden="true"></span><span class="sensitive">{agent.name}</span></button>
          {/each}
          <button
            type="button"
            class="agent-add"
            data-testid="agent-add"
            aria-label="检测本机或添加 Agent"
            title="检测本机 Agent；没有想要的可以自己添加"
            aria-expanded={addAgentOpen}
            onclick={() => void toggleAddAgent()}
          ><svg viewBox="0 0 12 12" width="11" height="11" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" aria-hidden="true"><path d="M6 2.5v7M2.5 6h7" /></svg></button>
        </nav>
        {#if addAgentOpen}
          <section class="agent-add-panel" data-testid="agent-add-panel" aria-label="检测或添加 Agent">
            <div class="add-head">
              <strong>检测 / 添加 Agent</strong>
              <button type="button" class="add-close" aria-label="关闭" onclick={() => { addAgentOpen = false; void syncWindowSize(); }}><svg viewBox="0 0 12 12" width="10" height="10" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" aria-hidden="true"><path d="M3 3l6 6M9 3l-6 6" /></svg></button>
            </div>
            <p class="add-hint">已检测本机已安装的 Agent；下面是结果。</p>
            {#if addAgentStatus === "loading"}
              <p class="add-empty">正在检测本机…</p>
            {:else if addAgentCandidates.filter((candidate) => candidate.installed).length === 0}
              <p class="add-empty">本机没有检测到新的 Agent。</p>
            {:else}
              <div class="candidate-list">
                {#each addAgentCandidates.filter((candidate) => candidate.installed) as candidate (candidate.name)}
                  <div class="candidate" data-testid="agent-candidate" data-configured={candidate.configured}>
                    <span class="candidate-main">
                      <strong>{candidate.name}</strong>
                      <span class="candidate-path sensitive" title={candidate.path ?? ""}>{candidate.path ?? "未找到可执行文件"}</span>
                    </span>
                    {#if candidate.configured}
                      <span class="candidate-tag">已添加</span>
                    {:else}
                      <button type="button" data-testid="agent-candidate-add" disabled={addAgentSaving} onclick={() => void addDetectedAgent(candidate)}>添加</button>
                    {/if}
                  </div>
                {/each}
              </div>
            {/if}
            <form class="manual-add" onsubmit={(event) => { event.preventDefault(); void addManualAgent(); }}>
              <strong>如没有想要的，可自己添加</strong>
              <input
                data-testid="agent-manual-name"
                aria-label="Agent 名称"
                placeholder="名称，例如 Gemini CLI"
                value={addAgentName}
                oninput={(event) => { addAgentName = event.currentTarget.value; }}
              />
              <input
                data-testid="agent-manual-program"
                aria-label="命令或可执行文件路径"
                placeholder="命令或完整路径，例如 gemini"
                value={addAgentProgram}
                oninput={(event) => { addAgentProgram = event.currentTarget.value; }}
              />
              <button type="submit" data-testid="agent-manual-add" disabled={addAgentSaving}>{addAgentSaving ? "添加中…" : "添加"}</button>
            </form>
            {#if addAgentError}<p class="add-error" role="alert" data-testid="agent-add-error">{addAgentError}</p>{/if}
          </section>
        {/if}
        {#if selectedAgent && selectedAgent.has_log_adapter === false}
          <p class="adapter-note" data-testid="adapter-note" role="status">
            「{selectedAgent.name}」是自定义 Agent：灵动岛只监控进程状态；会话、日志与用量需要对应的日志适配器。
          </p>
        {:else if coldStart && selectedAgent}
          <div class="cold-start" data-testid="cold-start" role="status">
            <p>{selectedAgent.state.process === "running"
              ? `已检测到 ${selectedAgent.name} 在运行，正在等它写出第一条会话记录…`
              : `还没有 ${selectedAgent.name} 的会话。灵动岛只显示能确认身份的当前会话；在终端里跑一轮，或直接在岛内启动它。`}</p>
            {#if selectedAgent.state.process !== "running" && terminalTarget(selectedAgent).label !== "PowerShell"}
              <button type="button" class="cold-start-action" data-testid="cold-start-launch" onclick={() => void launchAgentInTerminal()}>在岛内启动 {terminalTarget(selectedAgent).label}</button>
            {/if}
          </div>
        {/if}
        <div class="cards-region" class:terminal-open={terminalOpen}>
        {#if selectedAgent}
          <LayoutEditor
            bind:layout
            emptyCardIds={[...(!selectedSession ? ["session", "log"] as CardId[] : []), ...(!selectedAgent.usage ? ["usage"] as CardId[] : [])]}
            onLayoutChange={persistLayout}
            beginInteraction={beginLayoutInteraction}
            endInteraction={endLayoutInteraction}
          >
            {#snippet content(placement: CardPlacement)}
              {#if placement.id === "status"}
                <CardBoundary cardName="状态卡片" errorCode="status_card_render_failed">
                  <StatusCard agent={selectedAgent} session={selectedSession} inspectingHistory={viewingArchive} />
                </CardBoundary>
              {:else if placement.id === "usage"}
                <CardBoundary cardName="用量卡片" errorCode="usage_card_render_failed">
                  <UsageCard agent={selectedAgent} refreshFailed={viewState.refreshError !== null} todaySpend={dailyTotal(spend, Date.now())} budget={dailyBudget} />
                </CardBoundary>
              {:else if placement.id === "session"}
                <CardBoundary cardName="会话卡片" errorCode="session_card_render_failed">
                  {#if selectedSession}<SessionCard session={selectedSession} {privacy} binding={selectedAgent?.binding ?? "inferred"} />
                  {:else}<div class="empty" class:quiet={quietEmpty} role="status">还没有会话：目录与会话信息会在 Agent 跑出第一条记录后出现。</div>{/if}
                </CardBoundary>
              {:else if placement.id === "log"}
                <CardBoundary cardName="工作回执" errorCode="log_card_render_failed">
                  {#if selectedSession}<LogCard session={selectedSession} />
                  {:else}<div class="empty" class:quiet={quietEmpty} data-testid="empty-session" role="status"><strong>无法确认当前会话</strong><span>灵动岛不会用历史对话冒充当前任务；启动并跑一轮后，这里会出现工作回执。</span></div>{/if}
                </CardBoundary>
              {:else}
                <article class="stats-card" aria-label="当前 Agent 统计">
                  <div class="line">
                    <span class="key">统计</span>
                    <div class="stats-values">
                      <span><strong data-testid="stats-event-count">{inspectingHistory ? "—" : selectedAgent.diagnostic?.event_count ?? "—"}</strong>事件</span>
                      <span><strong data-testid="stats-message-count">{inspectingHistory ? selectedSession?.records.length ?? "—" : selectedAgent.diagnostic?.message_count ?? "—"}</strong>消息</span>
                      <span><strong>{selectedAgent.history_sessions.length + (selectedAgent.active_session ? 1 : 0)}</strong>会话</span>
                    </div>
                  </div>
                </article>
              {/if}
            {/snippet}
          </LayoutEditor>
          {#if layoutPersistenceFailed}
            <p class="layout-persistence-status" data-testid="layout-persistence-status" role="status">布局已应用，但暂时未保存；下次调整时会重试。</p>
          {/if}
        {:else if visibleAgents.length === 0}
          <div class="empty" data-testid="empty-agents" role="status">
            <strong>{agentMode === "running" ? "当前没有运行中的 Agent" : "未检测到任何 Agent"}</strong>
            <span>{agentMode === "running" ? "设置里把「显示」切成「全部检测」，即可看到已安装或有本地记录的 Agent。" : "启动一个 Agent 后会自动出现在这里。"}</span>
          </div>
        {:else}
          <div class="empty" data-testid="empty-session" role="status"><strong>无法确认当前会话</strong><span>为避免串入历史对话，灵动岛不会显示其他会话内容。</span></div>
        {/if}
        </div>
        {#if selectedAgent}
          <div class="composer-slot">
            <PromptComposer
              agent={selectedAgent}
              session={selectedSession}
              runCommand={runActionCommand}
              onLayoutChange={() => void syncWindowSize()}
              disabledReason={viewingArchive
                ? "正在查看本地历史记录，不能向历史会话发送指令"
                : selectedAgent?.has_log_adapter === false
                  ? "该 Agent 没有日志适配器，无法发送指令或读取会话"
                  : null}
              taskCapable={selectedAgent?.id === "codex" && !selectedAgent?.active_session}
            />
          </div>
        {/if}
        {#if selectedAgent && (selectedAgent.history_sessions.length > 0 || inspectingHistory)}
          <label class="session-picker">查看会话
            <select data-testid="session-picker" value={inspectingHistory ? inspectedSession?.sessionId : ""} onchange={(event) => {
              inspectedSession = event.currentTarget.value ? { agentId: selectedAgent!.id, sessionId: event.currentTarget.value } : null;
              void syncWindowSize();
            }}>
              <option value="">自动：可靠匹配的当前会话</option>
              {#if inspectingHistory && !selectedSession}<option value={inspectedSession?.sessionId} disabled>所选记录已不可用</option>{/if}
              {#each selectedAgent.history_sessions as session (session.id)}
                <option value={session.id}>本地记录 · {privacy ? "内容已隐藏" : `${session.name} · ${session.id}`}</option>
              {/each}
            </select>
          </label>
        {#if bridge.bindSession && selectedAgent}
          {#if selectedAgent.binding === "manual"}
            <button type="button" class="link-button" data-testid="unbind-session" onclick={() => void unbindSelectedSession()}>解除绑定{selectedSession ? `（${selectedSession.id.slice(0, 8)}）` : ""}</button>
          {:else if selectedSession}
            <button type="button" class="link-button" data-testid="bind-session" onclick={() => void bindSelectedSession()}>绑定此会话</button>
          {/if}
        {/if}
        {#if searchedSession}
          <p class="refresh-note" role="status" data-testid="searched-session-note">
            正在查看搜索结果（{searchedMeta?.agent ?? "会话"} · {searchedSession.name}），不代表桌面端当前任务。
            <button type="button" class="link-button" data-testid="searched-session-close" onclick={closeSearchedSession}>返回当前会话</button>
          </p>
        {/if}          {#if inspectingHistory}<p class="refresh-note" role="status" data-testid="history-inspection-note">正在查看手动选择的本地会话记录，不代表桌面端当前任务。{selectedSession ? "目录和终端操作使用此会话；停止、重启和跳回已禁用。" : "该记录已不在当前快照中，请重新选择。"}</p>{/if}
        {/if}
        {#if viewState.refreshError}<p class="refresh-note" role="status">数据刷新暂时失败，已保留最近状态。</p>{/if}
        {#if selectedAgent}
          <div class="terminal-toolbar">
            <button
              type="button"
              class="terminal-toggle"
              class:active={terminalOpen}
              data-testid="terminal-toggle"
              aria-pressed={terminalOpen}
              onclick={() => void toggleTerminal()}
            >{terminalOpen ? "收起终端" : "内置终端"}</button>
            {#if agentPackage}
              <button
                type="button"
                class="update-button"
                class:available={updateAvailable}
                data-testid="agent-update"
                data-update-state={updateStatus}
                disabled={updateRunning || updateStatus === "checking"}
                title={updateMessage
                  ?? (updateInstalled && updateLatest
                    ? `本地 v${updateInstalled} → npm 最新 v${updateLatest}`
                    : "检查 Agent CLI 是否有新版本（需要联网）")}
                onclick={() => void (updateAvailable ? runAgentUpdate() : checkAgentUpdate(true))}
              >{updateLabel}</button>
            {/if}
            {#if terminalOpen}
              <span class="terminal-hint">岛内终端：{terminalTarget(selectedAgent).label}</span>
            {/if}
          </div>
          {#if updateMessage}
            <p class="update-message" data-testid="agent-update-message" role="status">{updateMessage}</p>
          {/if}
          {#if terminalOpen}
            <TerminalPanel
              agent={selectedAgent}
              sessionId={terminalId}
              program={terminalProgram}
              text={terminalText}
              starting={terminalStarting}
              exited={terminalExited}
              error={terminalError}
              maximized={terminalMax}
              binding={selectedAgent?.binding ?? "inferred"}
              onToggleMax={() => void toggleTerminalMax()}
              onStart={(program) => void startTerminal(program)}
              onStop={() => void stopTerminal()}
              onInput={(data) => { if (terminalId) void bridge.terminalWrite?.(terminalId, data).catch(() => undefined); }}
              onResize={(cols, rows) => {
                terminalSize = { cols, rows };
                if (!terminalId) return;
                // PTY 现在的尺寸由 App 记住（面板会重挂载，不能把"已上报"存在面板里）。
                // 尺寸没变就绝不再通知后端：ConPTY 每变一次，TUI 就把整屏重打一遍。
                if (ptySize && ptySize.cols === cols && ptySize.rows === rows) return;
                ptySize = { cols, rows };
                void bridge.terminalResize?.(terminalId, cols, rows).catch(() => undefined);
              }}
              onLayoutChange={() => void syncWindowSize()}
            />
          {/if}
        {/if}

        <div class="system">
        <details class="diagnostics-drawer" data-testid="diagnostics-drawer" ontoggle={(event) => { diagnosticsOpen = event.currentTarget.open; if (diagnosticsOpen) { void loadDiagnostics(); void loadAgentDetections(); } }}>
          <summary>
            <span>诊断信息</span>
            {#if agentDiagnosticIssues.length > 0}<span class="drawer-badge" data-testid="diagnostics-count">{agentDiagnosticIssues.length} 项提示</span>{:else}<span class="drawer-muted">无异常</span>{/if}
          </summary>
          <DiagnosticsPanel
            issues={agentDiagnosticIssues}
            sources={agentDiagnosticSources}
            status={diagnosticStatus}
            exportDiagnostics={(destination) => diagnosticsBridge.exportDiagnostics(destination)}
            onLayoutChange={() => void syncWindowSize()}
          />
          {#if bridge.detectAgents || bridge.refreshAgentDetection}
            <div class="agent-detection" data-testid="agent-detection">
              <div class="detection-head">
                <span>Agent 检测 <small>（只显示本机找得到的）</small></span>
                <button type="button" data-testid="agent-detection-refresh" onclick={() => void loadAgentDetections(true)}>重新检测</button>
              </div>
              {#if detectionStatus === "failed"}
                <p class="detection-empty" role="alert">检测失败，请稍后重试</p>
              {:else if agentDetections.length === 0}
                <p class="detection-empty">{detectionStatus === "loading" ? "正在检测…" : "暂无检测结果"}</p>
              {:else}
                {#each agentDetections as detection (detection.id)}
                  <div
                    class="detection-row"
                    data-testid="agent-detection-row"
                    data-agent-id={detection.id}
                    data-installed={detection.installed}
                    data-running={detection.running}
                  >
                    <span class="detection-dot" class:on={detection.installed} aria-hidden="true"></span>
                    <strong>{detection.name}</strong>
                    <span class="detection-path sensitive" title={detection.path ?? ""}>{detection.path ?? "未找到可执行文件"}</span>
                    <span class="detection-tag" title={detectionTitle(detection)}>{detectionTag(detection)}</span>
                  </div>
                {/each}
              {/if}
            </div>
          {/if}
        </details>
        <section class="about-row" data-testid="about-row" aria-label="版本与更新">
          <span>版本 <strong data-testid="app-version">{appVersion ? `v${appVersion}` : "未知"}</strong></span>
          <button type="button" data-testid="app-update-check" disabled={appUpdate.status === "checking" || !appVersion} onclick={() => void checkAppRelease()}>
            {appUpdate.status === "checking" ? "检查中…" : "检查更新"}
          </button>
          {#if (appUpdate.status === "available" || appUpdate.status === "installing") && appUpdate.latest}
            {#if bridge.installAppUpdate}
              <button
                type="button"
                class="primary"
                data-testid="app-update-install"
                data-progress={appUpdate.progress ?? ""}
                disabled={appUpdate.status === "installing"}
                onclick={() => void installAppRelease()}
              >{appUpdate.status === "installing" ? "下载中…" : "下载并安装"}</button>
            {:else if bridge.openExternal}
              <button type="button" class="primary" data-testid="app-update-open" onclick={() => void openReleasePage()}>打开下载页</button>
            {/if}
          {/if}
          {#if appUpdate.message}
            <span class="about-message" data-testid="app-update-message" role="status">{appUpdate.message}</span>
          {/if}
        </section>        {#if bridge.searchSessions}
          <GlobalSearch {bridge} onOpen={(hit) => void openSearchedSession(hit)} />
        {/if}
        {#if bridge.listApprovalRules}
          <ApprovalRules {bridge} />
        {/if}
        </div>
      </section>
      {#key JSON.stringify([selectedAgent?.id, inspectingHistory, selectedSession?.id, selectedSession?.cwd])}
        <ActionBar
          agent={selectedAgent}
          session={selectedSession}
          {inspectingHistory}
          runCommand={runActionCommand}
          onLayoutChange={() => void syncWindowSize()}
          {privacy}
          {focusMode}
          {appearance}
          {iconStyle}
          {agentMode}
          {hookEnabled}
          {hookState}
          {hookError}
          onPrivacy={setPrivacy}
          onFocusMode={setFocusMode}
          onAppearance={setAppearance}
          budget={dailyBudget}
          onBudget={(value) => {
            dailyBudget = value;
            localStorage.setItem(BUDGET_STORAGE_KEY, String(value));
          }}
          pinnedAgentName={pinnedAgentId ? selectedAgent?.name ?? pinnedAgentId : null}
          onTogglePin={() => setPinnedAgent(pinnedAgentId ? null : selectedAgent?.id ?? null)}
          onIconStyle={setIconStyle}
          onAgentMode={setAgentMode}
          onHookToggle={() => void toggleHook()}
          onSettingsOpen={() => void loadHookStatus()}
          onAdjustWidth={bridge.adjustWindowWidth ? (delta) => void windowAction(() => bridge.adjustWindowWidth!(delta, expandedMinWidth)) : undefined}
          onStartResize={bridge.startWindowResize ? () => windowAction(() => bridge.startWindowResize!()) : undefined}
        />
      {/key}
    {/if}
  </main>
  {#if expanded}
    <aside
      class="event-layer"
      class:empty={approvals.length === 0 && notifications.length === 0 && !listenerError}
      class:privacy
      data-testid="event-layer"
      data-theme-style={appearance}
      aria-label="紧急事件"
      bind:this={eventLayerElement}
    >
      <ApprovalStack
        items={approvals}
        onRespond={(id, allow, scope, reason) => void respondToApproval(id, allow, scope, reason)}
        onBatchAllowLowRisk={() => void allowLowRiskApprovals()}
        onRemember={bridge.addApprovalRule ? (tool, pattern) => void rememberApprovalRule(tool, pattern) : undefined}
      />
      {#if listenerError}<p class="listener-error" role="alert">事件监听连接失败，请重启灵动岛后重试。</p>{/if}
      <NotificationStack items={notifications} onDismiss={dismissNotification} onPause={pauseNotification} />
    </aside>
  {/if}
  <!-- 命令面板挂最外层：岛自身有 backdrop-filter，会给 fixed 后代建立新的包含块 -->
  {#if paletteOpen}
    <CommandPalette commands={paletteCommands} onRun={runPaletteCommand} onClose={() => (paletteOpen = false)} {appearance} />
  {/if}
{:else}
  <div class="island-loading" data-theme-style={appearance} aria-live="polite" bind:this={islandElement}>
    {#if viewState.refreshError}
      <span data-testid="connect-error">连接失败：{viewState.refreshError.message}</span>
    {:else}
      正在连接 Agent Island…
    {/if}
  </div>
{/if}

<style>
  /*
   * 岛的外壳：一块圆角矩形板，没有模糊、没有大阴影，只有一条 1px 边和一条内发丝。
   * 面板内部由发丝线分区，不再用卡片盒子套卡片。
   */
  .agent-island,
  .island-loading {
    box-sizing: border-box;
    color: var(--ink);
    font: 400 var(--fs-base)/1.5 var(--font-ui);
    font-feature-settings: "tnum" 0;
  }

  /*
   * 贴边刘海：窗口就是这块形状本身，顶边与屏幕工作区顶边齐平，
   * 只有下缘是圆的。收起时宽度由内容决定（fit-content），展开时占满面板宽度。
   */
  .agent-island {
    width: fit-content;
    max-width: 100%;
    margin-inline: auto;
    overflow: hidden;
    border: 1px solid var(--rule);
    border-top: 0;
    border-radius: 0 0 var(--radius-island) var(--radius-island);
    background: var(--ground);
    isolation: isolate;
  }

  .agent-island.expanded { width: 100%; margin-inline: 0; }

  /* 自绘滚动条：原生 scrollbar-color 会让 Chromium 用方形轨道盖住圆角 */
  .agent-island.expanded { max-height: 100vh; overflow-y: auto; background-clip: padding-box; }
  .agent-island.expanded::-webkit-scrollbar { width: 8px; background: transparent; }
  .agent-island.expanded::-webkit-scrollbar-track { margin: 22px 0; background: transparent; }
  .agent-island.expanded::-webkit-scrollbar-thumb { border-radius: 999px; background: var(--scrollbar-thumb); }

  /* 顶部胶囊：始终吸顶；展开时它变成整块面板的抬头 */
  .compact-header { display: flex; align-items: center; min-width: 0; }
  .agent-island.expanded > .compact-header { position: sticky; top: 0; z-index: var(--z-sticky); background: var(--sticky); border-bottom: 1px solid var(--rule); }

  .window-handle {
    flex: 0 0 auto;
    display: grid;
    place-items: center;
    width: 26px;
    min-height: 26px;
    margin-right: 10px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-control);
    color: var(--ink-3);
    background: transparent;
    cursor: move;
    touch-action: none;
  }
  .window-handle:hover { color: var(--ink); background: var(--field); }
  .window-handle:focus-visible { outline: 2px solid var(--ink); outline-offset: 2px; }

  /* 隐藏态：只剩一条可以忽略的刻度，鼠标扫过变宽 */
  .agent-island.idle-hidden {
    display: grid;
    place-items: center;
    overflow: hidden;
    border-color: transparent;
    background: transparent;
    cursor: default;
  }
  .idle-notch {
    display: grid;
    place-items: center;
    width: 64px;
    height: 3px;
    margin-block: 2px;
    border-radius: 2px;
    background: var(--rule-strong);
    opacity: .8;
    transition: width var(--duration-normal) var(--motion-out), background var(--duration-normal) ease-out, opacity var(--duration-normal) ease-out;
  }
  .agent-island.idle-hidden:hover .idle-notch { width: 104px; background: var(--ink-3); opacity: 1; }
  .idle-notch-dot { display: none; }
  .idle-notch[data-pending="true"] {
    width: 104px;
    height: 3px;
    background: var(--alarm);
    opacity: 1;
    animation: notch-breathe 2.4s var(--motion-out) infinite;
  }
  @keyframes notch-breathe { 0%, 100% { opacity: 1; } 50% { opacity: .55; } }
  @media (prefers-reduced-motion: reduce) { .idle-notch[data-pending="true"] { animation: none; } }

  /* 面板：内部左右留白交给直接子元素，卡片区自己贴边（发丝线才能贯穿整块板） */
  .adaptive-details {
    min-width: 0;
    display: flex;
    flex-direction: column;
    border-top: 0;
    padding: 0 0 6px;
    animation: details-enter 200ms var(--motion-out);
  }
  .adaptive-details > * { padding-inline: 14px; }
  .adaptive-details > .cards-region { padding-inline: 0; }
  .adaptive-details > .agent-strip { padding-top: 10px; padding-bottom: 4px; }

  @keyframes details-enter { from { opacity: 0; transform: translateY(-4px); } to { opacity: 1; transform: translateY(0); } }
  @media (prefers-reduced-motion: reduce) { .adaptive-details { animation: none; } }

  /* ---- Agent 芯片：一行可横滚的标签，不是导航栏 ---- */
  .agent-strip { display: flex; align-items: center; gap: 4px; overflow-x: auto; scrollbar-width: none; }
  .agent-strip::-webkit-scrollbar { display: none; }
  .agent-strip button {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-height: 26px;
    padding: 3px 8px;
    border: 0;
    border-radius: var(--radius-control);
    color: var(--ink-3);
    background: transparent;
    font: 600 var(--fs-2xs)/1 var(--font-ui);
    cursor: pointer;
    transition: color var(--duration-fast) ease-out, background var(--duration-fast) ease-out;
  }
  .agent-strip button:hover { color: var(--ink-2); background: var(--field); }
  .agent-strip button.active { color: var(--ink); background: var(--field-hover); }
  .agent-strip button:focus-visible { outline: 2px solid var(--ink); outline-offset: 2px; }
  .agent-strip .agent-add { width: 26px; justify-content: center; padding: 0; color: var(--ink-3); font-size: 14px; }
  .agent-strip .agent-add:hover { color: var(--ink); }
  .mini-dot { width: 5px; height: 5px; border-radius: 1px; background: var(--state-idle); }
  .mini-dot.status-working { background: var(--state-working); }
  .mini-dot.status-done { background: var(--state-done); }
  .mini-dot.status-error { background: var(--state-error); }
  .mini-dot.status-waiting { background: var(--state-waiting); }
  .mini-dot.status-stopped { background: var(--state-stopped); }
  .mini-dot.status-working { animation: working-blink 2.4s var(--motion-out) infinite; }
  @keyframes working-blink { 50% { opacity: .35; } }
  @media (prefers-reduced-motion: reduce) { .mini-dot.status-working { animation: none; } }

  /* ---- 检测 / 添加 Agent ---- */
  .agent-add-panel {
    display: grid;
    gap: 8px;
    margin: 6px 0 10px;
    padding: 10px 0 0;
    border-top: 1px solid var(--rule);
  }
  .add-head { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  .add-head strong { font-size: var(--fs-xs); font-weight: 650; }
  .add-close { display: grid; place-items: center; width: 22px; height: 22px; padding: 0; border: 0; border-radius: 4px; color: var(--ink-3); background: transparent; cursor: pointer; }
  .add-close:hover { color: var(--ink); background: var(--field); }
  .add-hint, .add-empty { margin: 0; color: var(--ink-3); font-size: var(--fs-2xs); line-height: 1.6; }
  .candidate-list { display: grid; gap: 0; max-height: 150px; overflow-y: auto; }
  .candidate { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: center; gap: 8px; padding: 5px 0; border-top: 1px solid var(--rule); }
  .candidate:first-child { border-top: 0; }
  .candidate-main { min-width: 0; display: grid; gap: 1px; }
  .candidate-main strong { overflow: hidden; font-size: var(--fs-xs); text-overflow: ellipsis; white-space: nowrap; }
  .candidate-path, .detection-path { overflow: hidden; color: var(--ink-3); font: 400 var(--fs-2xs)/1.5 var(--font-mono); text-overflow: ellipsis; white-space: nowrap; }
  .candidate-tag { color: var(--state-done); font-size: var(--fs-2xs); }
  .candidate button, .manual-add button, .detection-head button {
    min-height: 24px;
    padding: 2px 8px;
    border: 1px solid var(--rule);
    border-radius: 5px;
    color: var(--ink-2);
    background: transparent;
    font-size: var(--fs-2xs);
    cursor: pointer;
  }
  .candidate button:hover, .manual-add button:hover, .detection-head button:hover { color: var(--ink); border-color: var(--rule-strong); }
  .manual-add { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1.2fr) auto; gap: 6px; align-items: center; padding-top: 8px; border-top: 1px solid var(--rule); }
  .manual-add strong { grid-column: 1 / -1; color: var(--ink-3); font-size: var(--fs-2xs); font-weight: 600; }
  .manual-add input {
    min-width: 0;
    height: 26px;
    padding: 3px 7px;
    border: 1px solid var(--rule);
    border-radius: 5px;
    color: var(--ink);
    background: var(--field);
    font-size: var(--fs-2xs);
  }
  .manual-add input::placeholder { color: var(--ink-3); }
  .manual-add input:focus-visible { outline: 2px solid var(--ink); outline-offset: 1px; }
  .add-error { margin: 0; color: var(--state-error); font-size: var(--fs-2xs); }

  /* ---- 系统区：诊断、版本、搜索、审批规则。它们是设置，不是状态。 ---- */
  .system { display: flex; flex-direction: column; margin-top: 8px; border-top: 1px solid var(--rule); padding-top: 0; }
  .system > * { padding-block: 0; }

  .diagnostics-drawer { border-top: 1px solid var(--rule); }
  .diagnostics-drawer > summary {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 0;
    list-style: none;
    cursor: pointer;
    color: var(--ink-3);
    font-size: var(--fs-2xs);
  }
  .diagnostics-drawer > summary::-webkit-details-marker { display: none; }
  .diagnostics-drawer > summary:focus-visible { outline: 2px solid var(--ink); outline-offset: 2px; }
  .diagnostics-drawer > summary::after { content: ""; width: 7px; height: 7px; margin-left: auto; border-right: 1px solid currentColor; border-bottom: 1px solid currentColor; transform: rotate(45deg) translateY(-2px); transition: transform var(--duration-fast) ease-out; }
  .diagnostics-drawer[open] > summary { color: var(--ink-2); }
  .diagnostics-drawer[open] > summary::after { transform: rotate(225deg) translateY(-1px); }
  .drawer-badge { padding: 1px 6px; border-radius: 3px; background: var(--alarm); color: var(--alarm-ink); font: 600 var(--fs-2xs)/1.5 var(--font-readout); }
  .drawer-muted { color: var(--ink-3); font-size: var(--fs-2xs); }

  .agent-detection { display: grid; gap: 6px; margin: 0 0 10px; padding: 8px 0 0; border-top: 1px solid var(--rule); }
  .detection-head { display: flex; align-items: center; justify-content: space-between; gap: 8px; color: var(--ink-3); font-size: var(--fs-2xs); }
  .detection-head small { color: var(--ink-3); font-size: var(--fs-2xs); }
  .detection-row { display: grid; grid-template-columns: 6px 76px minmax(0, 1fr) auto; align-items: center; gap: 8px; min-width: 0; color: var(--ink-3); font-size: var(--fs-2xs); }
  .detection-row strong { overflow: hidden; color: var(--ink-2); font-weight: 600; text-overflow: ellipsis; white-space: nowrap; }
  .detection-dot { width: 5px; height: 5px; border-radius: 1px; background: var(--state-stopped); }
  .detection-dot.on { background: var(--state-done); }
  .detection-tag { padding: 0 4px; border-radius: 3px; color: var(--ink-3); background: var(--field); font-size: 9px; white-space: nowrap; }
  .detection-empty { margin: 0; color: var(--ink-3); font-size: var(--fs-2xs); }

  /* 版本与更新：一行元信息，不抢戏 */
  .about-row {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
    padding: 8px 0;
    border-top: 1px solid var(--rule);
    color: var(--ink-3);
    font-size: var(--fs-2xs);
  }
  .about-row strong { color: var(--ink-2); font: 500 var(--fs-2xs)/1 var(--font-readout); font-variant-numeric: tabular-nums; }
  .about-row button {
    min-height: 22px;
    padding: 1px 7px;
    border: 1px solid var(--rule);
    border-radius: 4px;
    color: var(--ink-3);
    background: transparent;
    font-size: var(--fs-2xs);
    cursor: pointer;
  }
  .about-row button:hover:not(:disabled) { color: var(--ink); border-color: var(--rule-strong); }
  .about-row button.primary { color: var(--ink); border-color: var(--ink-2); }
  .about-row button:disabled { opacity: .5; cursor: progress; }
  .about-message { color: var(--ink-3); font-size: var(--fs-2xs); }

  /* ---- 查看会话 / 绑定 ---- */
  .session-picker { display: grid; gap: 5px; margin: 4px 0 8px; color: var(--ink-3); font-size: var(--fs-2xs); }
  .session-picker select {
    width: 100%;
    min-width: 0;
    padding: 6px 8px;
    border: 1px solid var(--rule);
    border-radius: var(--radius-control);
    color: var(--ink);
    background: var(--field);
    font: 400 var(--fs-2xs)/1.4 var(--font-ui);
  }
  .session-picker select:focus-visible { outline: 2px solid var(--ink); outline-offset: 2px; }
  :global(.link-button) { color: var(--ink-2); text-decoration: underline; text-underline-offset: 2px; text-decoration-color: var(--rule-strong); font-size: var(--fs-2xs); background: none; border: 0; padding: 0; cursor: pointer; }
  :global(.link-button:hover) { color: var(--ink); text-decoration-color: var(--ink-2); }

  /* ---- 内置终端 ---- */
  .terminal-toolbar { display: flex; align-items: center; gap: 8px; margin: 4px 0 6px; }
  .terminal-toggle, .update-button {
    min-height: 26px;
    padding: 3px 9px;
    border: 1px solid var(--rule);
    border-radius: var(--radius-control);
    color: var(--ink-2);
    background: transparent;
    font-size: var(--fs-2xs);
    cursor: pointer;
  }
  .terminal-toggle:hover, .update-button:hover:not(:disabled) { color: var(--ink); border-color: var(--rule-strong); }
  .terminal-toggle.active { color: var(--ground); background: var(--ink); border-color: transparent; font-weight: 650; }
  .update-button.available { color: var(--state-working); border-color: var(--state-working); }
  .update-button:disabled { opacity: .5; cursor: progress; }
  .terminal-toggle:focus-visible, .update-button:focus-visible { outline: 2px solid var(--ink); outline-offset: 2px; }
  .terminal-hint { color: var(--ink-3); font-size: var(--fs-2xs); }
  .update-message { margin: -2px 0 6px; color: var(--ink-3); font-size: var(--fs-2xs); line-height: 1.5; overflow-wrap: anywhere; }
  .cards-region.terminal-open { display: none; }
  .cards-region > .empty,
  .cards-region > .layout-persistence-status { margin-inline: 14px; }
  .adaptive-details.terminal-max .diagnostics-drawer,
  .adaptive-details.terminal-max .composer-slot,
  .adaptive-details.terminal-max .session-picker,
  .adaptive-details.terminal-max .cards-region,
  .adaptive-details.terminal-max .refresh-note.note { display: none; }

  /* ---- 提示与空态 ---- */
  /* 自定义 Agent：只有进程状态，用一条中性说明代替满屏"未确认" */
  .adapter-note {
    margin: 0 0 10px;
    padding: 8px 10px;
    border-radius: var(--radius-control);
    color: var(--ink-2);
    background: var(--field);
    font-size: var(--fs-2xs);
    line-height: 1.6;
  }
  /* 冷启动：有适配器但还没有会话，给一条引导 + 一个就地启动的入口 */
  .cold-start {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 0 0 10px;
    padding: 8px 10px;
    border-radius: var(--radius-control);
    background: var(--field);
  }
  .cold-start p {
    flex: 1 1 auto;
    min-width: 0;
    margin: 0;
    color: var(--ink-2);
    font-size: var(--fs-2xs);
    line-height: 1.6;
  }
  .cold-start-action {
    flex: 0 0 auto;
    min-height: 24px;
    padding: 2px 9px;
    border: 1px solid var(--rule-strong);
    border-radius: 4px;
    color: var(--ink);
    background: transparent;
    font-size: var(--fs-2xs);
    cursor: pointer;
  }
  .cold-start-action:hover { background: var(--field-hover); }
  .cold-start-action:focus-visible { outline: 2px solid var(--ink); outline-offset: 2px; }
  .refresh-note,
  .layout-persistence-status {
    margin: 0 0 10px;
    padding: 7px 10px;
    border-radius: var(--radius-control);
    color: var(--alarm-ink);
    background: var(--alarm);
    font-size: var(--fs-2xs);
    overflow-wrap: anywhere;
  }
  .refresh-note :global(.link-button) { color: var(--alarm-ink); text-decoration-color: currentColor; }
  .layout-persistence-status { margin: 10px 0 0; }
  .empty { display: grid; gap: 4px; padding: 10px 0; color: var(--ink-3); font-size: var(--fs-2xs); line-height: 1.6; }
  .empty strong { color: var(--ink-2); font-size: var(--fs-xs); font-weight: 650; }
  /* 冷启动 / 无适配器：空态只是"还没有数据"，压到和正文同一档 */
  .empty.quiet { padding-block: 6px; }
  .empty.quiet strong { color: var(--ink-3); font-size: var(--fs-2xs); font-weight: 600; }

  /* ---- 统计卡：三个数字，横排，等宽 ---- */
  .stats-card { min-width: 0; height: 100%; box-sizing: border-box; display: grid; align-content: center; gap: 6px; }
  .stats-card .line { display: flex; align-items: baseline; gap: 10px; min-width: 0; }
  .stats-card .key { flex: 0 0 auto; width: 30px; color: var(--ink-3); font-size: var(--fs-2xs); letter-spacing: var(--tracking-label); }
  .stats-values { min-width: 0; display: flex; flex-wrap: wrap; gap: 16px; }
  .stats-values span { min-width: 0; display: flex; align-items: baseline; gap: 4px; color: var(--ink-3); font-size: var(--fs-2xs); }
  .stats-values strong { color: var(--ink); font: 600 18px/1 var(--font-readout); font-variant-numeric: tabular-nums; }

  /* ---- 事件层：审批与通知是头顶滑下来的一条托盘，托盘内才放卡片 ---- */
  .event-layer {
    position: fixed;
    top: 55px;
    left: 0;
    right: 0;
    z-index: var(--z-event);
    max-height: calc(100dvh - 66px);
    overflow-y: auto;
    overscroll-behavior: contain;
    pointer-events: auto;
    padding: 10px 12px 12px;
    border: 1px solid var(--rule);
    border-top: 0;
    border-radius: 0 0 12px 12px;
    background: var(--ground);
    filter: drop-shadow(0 14px 26px var(--shadow-color));
  }
  /* 没有待处理事件时不留空托盘：它会盖住下面板面 */
  .event-layer.empty { display: none; }
  .listener-error {
    margin: 8px 0 0;
    padding: 8px 10px;
    border-radius: var(--radius-control);
    color: var(--alarm-ink);
    background: var(--alarm);
    font-size: var(--fs-2xs);
  }
  :global(.privacy .sensitive) { filter: blur(5px); user-select: none; pointer-events: none; }

  .island-loading {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 16px;
    border: 1px solid var(--rule);
    border-radius: 999px;
    background: var(--ground);
    color: var(--ink-2);
    font-size: var(--fs-xs);
  }
  .island-loading::before { content: ""; width: 5px; height: 5px; border-radius: 1px; background: var(--state-working); animation: working-blink 1.8s var(--motion-out) infinite; }

  .sr-only { position: absolute; width: 1px; height: 1px; padding: 0; margin: -1px; overflow: hidden; clip: rect(0, 0, 0, 0); white-space: nowrap; border: 0; }

  :global(.adaptive-details .layout-card.custom-height .log-card) { display: flex; flex-direction: column; }
  :global(.adaptive-details .layout-card.custom-height .log) { height: auto; flex: 1; min-height: 0; }
  :global(.adaptive-details .layout-card .log) { height: var(--layout-log-height, 164px); }
  :global(.adaptive-details .layout-card .card.log-card) { height: 100%; }

  @media (max-width: 420px) {
    .agent-island { border-radius: 0 0 14px 14px; }
    .adaptive-details > * { padding-inline: 12px; }
    .event-layer { top: 55px; padding-inline: 10px; }
    .stats-values { gap: 12px; }
  }
</style>































