export type InteractionKind = "input" | "drag" | "approval";

export interface InteractionDepths {
  readonly input: number;
  readonly drag: number;
  readonly approval: number;
}

export interface InteractionState {
  readonly depths: InteractionDepths;
  readonly totalDepth: number;
  readonly interacting: boolean;
}

export interface InteractionStore {
  current(): InteractionState;
  subscribe(subscriber: (state: InteractionState) => void): () => void;
  beginInteraction(kind: InteractionKind): () => void;
  endInteraction(kind: InteractionKind): void;
}

function nextState(depths: InteractionDepths): InteractionState {
  const stableDepths = Object.freeze({ ...depths });
  const totalDepth = stableDepths.input + stableDepths.drag + stableDepths.approval;
  return Object.freeze({
    depths: stableDepths,
    totalDepth,
    interacting: totalDepth > 0,
  });
}

export function createInteractionStore(): InteractionStore {
  let state = nextState({ input: 0, drag: 0, approval: 0 });
  const subscribers = new Set<(state: InteractionState) => void>();

  function publish(depths: InteractionDepths): void {
    state = nextState(depths);
    for (const subscriber of [...subscribers]) {
      try {
        subscriber(state);
      } catch {
        subscribers.delete(subscriber);
      }
    }
  }

  function endInteraction(kind: InteractionKind): void {
    if (state.depths[kind] === 0) return;
    publish({
      ...state.depths,
      [kind]: state.depths[kind] - 1,
    });
  }

  return {
    current: () => state,
    subscribe(subscriber) {
      subscribers.add(subscriber);
      try {
        subscriber(state);
      } catch {
        subscribers.delete(subscriber);
      }
      return () => subscribers.delete(subscriber);
    },
    beginInteraction(kind) {
      publish({
        ...state.depths,
        [kind]: state.depths[kind] + 1,
      });
      let released = false;
      return () => {
        if (released) return;
        released = true;
        endInteraction(kind);
      };
    },
    endInteraction,
  };
}

export interface LogViewport {
  readonly scrollTop: number;
  readonly clientHeight: number;
  readonly scrollHeight: number;
}

/** Capture this before a snapshot render; scroll after render only when true. */
export function shouldFollowLog(viewport: LogViewport, thresholdPx = 24): boolean {
  if (
    !Number.isFinite(viewport.scrollTop) ||
    !Number.isFinite(viewport.clientHeight) ||
    !Number.isFinite(viewport.scrollHeight) ||
    !Number.isFinite(thresholdPx) ||
    viewport.scrollTop < 0 ||
    viewport.clientHeight < 0 ||
    viewport.scrollHeight < 0 ||
    thresholdPx < 0
  ) {
    return false;
  }
  const distanceFromBottom =
    viewport.scrollHeight - viewport.clientHeight - viewport.scrollTop;
  return distanceFromBottom <= thresholdPx;
}
