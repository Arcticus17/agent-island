<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import "@xterm/xterm/css/xterm.css";
  import type { AgentView } from "../../bridge/types";

  let {
    agent,
    sessionId = null,
    program = null,
    text = "",
    starting = false,
    exited = false,
    error = null,
    maximized = false,
    binding = "inferred",
    onToggleMax,
    onStart,
    onStop,
    onInput,
    onResize,
    onLayoutChange,
  }: {
    agent: AgentView | null;
    /** 会话由 App 持有：面板只管渲染，收起面板不会重启 Agent */
    sessionId?: string | null;
    program?: string | null;
    text?: string;
    starting?: boolean;
    exited?: boolean;
    error?: string | null;
    maximized?: boolean;
    /** 选中 Agent 的会话身份来源：launched/hooked 时显示"已绑定" */
    binding?: "launched" | "hooked" | "inferred" | "manual";
    onToggleMax?: () => void;
    onStart?: (program: string) => void;
    onStop?: () => void;
    onInput?: (data: string) => void;
    onResize?: (cols: number, rows: number) => void;
    onLayoutChange?: () => void;
  } = $props();

  const AGENT_CLI: Record<string, string> = {
    "Claude Code": "claude",
    "Codex CLI": "codex",
    OpenCode: "opencode",
    Hermes: "hermes",
  };

  let host = $state<HTMLDivElement>();
  let terminal: Terminal | null = null;
  let fitAddon: FitAddon | null = null;
  let observer: ResizeObserver | null = null;
  let written = 0;
  /** 首次整段重放由 onMount 负责；effect 在挂载早期不重复写，避免内容出现两遍 */
  let synced = false;
  /** 上一帧应用的输出长度，用来处理"历史被截断"的情况 */
  let appliedSnapshot = 0;

  // 自定义 / 自动发现的 Agent 也能一键启动：用快照里下发的 program
  const agentCli = $derived(
    agent ? AGENT_CLI[agent.name] ?? (agent.program?.trim() || null) : null,
  );
  /** 终端抬头显示人类可读的程序名：powershell → PowerShell，cmd.exe → Shell。 */
  const PROGRAM_LABELS: Record<string, string> = {
    powershell: "PowerShell",
    "powershell.exe": "PowerShell",
    pwsh: "PowerShell",
    "pwsh.exe": "PowerShell",
    cmd: "Shell",
    "cmd.exe": "Shell",
  };
  /** 终端抬头显示人类可读的名字：Agent 自己的命令用 Agent 名，shell 用固定文案。 */
  const programLabel = $derived.by(() => {
    if (!program) return null;
    const known = PROGRAM_LABELS[program.toLowerCase()];
    if (known) return known;
    if (agent && agent.program?.trim() === program) return agent.name;
    return program;
  });
  const phase = $derived(
    error ? "error" : starting ? "starting" : sessionId ? "running" : exited ? "exited" : "idle",
  );

  function themeColors(node: HTMLElement) {
    const styles = getComputedStyle(node);
    const read = (name: string, fallback: string) => styles.getPropertyValue(name).trim() || fallback;
    return {
      background: read("--ground-sunken", "#0b0d11"),
      foreground: read("--ink-log", "#c8cfd9"),
      cursor: read("--state-working", "#38c6a5"),
      selectionBackground: read("--field-hover", "rgba(255, 255, 255, .12)"),
    };
  }

  /**
   * 视觉上始终适配容器；是否让 PTY 跟着变由 App 决定（它持有"PTY 当前尺寸"）。
   *
   * 原因：ConPTY 尺寸一变，claude/codex 这类 TUI 会把整屏重新打印一遍，
   * 而重打印的内容会被追加进 scrollback——拖动/伸缩窗口时就会看到同一段内容反复出现。
   * 所以普通窗口伸缩只做本地 reflow（report=false），只有挂载对齐和用户点最大化才上报。
   */
  function fit(report = false): void {
    if (!fitAddon || !terminal) return;
    try {
      fitAddon.fit();
    } catch {
      return;
    }
    if (!report) return;
    onResize?.(terminal.cols, terminal.rows);
  }

  /** 拖动/展开窗口时 ResizeObserver 会连发很多次；等停下来再做本地 reflow。 */
  let fitTimer: number | undefined;
  function scheduleFit(): void {
    if (fitTimer !== undefined) window.clearTimeout(fitTimer);
    fitTimer = window.setTimeout(() => {
      fitTimer = undefined;
      fit();
    }, 160);
  }

  function createTerminal(): Terminal {
    const node = host as HTMLDivElement;
    const colors = themeColors(node);
    const instance = new Terminal({
      convertEol: false,
      cursorBlink: true,
      fontSize: 12,
      fontFamily: 'ui-monospace, "Cascadia Mono", "Consolas", monospace',
      scrollback: 2000,
      theme: {
        background: colors.background,
        foreground: colors.foreground,
        cursor: colors.cursor,
        selectionBackground: colors.selectionBackground,
      },
    });
    const addon = new FitAddon();
    instance.loadAddon(addon);
    instance.open(node);
    instance.onData((data) => onInput?.(data));
    terminal = instance;
    fitAddon = addon;
    return instance;
  }

  /** 把 App 累积的输出增量写进 xterm；历史被截断时整体重放一次。 */
  $effect(() => {
    const next = text;
    const instance = terminal;
    // 首次整段重放交给 onMount：两个入口都写一遍就会看到两倍内容
    if (!instance || !synced) return;
    if (next.length < appliedSnapshot) {
      instance.reset();
      written = 0;
    }
    appliedSnapshot = next.length;
    if (next.length > written) {
      instance.write(next.slice(written));
      written = next.length;
    }
  });

  /** 会话就绪（首次启动或重新挂载后拿到 id）→ 重新对齐尺寸并聚焦。 */
  $effect(() => {
    if (!sessionId || !terminal) return;
    // 会话就绪时对齐一次 PTY 尺寸（这是唯一"必须让 TUI 知道"的时刻之一）
    requestAnimationFrame(() => fit(true));
  });

  onMount(() => {
    const instance = createTerminal();
    if (text.length > 0) {
      instance.write(text);
      written = text.length;
      appliedSnapshot = text.length;
    }
    synced = true;
    requestAnimationFrame(() => {
      // 首次挂载只做本地 reflow；PTY 尺寸由 open 时的 cols/rows 决定，
      // 真正的对齐发生在会话就绪（下面的 $effect）和最大化时。
      fit();
      if (sessionId) instance.focus();
      onLayoutChange?.();
    });

    if (host && typeof ResizeObserver !== "undefined") {
      observer = new ResizeObserver(() => { if (sessionId) scheduleFit(); });
      observer.observe(host as HTMLElement);
    }

    return () => {
      // 只销毁渲染器；PTY 归 App 所有，收起面板不关 Agent
      observer?.disconnect();
      observer = null;
      instance.dispose();
      terminal = null;
      fitAddon = null;
    };
  });

  onDestroy(() => {
    observer?.disconnect();
    observer = null;
  });
</script>

<section class="terminal-panel" data-testid="terminal-panel" aria-label="内置终端">
  <header>
    <span class="title">终端</span>
    <span class="phase" data-testid="terminal-phase" data-phase={phase}>
      {#if phase === "starting"}启动中…
      {:else if phase === "running"}{programLabel ?? "运行中"}
      {:else if phase === "exited"}已退出
      {:else if phase === "error"}出错
      {:else}未启动{/if}
    </span>
    {#if binding !== "inferred" && program}
      <span class="binding" data-testid="terminal-binding" data-binding={binding} title="这个终端里的 Agent 由岛内启动，写出的会话会自动关联">已绑定 · 会话自动关联</span>
    {/if}
    <div class="actions">
      {#if agentCli}
        <button type="button" data-testid="terminal-start-agent" onclick={() => onStart?.(agentCli)}>{agentCli}</button>
      {/if}
      <button type="button" data-testid="terminal-start-shell" onclick={() => onStart?.("cmd.exe")}>Shell</button>
      {#if onToggleMax}
        <!-- Lucide "maximize-2 / minimize-2"（ISC）：对角箭头表示放大/还原，和 VS Code 面板、浏览器全屏的语义一致 -->
        <button
          type="button"
          class="icon-button"
          data-testid="terminal-maximize"
          data-maximized={maximized}
          aria-pressed={maximized}
          aria-label={maximized ? "还原终端大小" : "放大终端"}
          title={maximized ? "还原终端大小" : "放大终端：窗口一起放大"}
          onclick={() => {
            onToggleMax?.();
            // 最大化/还原是用户明确要求的尺寸变化：等布局落定后再对齐 PTY
            window.setTimeout(() => fit(true), 260);
          }}
        >
          {#if maximized}
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">
              <path d="m14 10l7-7m-1 7h-6V4M3 21l7-7m-6 0h6v6" />
            </svg>
          {:else}
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">
              <path d="M15 3h6v6m0-6l-7 7M3 21l7-7m-1 7H3v-6" />
            </svg>
          {/if}
        </button>
      {/if}
      {#if sessionId}<button type="button" data-testid="terminal-stop" onclick={() => onStop?.()}>关闭</button>{/if}
    </div>
  </header>
  {#if error}
    <p class="error" role="alert" data-testid="terminal-error">{error}</p>
  {/if}
  <div class="screen" class:maximized bind:this={host} data-testid="terminal-screen" role="application" aria-label="Agent 终端"
    onpointerdown={() => terminal?.focus()}></div>
</section>

<style>
  .terminal-panel { display: grid; gap: 6px; min-width: 0; }
  header { display: flex; align-items: center; gap: 8px; }
  .title { color: var(--island-text-primary); font-size: 12px; font-weight: 700; }
  .phase { color: var(--island-text-tertiary); font-size: 10px; }
  .phase[data-phase="running"] { color: var(--island-status-done); }
  .phase[data-phase="error"] { color: var(--island-status-error); }
  .binding { padding: 1px 7px; border: 1px solid var(--island-accent-primary); border-radius: 999px; color: var(--island-accent-primary); font-size: 9px; font-weight: 700; white-space: nowrap; }
  .actions { margin-left: auto; display: flex; gap: 6px; }
  .actions button { min-height: 26px; padding: 3px 9px; border: 1px solid var(--island-border-subtle); border-radius: var(--island-radius-control); color: var(--island-text-secondary); background: var(--island-control-surface); font-size: 11px; cursor: pointer; }
  .actions button:hover { color: var(--island-text-primary); background: var(--island-control-hover); }
  .actions button:focus-visible { outline: 2px solid var(--island-accent-primary); outline-offset: 2px; }
  .actions .icon-button { display: inline-grid; place-items: center; width: 26px; min-width: 26px; padding: 0; }
  .actions .icon-button[aria-pressed="true"] { color: var(--island-accent-primary); border-color: var(--island-accent-primary); }
  .screen {
    height: 240px;
    padding: 6px;
    overflow: hidden;
    border: 1px solid var(--island-border-subtle);
    border-radius: var(--island-radius-control);
    background: var(--island-log-surface);
    transition: height var(--duration-normal) ease-out;
  }
  .screen.maximized { height: 560px; }
  .terminal-panel:has(.screen.maximized) { gap: 8px; }
  .error { margin: 0; color: var(--island-refresh-error-text); font-size: 11px; overflow-wrap: anywhere; }
</style>





