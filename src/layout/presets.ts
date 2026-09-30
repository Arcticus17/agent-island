import type { CardPlacement, LayoutConfigV1 } from "./schema";

export type DefaultLayoutName = Exclude<LayoutConfigV1["preset"], "custom">;

function layout(
  preset: DefaultLayoutName,
  cards: CardPlacement[],
): LayoutConfigV1 {
  return { version: 1, preset, cards };
}

export const DEFAULT_LAYOUTS: Record<DefaultLayoutName, LayoutConfigV1> = {
  minimal: layout("minimal", [
    { id: "status", size: "compact", visible: true },
    { id: "log", size: "wide", visible: true },
    { id: "session", size: "standard", visible: false },
    { id: "usage", size: "compact", visible: false },
    { id: "stats", size: "standard", visible: false },
  ]),
  monitoring: layout("monitoring", [
    { id: "status", size: "compact", visible: true },
    { id: "usage", size: "compact", visible: true },
    { id: "log", size: "wide", visible: true },
    { id: "session", size: "standard", visible: true },
    { id: "stats", size: "standard", visible: true },
  ]),
  debugging: layout("debugging", [
    { id: "status", size: "compact", visible: true },
    { id: "session", size: "wide", visible: true },
    { id: "log", size: "wide", visible: true },
    { id: "stats", size: "standard", visible: true },
    { id: "usage", size: "standard", visible: true },
  ]),
};
