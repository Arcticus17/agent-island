export const MIN_ISLAND_WIDTH = 280;
export const MIN_EXPANDED_HEIGHT = 260;
export const VIEWPORT_GUTTER = 16;

export function getSafeIslandWidth(preferredWidth, availableWidth) {
  const preferred = Number.isFinite(preferredWidth) ? preferredWidth : MIN_ISLAND_WIDTH;
  const available = Number.isFinite(availableWidth) && availableWidth > 0
    ? Math.floor(availableWidth) - VIEWPORT_GUTTER
    : MIN_ISLAND_WIDTH;
  return Math.min(Math.max(MIN_ISLAND_WIDTH, preferred), Math.max(1, available));
}

export function getSafeIslandHeight(preferredHeight, availableHeight) {
  const preferred = Number.isFinite(preferredHeight) ? preferredHeight : MIN_EXPANDED_HEIGHT;
  const available = Number.isFinite(availableHeight) && availableHeight > 0
    ? Math.floor(availableHeight) - VIEWPORT_GUTTER
    : MIN_EXPANDED_HEIGHT;
  return Math.min(Math.max(MIN_EXPANDED_HEIGHT, preferred), Math.max(1, available));
}

export function clampIslandX(x, minX, maxX, islandWidth) {
  const width = Math.max(0, Number(islandWidth) || 0);
  const left = Number.isFinite(minX) ? minX : 0;
  const right = Number.isFinite(maxX) ? maxX : left + width;
  const maxLeft = Math.max(left, right - width);
  return Math.min(Math.max(Number(x) || 0, left), maxLeft);
}
