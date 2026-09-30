export const CARD_IDS = ["status", "usage", "session", "log", "stats"] as const;
export type CardId = (typeof CARD_IDS)[number];

export const CARD_SIZES = ["compact", "standard", "wide"] as const;
export type CardSize = (typeof CARD_SIZES)[number];

export const LAYOUT_PRESETS = ["custom", "minimal", "monitoring", "debugging"] as const;
export type LayoutPreset = (typeof LAYOUT_PRESETS)[number];

export const REQUIRED_CARD_IDS = ["status", "log"] as const satisfies readonly CardId[];

export interface CardPlacement {
  id: CardId;
  size: CardSize;
  visible: boolean;
  height?: number;
}

export interface LayoutConfigV1 {
  version: 1;
  preset: LayoutPreset;
  cards: CardPlacement[];
}

export type LayoutValidationError =
  | "invalid_layout_shape"
  | "unsupported_layout_version"
  | "invalid_layout_preset"
  | "invalid_card_shape"
  | "unknown_card_id"
  | "duplicate_card_id"
  | "invalid_card_size"
  | "invalid_card_height"
  | "invalid_card_visibility"
  | "missing_required_card"
  | "hidden_required_card"
  | "missing_card_id";

export type LayoutValidationResult =
  | { ok: true; value: LayoutConfigV1 }
  | { ok: false; code: LayoutValidationError };

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function isOneOf<const T extends readonly string[]>(
  value: unknown,
  allowed: T,
): value is T[number] {
  return typeof value === "string" && allowed.some((candidate) => candidate === value);
}

function invalid(code: LayoutValidationError): LayoutValidationResult {
  return { ok: false, code };
}

/**
 * Validates untrusted persisted layout data and returns a fresh, whitelisted
 * value. Invalid input is reported as data and never escapes as an exception.
 */
export function validateLayout(input: unknown): LayoutValidationResult {
  try {
    if (!isRecord(input)) return invalid("invalid_layout_shape");
    if (input.version !== 1) return invalid("unsupported_layout_version");
    if (!isOneOf(input.preset, LAYOUT_PRESETS)) return invalid("invalid_layout_preset");
    if (!Array.isArray(input.cards)) return invalid("invalid_layout_shape");

    const cards: CardPlacement[] = [];
    const seen = new Set<CardId>();
    for (const candidate of input.cards) {
      if (!isRecord(candidate)) return invalid("invalid_card_shape");
      if (!isOneOf(candidate.id, CARD_IDS)) return invalid("unknown_card_id");
      if (seen.has(candidate.id)) return invalid("duplicate_card_id");
      if (!isOneOf(candidate.size, CARD_SIZES)) return invalid("invalid_card_size");
      if (typeof candidate.visible !== "boolean") return invalid("invalid_card_visibility");
      if (candidate.height !== undefined && (typeof candidate.height !== "number" || !Number.isInteger(candidate.height) || candidate.height < 96 || candidate.height > 600)) return invalid("invalid_card_height");

      seen.add(candidate.id);
      cards.push({
        id: candidate.id,
        size: candidate.size,
        visible: candidate.visible,
        ...(candidate.height === undefined ? {} : { height: candidate.height as number }),
      });
    }

    for (const required of REQUIRED_CARD_IDS) {
      const placement = cards.find(({ id }) => id === required);
      if (!placement) return invalid("missing_required_card");
      if (!placement.visible) return invalid("hidden_required_card");
    }
    if (CARD_IDS.some((id) => !seen.has(id))) return invalid("missing_card_id");

    return {
      ok: true,
      value: {
        version: 1,
        preset: input.preset,
        cards,
      },
    };
  } catch {
    return invalid("invalid_layout_shape");
  }
}
