<script lang="ts">
  import type { CommandFailure, CommandUiState } from "../events/types";

  let {
    privacy,
    focusMode,
    themeStyle,
    iconStyle,
    agentMode,
    hookEnabled,
    hookState,
    hookError,
    onPrivacy,
    onFocusMode,
    onThemeStyle,
    onIconStyle,
    onAgentMode,
    onHookToggle,
    onOpen,
    onLayoutChange,
    onAdjustWidth,
    onStartResize,
  }: {
    privacy: boolean;
    focusMode: "off" | "errors" | "quiet";
    themeStyle: "graphite" | "pure-black";
    iconStyle: "dot" | "pixel";
    agentMode: "all" | "running";
    hookEnabled: boolean | null;
    hookState: CommandUiState;
    hookError: CommandFailure | null;
    onPrivacy: (value: boolean) => void;
    onFocusMode: (value: "off" | "errors" | "quiet") => void;
    onThemeStyle: (value: "graphite" | "pure-black") => void;
    onIconStyle: (value: "dot" | "pixel") => void;
    onAgentMode: (value: "all" | "running") => void;
    onHookToggle: () => void;
    onOpen: () => void;
    onLayoutChange: () => void;
    onAdjustWidth?: (delta: number) => void;
    onStartResize?: () => void;
  } = $props();
  let element: HTMLDetailsElement;
  function dismiss(event: PointerEvent): void {
    if (element?.open && event.target instanceof Node && !element.contains(event.target)) element.open = false;
  }
  function escape(event: KeyboardEvent): void {
    if (event.key === "Escape" && element?.open) {
      element.open = false;
      element.querySelector("summary")?.focus();
    }
  }
</script>

<svelte:window onpointerdown={dismiss} onkeydown={escape} />
<details bind:this={element} class="settings" ontoggle={(event) => { onLayoutChange(); if (event.currentTarget.open) onOpen(); }}>
  <summary aria-label="打开显示设置">设置</summary>
  <div class="popover">
    <div class="setting-row">
      <span>Claude 事件</span>
      <button type="button" data-testid="hook-toggle" data-action-state={hookState} disabled={hookState === "pending"} onclick={onHookToggle}>
        {hookState === "pending" ? "处理中…" : hookEnabled === true ? "已接入" : hookEnabled === false ? "未接入" : "检查状态"}
      </button>
    </div>
    {#if hookError}<p class="setting-error sensitive" role="alert"><strong>{hookError.code}</strong> · {hookError.message}</p>{/if}
    <label><span>隐私遮罩</span><input type="checkbox" checked={privacy} onchange={(event) => onPrivacy(event.currentTarget.checked)} /></label>
    <label><span>专注模式</span><select value={focusMode} onchange={(event) => onFocusMode(event.currentTarget.value as "off" | "errors" | "quiet")}><option value="off">全部状态</option><option value="errors">聚焦错误</option><option value="quiet">静音通知</option></select></label>
    <label><span>外观</span><select value={themeStyle} onchange={(event) => onThemeStyle(event.currentTarget.value as "graphite" | "pure-black")}><option value="graphite">石墨玻璃</option><option value="pure-black">纯黑</option></select></label>
    <label><span>图标</span><select value={iconStyle} onchange={(event) => onIconStyle(event.currentTarget.value as "dot" | "pixel")}><option value="pixel">像素吉祥物</option><option value="dot">圆点</option></select></label>
    <label><span>显示</span><select value={agentMode} onchange={(event) => onAgentMode(event.currentTarget.value as "all" | "running")}><option value="all">全部检测</option><option value="running">仅运行中</option></select></label>
    {#if onAdjustWidth || onStartResize}
      <div class="setting-row">
        <span>窗口宽度</span>
        <div class="width-controls">
          {#if onAdjustWidth}<button type="button" aria-label="缩窄窗口" title="缩窄 40px" onclick={() => onAdjustWidth(-40)}>−</button><button type="button" aria-label="加宽窗口" title="加宽 40px" onclick={() => onAdjustWidth(40)}>＋</button>{/if}
          {#if onStartResize}<button type="button" aria-label="拖动调整窗口宽度" title="按住拖动，或用左右方向键" onpointerdown={(event) => { if (event.button === 0) { event.preventDefault(); onStartResize(); } }} onkeydown={(event) => { if (onAdjustWidth && (event.key === "ArrowLeft" || event.key === "ArrowRight")) { event.preventDefault(); onAdjustWidth(event.key === "ArrowRight" ? 20 : -20); } }}>↔</button>{/if}
        </div>
      </div>
    {/if}
    <div class="setting-feedback" role="status" aria-live="polite" data-testid="settings-feedback">
      <p>{privacy ? "隐私遮罩已开启：会话文本和路径已模糊。" : "隐私遮罩已关闭：会话内容正常显示。"}</p>
      <p>{focusMode === "errors" ? "聚焦错误：优先选择异常 Agent，并隐藏普通通知。" : focusMode === "quiet" ? "静音通知：隐藏普通通知，审批请求仍会显示。" : "全部状态：正常显示任务通知与审批。"}</p>
      <p>当前外观：{themeStyle === "graphite" ? "石墨玻璃" : "纯黑"}；图标：{iconStyle === "pixel" ? "像素吉祥物" : "圆点"}；显示：{agentMode === "running" ? "仅运行中" : "全部检测"}。</p>
    </div>
  </div>
</details>

<style>
  .settings { position: relative; }
  .settings[open] { flex: 1 0 100%; min-width: 0; }
  .settings[open] summary { width: fit-content; margin-left: auto; }
  /* 下拉弹层由浏览器渲染：必须让 select/option 都声明深色，且给不透明底色，
     否则 Windows 上弹层是白底 + 白色文字，只有悬停高亮时才能看见。 */
  select { color-scheme: dark; }
  select option { color-scheme: dark; color: var(--island-menu-text); background-color: var(--island-menu-surface); }
  select option:checked { background-color: var(--island-menu-surface-hover); }
  summary { min-height: 30px; display: grid; place-items: center; padding: 5px 10px; border: 1px solid var(--island-border-subtle); border-radius: 9px; color: var(--island-text-secondary); background: var(--island-control-surface); cursor: pointer; list-style: none; }
  summary::-webkit-details-marker { display: none; }
  summary:focus-visible, select:focus-visible, input:focus-visible { outline: 2px solid var(--island-accent-primary); outline-offset: 2px; }
  .popover { position: static; width: auto; margin-top: 8px; display: grid; gap: 10px; padding: 12px; border: 1px solid var(--island-event-border); border-radius: 14px; background: var(--island-event-surface); }
  label { display: flex; align-items: center; justify-content: space-between; gap: 12px; color: var(--island-text-secondary); font-size: 11px; }
  .setting-row { display: flex; align-items: center; justify-content: space-between; gap: 12px; color: var(--island-text-secondary); font-size: 11px; }
  .width-controls { display: flex; align-items: center; gap: 6px; }
  .width-controls button { min-width: 32px; padding: 4px 8px; }
  select, button { min-width: 100px; min-height: 32px; padding: 5px 7px; border: 1px solid var(--island-border-subtle); border-radius: 8px; color: var(--island-text-primary); background: var(--island-control-surface); }
  button { cursor: pointer; }
  button:disabled { opacity: .55; cursor: wait; }
  .setting-feedback { display: grid; gap: 5px; padding-top: 8px; border-top: 1px solid var(--island-border-subtle); color: var(--island-text-secondary); font-size: 10px; line-height: 1.5; }
  .setting-feedback p { margin: 0; }
  summary, button { transition: background 140ms, transform 140ms; }
  summary:hover, button:hover:not(:disabled) { background: var(--island-control-hover); }
  button:active:not(:disabled) { transform: scale(.97); }
  .popover { max-height: min(280px, 35dvh); overflow-y: auto; }
  @media (prefers-reduced-motion: reduce) { summary, button { transition: none; } }
  .setting-error { margin: -2px 0 0; color: var(--island-refresh-error-text); font-size: 10px; overflow-wrap: anywhere; }
</style>
