<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { Terminal } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import "@xterm/xterm/css/xterm.css";
  import type { AgentIslandBridge, AgentView } from "../../bridge/types";

  let {
    agent,
    bridge,
    maximized = false,
    onToggleMax,
    onLayoutChange,
  }: {
    agent: AgentView | null;
    bridge: AgentIslandBridge;
    /** 终端放大模式：占满面板，由 App 负责把窗口一起放大 */
    maximized?: boolean;
    onToggleMax?: () => void;
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
  let sessionId = $state<string | null>(null);
  let unlisten: (() => void) | null = null;
  let observer: ResizeObserver | null = null;
  let disposed = false;

  let phase = $state<"idle" | "starting" | "running" | "exited" | "error">("idle");
  let error = $state<string | null>(null);
  let activeProgram = $state<string | null>(null);

  const agentCli = $derived(agent ? AGENT_CLI[agent.name] ?? null : null);
  const cwd = $derived(agent?.active_session?.cwd ?? null);

  function themeColors(node: HTMLElement) {
    const styles = getComputedStyle(node);
    const read = (name: string, fallback: string) => styles.getPropertyValue(name).trim() || fallback;
    return {
      background: read("--island-log-surface", read("--island-surface", "#09090b")),
      foreground: read("--island-log-text", read("--island-text-primary", "#e4e4e7")),
      cursor: read("--island-accent-primary", "#7dd3fc"),
      selectionBackground: read("--island-surface-raised", "#161619"),
    };
  }

  function fit(): void {
    if (!fitAddon || !terminal || !sessionId) return;
    try {
      fitAddon.fit();
    } catch {
      return;
    }
    void bridge.terminalResize?.(sessionId, terminal.cols, terminal.rows).catch(() => undefined);
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
    instance.onData((data) => {
      const current = sessionId;
      if (current) void bridge.terminalWrite?.(current, data).catch(() => undefined);
    });
    terminal = instance;
    fitAddon = addon;
    return instance;
  }

  async function start(program: string, args: string[] = []): Promise<void> {
    if (!bridge.terminalOpen) {
      error = "当前宿主不支持内置终端";
      phase = "error";
      return;
    }
    const instance = terminal ?? createTerminal();
    if (sessionId) await stop(false);
    instance.reset();
    error = null;
    phase = "starting";
    activeProgram = program;
    try {
      const id = await bridge.terminalOpen({
        program,
        args,
        cwd,
        cols: instance.cols || 80,
        rows: instance.rows || 20,
      });
      if (disposed) {
        void bridge.terminalClose?.(id).catch(() => undefined);
        return;
      }
      sessionId = id;
      phase = "running";
      instance.focus();
      // 首帧布局完成后再 fit 一次，拿到正确的行列数
      requestAnimationFrame(() => { fit(); });
      onLayoutChange?.();
    } catch (cause) {
      phase = "error";
      error = cause instanceof Error ? cause.message : String(cause);
      onLayoutChange?.();
    }
  }

  async function stop(clearPhase = true): Promise<void> {
    const current = sessionId;
    sessionId = null;
    if (current) await bridge.terminalClose?.(current).catch(() => undefined);
    if (clearPhase) {
      phase = "idle";
      activeProgram = null;
      onLayoutChange?.();
    }
  }

  onMount(() => {
    void (async () => {
      if (bridge.listenTerminalEvents) {
        unlisten = await bridge.listenTerminalEvents((event) => {
          if (event.id !== sessionId) {
            if (event.kind === "exit") return;
            return;
          }
          if (event.kind === "data") terminal?.write(event.data);
          else {
            phase = "exited";
            sessionId = null;
            onLayoutChange?.();
          }
        });
      }
      if (!bridge.terminalOpen) {
        error = "当前宿主不支持内置终端";
        phase = "error";
        return;
      }
      // 打开面板就自动开一个：优先 Agent 自己的 CLI，没有就落到 Shell
      await start(agentCli ?? "cmd.exe");
    })();

    if (host && typeof ResizeObserver !== "undefined") {
      observer = new ResizeObserver(() => { if (sessionId) fit(); });
      observer.observe(host as HTMLElement);
    }

    return () => {
      disposed = true;
      observer?.disconnect();
      observer = null;
      unlisten?.();
      unlisten = null;
      void stop(false);
      terminal?.dispose();
      terminal = null;
    };
  });

  onDestroy(() => {
    disposed = true;
  });
</script>

<section class="terminal-panel" data-testid="terminal-panel" aria-label="内置终端">
  <header>
    <span class="title">终端</span>
    <span class="phase" data-testid="terminal-phase" data-phase={phase}>
      {#if phase === "starting"}启动中…
      {:else if phase === "running"}{activeProgram ?? "运行中"}
      {:else if phase === "exited"}已退出
      {:else if phase === "error"}出错
      {:else}未启动{/if}
    </span>
    <div class="actions">
      {#if agentCli}<button type="button" data-testid="terminal-start-agent" onclick={() => void start(agentCli)}>{agentCli}</button>{/if}
      <button type="button" data-testid="terminal-start-shell" onclick={() => void start("cmd.exe")}>Shell</button>
      {#if onToggleMax}
        <!-- Lucide "maximize-2 / minimize-2"（ISC）：对角箭头表示放大/还原，和 VS Code 面板、浏览器全屏的语义一致 -->
        <button
          type="button"
          class="icon-button"
          data-testid="terminal-maximize"
          data-maximized={maximized}
          aria-pressed={maximized}
          aria-label={maximized ? "还原终端大小" : "放大终端"}
          title={maximized ? "还原终端大小（Esc 之外也可点这里）" : "放大终端：窗口一起放大"}
          onclick={onToggleMax}
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
      {#if sessionId}<button type="button" data-testid="terminal-stop" onclick={() => void stop()}>关闭</button>{/if}
    </div>
  </header>
  {#if error}
    <p class="error" role="alert" data-testid="terminal-error">{error}</p>
  {/if}
  <div class="screen" class:maximized bind:this={host} data-testid="terminal-screen" role="application" aria-label="Agent 终端"
    onpointerdown={() => terminal?.focus()}></div>
</section>

<style>
  .terminal-panel { display: grid; gap: 6px; }
  header { display: flex; align-items: center; gap: 8px; }
  .title { color: var(--island-text-primary); font-size: 12px; font-weight: 700; }
  .phase { color: var(--island-text-tertiary); font-size: 10px; }
  .phase[data-phase="running"] { color: var(--island-status-done); }
  .phase[data-phase="error"] { color: var(--island-status-error); }
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


