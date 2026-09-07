import type {
  AgentIslandBridge,
  AgentView,
  AgentViewSnapshot,
  SessionView,
} from "../bridge/types";
import {
  createInteractionStore,
  type InteractionKind,
  type InteractionStore,
} from "./interaction-store";

export interface SnapshotRefreshError {
  readonly code: string;
  readonly message: string;
  readonly requestId: number;
}

export interface AgentStoreState {
  readonly snapshot: AgentViewSnapshot | null;
  readonly selectedAgentId: string | null;
  readonly selectedSessionId: string | null;
  readonly selectedAgent: AgentView | null;
  readonly selectedSession: SessionView | null;
  readonly refreshError: SnapshotRefreshError | null;
}

export interface AgentStore {
  current(): AgentStoreState;
  subscribe(subscriber: (state: AgentStoreState) => void): () => void;
  refresh(): Promise<boolean>;
  applySnapshot(snapshot: AgentViewSnapshot): boolean;
  selectAgent(agentId: string): void;
  selectSession(sessionId: string): void;
  beginInteraction(kind: InteractionKind): () => void;
  endInteraction(kind: InteractionKind): void;
  readonly interaction: InteractionStore;
}

interface SnapshotCandidate {
  readonly snapshot: AgentViewSnapshot;
  readonly order: number;
}

function deepFreeze<T>(value: T, seen = new WeakSet<object>()): T {
  if (value === null || typeof value !== "object" || seen.has(value)) return value;
  seen.add(value);
  for (const child of Object.values(value)) deepFreeze(child, seen);
  return Object.freeze(value);
}

function ownSnapshot(snapshot: AgentViewSnapshot): AgentViewSnapshot {
  return deepFreeze(structuredClone(snapshot));
}

function structurallyEqual(left: unknown, right: unknown): boolean {
  if (Object.is(left, right)) return true;
  if (
    left === null ||
    right === null ||
    typeof left !== "object" ||
    typeof right !== "object"
  ) {
    return false;
  }
  if (Array.isArray(left) || Array.isArray(right)) {
    return (
      Array.isArray(left) &&
      Array.isArray(right) &&
      left.length === right.length &&
      left.every((value, index) => structurallyEqual(value, right[index]))
    );
  }
  const leftRecord = left as Record<string, unknown>;
  const rightRecord = right as Record<string, unknown>;
  const leftKeys = Object.keys(leftRecord).sort();
  const rightKeys = Object.keys(rightRecord).sort();
  return (
    leftKeys.length === rightKeys.length &&
    leftKeys.every(
      (key, index) =>
        key === rightKeys[index] && structurallyEqual(leftRecord[key], rightRecord[key]),
    )
  );
}

function sessionsFor(agent: AgentView): readonly SessionView[] {
  return agent.active_session
    ? [agent.active_session, ...agent.history_sessions]
    : agent.history_sessions;
}

function selectedEntities(
  snapshot: AgentViewSnapshot | null,
  agentId: string | null,
  sessionId: string | null,
): Pick<AgentStoreState, "selectedAgent" | "selectedSession"> {
  const selectedAgent =
    snapshot?.agents.find((candidate) => candidate.id === agentId) ?? null;
  const selectedSession = selectedAgent
    ? sessionsFor(selectedAgent).find((candidate) => candidate.id === sessionId) ?? null
    : null;
  return { selectedAgent, selectedSession };
}

function stateWithSelection(
  state: Pick<AgentStoreState, "snapshot" | "refreshError">,
  selectedAgentId: string | null,
  selectedSessionId: string | null,
): AgentStoreState {
  return deepFreeze({
    ...state,
    selectedAgentId,
    selectedSessionId,
    ...selectedEntities(state.snapshot, selectedAgentId, selectedSessionId),
  });
}

function reconcile(
  previous: AgentStoreState,
  snapshot: AgentViewSnapshot,
  clearRefreshError: boolean,
): AgentStoreState {
  const retainedAgent = snapshot.agents.find(
    (candidate) => candidate.id === previous.selectedAgentId,
  );
  const selectedAgent = retainedAgent ?? snapshot.agents[0] ?? null;
  if (!selectedAgent) {
    return stateWithSelection(
      { snapshot, refreshError: clearRefreshError ? null : previous.refreshError },
      null,
      null,
    );
  }

  const selectedSession = retainedAgent
    ? sessionsFor(selectedAgent).find(
        (candidate) => candidate.id === previous.selectedSessionId,
      )
    : undefined;
  const selectedSessionId =
    selectedSession?.id ?? selectedAgent.active_session?.id ?? null;
  return stateWithSelection(
    { snapshot, refreshError: clearRefreshError ? null : previous.refreshError },
    selectedAgent.id,
    selectedSessionId,
  );
}

function refreshError(error: unknown, requestId: number): SnapshotRefreshError {
  const message = error instanceof Error ? error.message : String(error);
  const code = /^[a-z][a-z0-9_]*$/.test(message) ? message : "snapshot_refresh_failed";
  return { code, message, requestId };
}

function isNewer(candidate: SnapshotCandidate, reference: SnapshotCandidate | null): boolean {
  return (
    reference === null ||
    candidate.snapshot.generated_at_ms > reference.snapshot.generated_at_ms ||
    (candidate.snapshot.generated_at_ms === reference.snapshot.generated_at_ms &&
      candidate.order > reference.order)
  );
}

export function createAgentStore(bridge: AgentIslandBridge): AgentStore {
  const interaction = createInteractionStore();
  const subscribers = new Set<(state: AgentStoreState) => void>();
  let state: AgentStoreState = stateWithSelection(
    { snapshot: null, refreshError: null },
    null,
    null,
  );
  let currentCandidate: SnapshotCandidate | null = null;
  let pendingCandidate: SnapshotCandidate | null = null;
  let candidateOrder = 0;
  let refreshRequest = 0;
  let latestSettledRefresh = 0;
  let latestRefreshErrorOrder = 0;

  function publish(next: AgentStoreState): void {
    state = deepFreeze(next);
    for (const subscriber of [...subscribers]) {
      try {
        subscriber(state);
      } catch {
        subscribers.delete(subscriber);
      }
    }
  }

  function applyCandidate(candidate: SnapshotCandidate): boolean {
    const newestKnown =
      pendingCandidate && isNewer(pendingCandidate, currentCandidate)
        ? pendingCandidate
        : currentCandidate;
    if (
      newestKnown &&
      candidate.snapshot.generated_at_ms === newestKnown.snapshot.generated_at_ms &&
      structurallyEqual(candidate.snapshot, newestKnown.snapshot)
    ) {
      return false;
    }
    if (!isNewer(candidate, newestKnown)) return false;
    if (interaction.current().interacting) {
      pendingCandidate = candidate;
      return true;
    }
    currentCandidate = candidate;
    publish(
      reconcile(state, candidate.snapshot, candidate.order > latestRefreshErrorOrder),
    );
    return true;
  }

  let priorInteractionDepth = 0;
  interaction.subscribe((interactionState) => {
    const becameIdle = priorInteractionDepth > 0 && interactionState.totalDepth === 0;
    priorInteractionDepth = interactionState.totalDepth;
    if (!becameIdle || !pendingCandidate) return;
    const candidate = pendingCandidate;
    pendingCandidate = null;
    if (isNewer(candidate, currentCandidate)) {
      currentCandidate = candidate;
      publish(
        reconcile(state, candidate.snapshot, candidate.order > latestRefreshErrorOrder),
      );
    }
  });

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
    async refresh() {
      const requestId = ++refreshRequest;
      let snapshot: AgentViewSnapshot;
      try {
        snapshot = await bridge.getSnapshot();
      } catch (error) {
        if (requestId < latestSettledRefresh) return false;
        latestSettledRefresh = requestId;
        latestRefreshErrorOrder = ++candidateOrder;
        publish({ ...state, refreshError: refreshError(error, requestId) });
        return false;
      }
      if (requestId < latestSettledRefresh) return false;
      latestSettledRefresh = requestId;
      return applyCandidate({
        snapshot: ownSnapshot(snapshot),
        order: ++candidateOrder,
      });
    },
    applySnapshot(snapshot) {
      return applyCandidate({
        snapshot: ownSnapshot(snapshot),
        order: ++candidateOrder,
      });
    },
    selectAgent(agentId) {
      const selectedAgent = state.snapshot?.agents.find(
        (candidate) => candidate.id === agentId,
      );
      if (!selectedAgent || selectedAgent.id === state.selectedAgentId) return;
      publish(
        stateWithSelection(
          state,
          selectedAgent.id,
          selectedAgent.active_session?.id ?? null,
        ),
      );
    },
    selectSession(sessionId) {
      if (!state.selectedAgent) return;
      const selectedSession = sessionsFor(state.selectedAgent).find(
        (candidate) => candidate.id === sessionId,
      );
      if (!selectedSession || selectedSession.id === state.selectedSessionId) return;
      publish(stateWithSelection(state, state.selectedAgent.id, selectedSession.id));
    },
    beginInteraction: interaction.beginInteraction,
    endInteraction: interaction.endInteraction,
    interaction,
  };
}
