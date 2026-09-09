import {
  CARD_SIZES,
  REQUIRED_CARD_IDS,
  validateLayout,
  type CardPlacement,
  type CardSize,
  type LayoutConfigV1,
} from "./schema";

function copyCards(cards: readonly CardPlacement[]): CardPlacement[] {
  return cards.map(({ id, size, visible }) => ({ id, size, visible }));
}

function changedLayout(layout: LayoutConfigV1, cards: CardPlacement[]): LayoutConfigV1 {
  return { version: 1, preset: "custom", cards };
}

function validSource(layout: LayoutConfigV1): boolean {
  return validateLayout(layout).ok;
}

function isCardSize(value: string): value is CardSize {
  return CARD_SIZES.some((candidate) => candidate === value);
}

export function moveCard(
  layout: LayoutConfigV1,
  cardId: string,
  targetIndex: number,
): LayoutConfigV1 {
  if (!validSource(layout) || Number.isNaN(targetIndex)) return layout;
  const sourceIndex = layout.cards.findIndex(({ id }) => id === cardId);
  if (sourceIndex < 0) return layout;

  const lastIndex = layout.cards.length - 1;
  const integerTarget = Number.isFinite(targetIndex)
    ? Math.trunc(targetIndex)
    : targetIndex < 0 ? 0 : lastIndex;
  const clampedTarget = Math.max(0, Math.min(lastIndex, integerTarget));
  if (sourceIndex === clampedTarget) return layout;

  const cards = copyCards(layout.cards);
  const [moved] = cards.splice(sourceIndex, 1);
  cards.splice(clampedTarget, 0, moved);
  return changedLayout(layout, cards);
}

export function resizeCard(
  layout: LayoutConfigV1,
  cardId: string,
  size: string,
): LayoutConfigV1 {
  if (!validSource(layout) || !isCardSize(size)) return layout;
  const index = layout.cards.findIndex(({ id }) => id === cardId);
  if (index < 0 || layout.cards[index].size === size) return layout;

  const cards = copyCards(layout.cards);
  cards[index] = { ...cards[index], size };
  return changedLayout(layout, cards);
}

export function setCardVisible(
  layout: LayoutConfigV1,
  cardId: string,
  visible: boolean,
): LayoutConfigV1 {
  if (!validSource(layout)) return layout;
  const index = layout.cards.findIndex(({ id }) => id === cardId);
  if (index < 0 || layout.cards[index].visible === visible) return layout;
  if (!visible && REQUIRED_CARD_IDS.some((requiredId) => requiredId === cardId)) return layout;

  const cards = copyCards(layout.cards);
  cards[index] = { ...cards[index], visible };
  return changedLayout(layout, cards);
}
