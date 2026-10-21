/**
 * 设置持久化：localStorage 仍然是读写入口（同步、组件不用改），
 * 但每次变更会镜像一份到 `~/.agent-island/settings.json`（Rust 侧原子写）。
 *
 * 为什么需要：进程被强杀时 localStorage 的最后一次写入可能还没落盘，
 * 重启后主题/布局/窗口位置会回到默认值——实测丢过一次。
 *
 * 注意：这里不直接 import Tauri API，磁盘读写统一走 bridge（架构测试会检查这一点）。
 */

export const SETTINGS_PREFIX = "agent-island";

export interface SettingsStorage {
  readonly length: number;
  key(index: number): string | null;
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem?(key: string): void;
}

/** 持久化只需要 bridge 的这两个能力，测试里可以只实现这一部分。 */
export interface SettingsBridge {
  loadAppSettings?(): Promise<string | null>;
  saveAppSettings?(settings: string): Promise<void>;
}

/** 收集当前所有 `agent-island*` 键值（顺序无关）。 */
export function collectSettings(
  storage: SettingsStorage,
  prefix = SETTINGS_PREFIX,
): Record<string, string> {
  const data: Record<string, string> = {};
  for (let index = 0; index < storage.length; index += 1) {
    const key = storage.key(index);
    if (!key || !key.startsWith(prefix)) continue;
    const value = storage.getItem(key);
    if (value !== null) data[key] = value;
  }
  return data;
}

/** 把磁盘上的设置写回 localStorage；返回是否真的改过。 */
export function applySettings(
  storage: SettingsStorage,
  data: Record<string, unknown>,
  prefix = SETTINGS_PREFIX,
): boolean {
  let changed = false;
  for (const [key, value] of Object.entries(data)) {
    if (!key.startsWith(prefix) || typeof value !== "string") continue;
    if (storage.getItem(key) === value) continue;
    storage.setItem(key, value);
    changed = true;
  }
  return changed;
}

/** 与键顺序无关的指纹，用来判断"有没有变"。 */
export function settingsFingerprint(data: Record<string, string>): string {
  return JSON.stringify(
    Object.keys(data)
      .sort()
      .map((key) => [key, data[key]]),
  );
}

function runtimeStorage(): SettingsStorage | null {
  if (typeof window === "undefined" || !window.localStorage) return null;
  return window.localStorage;
}

/** 落盘当前设置（幂等）。 */
export async function persistStoredSettings(
  bridge: SettingsBridge,
  storage: SettingsStorage | null = runtimeStorage(),
): Promise<void> {
  if (!storage || !bridge.saveAppSettings) return;
  try {
    await bridge.saveAppSettings(JSON.stringify(collectSettings(storage)));
  } catch {
    // 落盘失败不应该影响界面
  }
}

/** 启动时调用：把磁盘设置镜像回 localStorage；首次运行则把现有设置落盘。 */
export async function restoreStoredSettings(
  bridge: SettingsBridge,
  storage: SettingsStorage | null = runtimeStorage(),
): Promise<boolean> {
  if (!storage || !bridge.loadAppSettings) return false;
  let raw: string | null = null;
  try {
    raw = await bridge.loadAppSettings();
  } catch {
    return false;
  }
  if (!raw) {
    await persistStoredSettings(bridge, storage);
    return false;
  }
  try {
    const parsed: unknown = JSON.parse(raw);
    if (parsed && typeof parsed === "object") {
      return applySettings(storage, parsed as Record<string, unknown>);
    }
  } catch {
    // 损坏的设置文件不阻塞启动
  }
  return false;
}

/**
 * 监听 localStorage 变化并落盘：不 patch localStorage，也不要求每个 setter 都记得调用，
 * 1.5s 比对一次指纹，代价可以忽略。
 */
export function startSettingsPersistence(
  bridge: SettingsBridge,
  options: { intervalMs?: number } = {},
): () => void {
  const storage = runtimeStorage();
  if (!storage || typeof window === "undefined" || !bridge.saveAppSettings) {
    return () => undefined;
  }
  const save = bridge.saveAppSettings;
  let last = settingsFingerprint(collectSettings(storage));

  const flush = (): void => {
    const current = collectSettings(storage);
    const fingerprint = settingsFingerprint(current);
    if (fingerprint === last) return;
    last = fingerprint;
    void save.call(bridge, JSON.stringify(current)).catch(() => undefined);
  };

  const timer = window.setInterval(flush, options.intervalMs ?? 1500);
  window.addEventListener("pagehide", flush);
  return () => {
    window.clearInterval(timer);
    window.removeEventListener("pagehide", flush);
  };
}
