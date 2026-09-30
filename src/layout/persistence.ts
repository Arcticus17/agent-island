import { DEFAULT_LAYOUTS, type DefaultLayoutName } from "./presets";
import { validateLayout, type LayoutConfigV1 } from "./schema";

export const LAYOUT_STORAGE_KEY = "agent-island-layout-v1";
export const LAYOUT_BACKUP_KEY = "agent-island-layout-backup";

export interface LayoutStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

export type LayoutWriteError =
  | "invalid_layout"
  | "invalid_layout_preset"
  | "layout_storage_write_failed";

export type LayoutWriteResult =
  | { ok: true; value: LayoutConfigV1 }
  | { ok: false; code: LayoutWriteError };

const DEFAULT_PRESET_NAMES = ["minimal", "monitoring", "debugging"] as const;

function copyLayout(layout: LayoutConfigV1): LayoutConfigV1 {
  return {
    version: 1,
    preset: layout.preset,
    cards: layout.cards.map((card) => ({ ...card })),
  };
}

function monitoringFallback(): LayoutConfigV1 {
  return copyLayout(DEFAULT_LAYOUTS.monitoring);
}

function backupInvalid(storage: LayoutStorage, raw: string): void {
  try {
    storage.setItem(LAYOUT_BACKUP_KEY, raw);
  } catch {
    // Recovery must remain available even when storage is read-only or full.
  }
}

/** Loads untrusted persisted state without allowing storage failures to block startup. */
export function loadLayout(storage: LayoutStorage): LayoutConfigV1 {
  try {
    const raw = storage.getItem(LAYOUT_STORAGE_KEY);
    if (raw === null) return monitoringFallback();
    if (typeof raw !== "string") return monitoringFallback();

    let parsed: unknown;
    try {
      parsed = JSON.parse(raw);
    } catch {
      backupInvalid(storage, raw);
      return monitoringFallback();
    }

    const result = validateLayout(parsed);
    if (!result.ok) {
      backupInvalid(storage, raw);
      return monitoringFallback();
    }
    return copyLayout(result.value);
  } catch {
    return monitoringFallback();
  }
}

/** Persists only a validated, owned layout and reports storage failures as data. */
export function saveLayout(storage: LayoutStorage, layout: unknown): LayoutWriteResult {
  const result = validateLayout(layout);
  if (!result.ok) return { ok: false, code: "invalid_layout" };

  const value = copyLayout(result.value);
  try {
    storage.setItem(LAYOUT_STORAGE_KEY, JSON.stringify(value));
  } catch {
    return { ok: false, code: "layout_storage_write_failed" };
  }
  return { ok: true, value };
}

/** Restores one of the three shipped defaults without sharing preset references. */
export function resetLayout(storage: LayoutStorage, preset: unknown): LayoutWriteResult {
  if (
    typeof preset !== "string" ||
    !DEFAULT_PRESET_NAMES.some((candidate) => candidate === preset)
  ) {
    return { ok: false, code: "invalid_layout_preset" };
  }

  return saveLayout(storage, copyLayout(DEFAULT_LAYOUTS[preset as DefaultLayoutName]));
}
