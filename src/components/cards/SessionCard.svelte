<script lang="ts">
  import type { SessionView } from "../../bridge/types";
  let { session, privacy, binding = "inferred" }: { session: SessionView; privacy: boolean; binding?: "launched" | "hooked" | "inferred" | "manual" } = $props();

  const bindingLabel: Record<"launched" | "hooked" | "inferred" | "manual", string> = {
    launched: "已确认",
    hooked: "已确认（hooks）",
    inferred: "推测",
    manual: "已绑定",
  };
</script>

<article class="card session-card">
  <div class="line">
    <span class="key">会话</span>
    <span class="val sensitive" data-testid="active-session-id">{session.id}</span>
    <span class="tag">{session.lifecycle === "Historical" ? "本地记录" : "活动会话"}</span>
    <span class="binding" data-testid="session-binding" data-binding={binding} title={binding === "inferred" ? "会话身份由日志推断，可能不准" : "会话身份来自可验证来源"}>{bindingLabel[binding]}</span>
  </div>
  <span class="path sensitive" data-testid="session-path" title={privacy ? undefined : (session.cwd ?? "")}>{session.cwd ?? "未提供工作目录"}</span>
  {#if session.current_file}<span class="file sensitive" title={privacy ? undefined : session.current_file}>{session.current_file}</span>{/if}
</article>

<style>
  .card { min-width: 0; display: grid; gap: 3px; }
  .line { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .key { flex: 0 0 auto; width: 30px; color: var(--ink-3); font-size: var(--fs-2xs); letter-spacing: var(--tracking-label); }
  .val { overflow: hidden; color: var(--ink); font-size: var(--fs-sm); font-weight: 600; text-overflow: ellipsis; white-space: nowrap; }
  .tag { flex: 0 0 auto; color: var(--ink-3); font-size: var(--fs-2xs); }
  .binding {
    flex: 0 0 auto;
    padding: 1px 5px;
    border: 1px solid var(--rule-strong);
    border-radius: 4px;
    color: var(--ink-3);
    font: 600 9px/1.3 var(--font-readout);
    letter-spacing: .04em;
  }
  .binding[data-binding="launched"], .binding[data-binding="hooked"] { border-color: var(--state-working); color: var(--state-working); }
  .binding[data-binding="manual"] { border-color: var(--ink-2); color: var(--ink-2); }
  .path, .file { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; padding-left: 38px; }
  .path { color: var(--ink-2); font: 400 var(--fs-2xs)/1.5 var(--font-mono); }
  .file { color: var(--ink-3); font: 400 var(--fs-2xs)/1.5 var(--font-mono); }
</style>
