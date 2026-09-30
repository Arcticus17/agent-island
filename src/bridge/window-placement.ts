export const WINDOW_PLACEMENT_KEY = "agent-island-window-v1";
export interface WindowPlacement { x: number; y: number; width: number }
export interface PlacementMonitor {
  scaleFactor: number;
  workArea: { position: { x: number; y: number }; size: { width: number; height: number } };
}

export function parseWindowPlacement(raw: string | null): WindowPlacement | null {
  if (!raw) return null;
  try {
    const value = JSON.parse(raw);
    if (value?.version !== 1 || ![value.x, value.y, value.width].every(Number.isFinite) || value.width <= 0) return null;
    return { x: value.x, y: value.y, width: value.width };
  } catch { return null; }
}

/** Positions are physical pixels; width and height are logical CSS pixels. */
export function clampWindowPlacement(value: WindowPlacement, monitors: PlacementMonitor[], height = 60): WindowPlacement {
  const valid = monitors.filter(m => Number.isFinite(m.scaleFactor) && m.scaleFactor > 0 && m.workArea.size.width > 0 && m.workArea.size.height > 0);
  if (!valid.length) throw new Error("无法读取显示器工作区域");
  const monitor = valid.find(m => value.x >= m.workArea.position.x && value.x < m.workArea.position.x + m.workArea.size.width && value.y >= m.workArea.position.y && value.y < m.workArea.position.y + m.workArea.size.height) ?? valid[0];
  const { position, size } = monitor.workArea;
  const width = Math.min(Math.max(360, value.width), 744, size.width / monitor.scaleFactor);
  return {
    width,
    x: Math.round(Math.max(position.x, Math.min(value.x, position.x + size.width - width * monitor.scaleFactor))),
    y: Math.round(Math.max(position.y, Math.min(value.y, position.y + size.height - Math.min(height * monitor.scaleFactor, size.height)))),
  };
}
