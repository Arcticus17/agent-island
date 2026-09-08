<script lang="ts">
  import type { CommandFailure, CommandUiState } from "../events/types";

  let {
    privacy,
    focusMode,
    themeStyle,
    hookEnabled,
    hookState,
    hookError,
    onPrivacy,
    onFocusMode,
    onThemeStyle,
    onHookToggle,
    onOpen,
    onLayoutChange,
  }: {
    privacy: boolean;
    focusMode: "off" | "errors" | "quiet";
    themeStyle: "graphite" | "pure-black";
    hookEnabled: boolean | null;
    hookState: CommandUiState;
    hookError: CommandFailure | null;
    onPrivacy: (value: boolean) => void;
    onFocusMode: (value: "off" | "errors" | "quiet") => void;
    onThemeStyle: (value: "graphite" | "pure-black") => void;
    onHookToggle: () => void;
    onOpen: () => void;
    onLayoutChange: () => void;
  } = $props();
</script>

<details class="settings" ontoggle={(event) => { onLayoutChange(); if (event.currentTarget.open) onOpen(); }}>
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
  </div>
</details>

<style>
  .settings { position: relative; }
  summary { min-height: 30px; display: grid; place-items: center; padding: 5px 10px; border: 1px solid var(--island-border-subtle); border-radius: 9px; color: var(--island-text-secondary); background: var(--island-control-surface); cursor: pointer; list-style: none; }
  summary::-webkit-details-marker { display: none; }
  summary:focus-visible, select:focus-visible, input:focus-visible { outline: 2px solid var(--island-accent-primary); outline-offset: 2px; }
  .popover { position: absolute; right: 0; bottom: calc(100% + 8px); z-index: 4; width: 220px; display: grid; gap: 10px; padding: 12px; border: 1px solid var(--island-event-border); border-radius: 14px; background: var(--island-event-surface); box-shadow: var(--island-event-shadow); }
  label { display: flex; align-items: center; justify-content: space-between; gap: 12px; color: var(--island-text-secondary); font-size: 11px; }
  .setting-row { display: flex; align-items: center; justify-content: space-between; gap: 12px; color: var(--island-text-secondary); font-size: 11px; }
  select, button { min-width: 100px; min-height: 32px; padding: 5px 7px; border: 1px solid var(--island-border-subtle); border-radius: 8px; color: var(--island-text-primary); background: var(--island-control-surface); }
  button { cursor: pointer; }
  button:disabled { opacity: .55; cursor: wait; }
  .setting-error { margin: -2px 0 0; color: var(--island-refresh-error-text); font-size: 10px; overflow-wrap: anywhere; }
  @media (max-width: 420px) { .popover { position: fixed; right: 12px; bottom: 54px; left: 12px; width: auto; } }
</style>
