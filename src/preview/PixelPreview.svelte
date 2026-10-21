<script lang="ts">
  import "../themes/tokens.css";
  import "../themes/graphite.css";
  import "../themes/pure-black.css";
  import CompactIsland from "../components/island/CompactIsland.svelte";
  import type { AgentView, DisplayStatus, SessionView } from "../bridge/types";

  const STATUSES: DisplayStatus[] = ["working", "waiting", "done", "error"];
  const AGENTS = ["Claude Code", "Codex CLI", "OpenCode", "Hermes"] as const;
  const STATUS_LABELS: Record<string, string> = {
    working: "工作中",
    waiting: "等待确认",
    done: "已完成",
    error: "错误",
  };

  function makeSession(name: string, recordCount: number): SessionView {
    return {
      id: `session-${name}`,
      name: "灵动岛界面验收",
      cwd: "D:\\demo\\agent-island",
      log_path: null,
      records: Array.from({ length: recordCount }, (_, index) => ({
        event_id: `record-${index}`,
        at_ms: 1_789_130_000_000 - index * 1_000,
        role: index % 2 === 0 ? ("Assistant" as const) : ("User" as const),
        text: "占位事件",
      })),
      recent_output: [],
      current_file: "src/app/App.svelte",
      log_status: null,
      alert: null,
      lifecycle: "Active",
      last_active_at_ms: 1_789_130_000_000,
      display_status: "working",
    };
  }

  function makeAgent(name: string, status: DisplayStatus, recordCount: number): AgentView {
    return {
      id: `agent-${name}`,
      name,
      can_restart: true,
      state: {
        process: status === "done" || status === "error" ? "running" : "running",
        turn: status === "working" ? "executing" : status === "error" ? "failed" : status === "done" ? "succeeded" : "idle",
        attention: status === "waiting" ? "input_required" : "none",
        result_at_ms: null,
      },
      display_status: status,
      active_session: { ...makeSession(name, recordCount), display_status: status },
      active_turn: null,
      history_sessions: [],
      diagnostic: null,
      freshness: { observed_at_ms: 1_789_130_000_000, stale: false },
      usage: { tokens_total: 12_500, tokens_output: 2_500, cost_usd: 0.42, used_percent: 42, window_secs: 18_000, resets_at_secs: null, credits: null, unlimited: false, stale: false },
    };
  }

  const noop = () => {};
</script>

<main class="agent-island sheet" data-theme-style="pure-black">
  <section class="group" data-theme-style="pure-black">
    <h2>纯黑 · 像素模式（4 Agent × 4 状态）</h2>
    <div class="grid">
      {#each AGENTS as name (name)}
        {#each STATUSES as status (status)}
          <figure>
            <div class="agent-island status-{status}" data-theme-style="pure-black">
              <CompactIsland agent={makeAgent(name, status, 12)} expanded={false} onToggle={noop} iconStyle="pixel" />
            </div>
            <figcaption>{name} · {STATUS_LABELS[status]}</figcaption>
          </figure>
        {/each}
      {/each}
    </div>
  </section>

  <section class="group" data-theme-style="graphite">
    <h2>石墨玻璃 · 像素模式</h2>
    <div class="grid">
      {#each STATUSES as status (status)}
        <figure>
          <div class="agent-island status-{status}" data-theme-style="graphite">
            <CompactIsland agent={makeAgent("Claude Code", status, 12)} expanded={false} onToggle={noop} iconStyle="pixel" />
          </div>
          <figcaption>Claude Code · {STATUS_LABELS[status]}</figcaption>
        </figure>
      {/each}
    </div>
  </section>

  <section class="group" data-theme-style="pure-black">
    <h2>对照：原圆点模式 vs 像素模式（同数据）</h2>
    <div class="grid">
      <figure>
        <div class="agent-island status-working" data-theme-style="pure-black">
          <CompactIsland agent={makeAgent("Claude Code", "working", 12)} expanded={false} onToggle={noop} iconStyle="dot" />
        </div>
        <figcaption>圆点模式（现状，默认）</figcaption>
      </figure>
      <figure>
        <div class="agent-island status-working" data-theme-style="pure-black">
          <CompactIsland agent={makeAgent("Claude Code", "working", 12)} expanded={false} onToggle={noop} iconStyle="pixel" />
        </div>
        <figcaption>像素模式</figcaption>
      </figure>
    </div>
  </section>
</main>

<style>
  /* 预览页只用语义 token，不写死颜色（项目 theme-tokens 测试强制这一条） */
  :global(html, body) { margin: 0; }

  /* 外层借用 .agent-island 作用域来拿到主题 token，但不要它的胶囊外观 */
  .sheet.agent-island {
    width: auto;
    overflow: visible;
    border: 0;
    border-radius: 0;
    background: var(--surface-log);
    box-shadow: none;
    min-height: 100vh;
    box-sizing: border-box;
    color: var(--text-primary);
    font: 500 13px/1.5 Inter, ui-sans-serif, system-ui, "Segoe UI", sans-serif;
  }

  .sheet { display: grid; gap: 26px; padding: 24px; align-content: start; }
  .group h2 { margin: 0 0 12px; font-size: 12px; font-weight: 700; letter-spacing: .08em; text-transform: uppercase; color: var(--text-tertiary); }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, 420px); gap: 14px 18px; }
  figure { margin: 0; display: grid; gap: 6px; }
  figcaption { font-size: 11px; color: var(--text-tertiary); }

  .agent-island {
    width: 420px;
    overflow: hidden;
    border: 1px solid var(--island-border-subtle);
    border-radius: var(--radius-island);
    background: var(--island-background);
    box-shadow: var(--island-inset-highlight);
  }
</style>
